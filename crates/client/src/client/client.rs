//! Client machine: 1:1 skeleton of `webclient/src/client/Client.ts`.
//!
//! Java-public fields used by login and later `RawClient`-style reads start
//! here (`ingame`, `loop_cycle` as an instance field, `loginUser`, `loginPass`,
//! `out`, `in`, menu arrays). `login` runs the 274 handshake (wrapper opcode
//! 16 cold / 18 reconnect) over Java-style TCP `ClientStream`.
//! There is no snapshot/query API.

use std::io;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use num_bigint::BigUint;

use crate::client::config::ClientConfig;
use crate::client::game_shell::GameShell;
use crate::client::login_error::LoginError;
use crate::client::mini_menu_action::MiniMenuAction;
use crate::client::skill::Skill;
use crate::config::if_type::ComponentType;
use crate::config::seq_type::{RESTART_RESET, RESTART_RESETLOOP};
use crate::config::Cache;
use crate::dash3d::world::LevelHeightmaps;
use crate::dash3d::{BuildArea, CollisionFlag, CollisionMap, DirectionFlag, LocShape, World};
pub use crate::dash3d::{ClientNpc, ClientPlayer};
use crate::io::{ClientProt, ClientStream, Isaac, Packet, ServerProt, SERVER_PROT_SIZES};
use crate::login_rsa::{LOGIN_RSAE, LOGIN_RSAN};
use crate::util::JString;

const MAX_PLAYER_COUNT: usize = 2048;
const MAX_NPC_COUNT: usize = 16384;
const MENU_CAPACITY: usize = 500;
const CLIENT_VERSION: i32 = 274;
const LOGIN_UID: i32 = 1337;

/// Index of the local player in `players` (`Client.ts` `LOCAL_PLAYER_INDEX`).
const LOCAL_PLAYER_INDEX: i32 = 2047;

/// `PlayerUpdate` mask enum from client-ts `dash3d/ClientPlayer.ts`.
mod player_update {
    pub const APPEARANCE: i32 = 0x1;
    pub const ANIM: i32 = 0x2;
    pub const FACEENTITY: i32 = 0x4;
    pub const SAY: i32 = 0x8;
    pub const HITMARK: i32 = 0x10;
    pub const FACESQUARE: i32 = 0x20;
    pub const CHAT: i32 = 0x40;
    pub const BIG_UPDATE: i32 = 0x80;
    pub const SPOTANIM: i32 = 0x100;
    pub const EXACTMOVE: i32 = 0x200;
    pub const HITMARK2: i32 = 0x400;
}

/// `NpcUpdate` mask enum from client-ts `dash3d/ClientNpc.ts`.
mod npc_update {
    pub const HITMARK2: i32 = 0x1;
    pub const ANIM: i32 = 0x2;
    pub const FACEENTITY: i32 = 0x4;
    pub const SAY: i32 = 0x8;
    pub const HITMARK: i32 = 0x10;
    pub const CHANGETYPE: i32 = 0x20;
    pub const SPOTANIM: i32 = 0x40;
    pub const FACESQUARE: i32 = 0x80;
}

/// Side of the build area, `BuildArea.SIZE` (13 << 3) in client-ts.
const BUILD_AREA_SIZE: i32 = 104;
const BUILD_AREA_TILES: usize = (BUILD_AREA_SIZE * BUILD_AREA_SIZE) as usize;
const ROUTE_BUFFER: usize = 4000;

/// JAG archives whose CRC values go out in the login wrapper; slot 0 of the
/// 9-slot `getJagChecksums` layout has no pack file and stays 0.
const JAG_FILES: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];

pub struct Client {
    pub shell: GameShell,
    pub config: ClientConfig,
    /// Config type tables (`obj`, `npc`, `loc`, ...), unpacked from the
    /// `config` jag by `Cache::unpack`; empty until loaded.
    pub cache: Cache,

    pub ingame: bool,
    pub scene_state: i32,
    pub local_player: Option<ClientPlayer>,
    pub players: Vec<Option<ClientPlayer>>,
    pub npc: Vec<Option<ClientNpc>>,

    /// `PLAYER_INFO`/`NPC_INFO` bookkeeping from client-ts: `playerCount`/
    /// `playerIds`, `npcCount`/`npcIds`, the removal/update id lists, and
    /// the raw appearance blocks re-applied when a player re-enters.
    pub player_count: i32,
    pub player_ids: Vec<i32>,
    pub npc_count: i32,
    pub npc_ids: Vec<i32>,
    pub entity_removal_count: i32,
    pub entity_removal_ids: Vec<i32>,
    pub entity_update_count: i32,
    pub entity_update_ids: Vec<i32>,
    pub player_appearance_buffer: Vec<Option<Packet>>,

    /// Scene height map, `groundh[level][x][z]` sized `[4][105][105]`; owned
    /// here and copied into `world` (Task 16's `ClientBuild` owns the write
    /// side and will reconcile the sharing).
    pub groundh: LevelHeightmaps,
    pub world: World,
    /// One collision grid per level, `CollisionMap` for the 4 build levels.
    pub collision: [CollisionMap; 4],
    pub minusedlevel: i32,
    pub zone_update_x: i32,
    pub zone_update_z: i32,

    pub stat_base_level: Vec<i32>,
    pub stat_effective_level: Vec<i32>,
    pub stat_xp: Vec<i32>,
    pub var: Vec<i32>,
    /// Server-authoritative var values (`varServ` from client-ts); `var`
    /// follows them once `VARP_SYNC` confirms.
    pub var_serv: Vec<i32>,
    pub runenergy: i32,

    /// Music control plane (`Client.ts` `midiActive`/`midiSong`/...). The
    /// `Midi` backend and the on-demand archive-2 request land with Task 19.
    pub midi_active: bool,
    pub midi_song: i32,
    pub next_midi_song: i32,
    pub next_music_delay: i32,
    pub midi_fading: bool,

    pub menu_num_entries: i32,
    pub menu_option: Vec<String>,
    pub menu_action: Vec<i32>,
    pub menu_param_a: Vec<i32>,
    pub menu_param_b: Vec<i32>,
    pub menu_param_c: Vec<i32>,

    pub dir_map: Vec<i32>,
    pub dist_map: Vec<i32>,
    pub route_x: Vec<i32>,
    pub route_z: Vec<i32>,
    pub try_move_nearest: i32,
    pub minimap_flag_x: i32,
    pub minimap_flag_z: i32,
    pub map_build_base_x: i32,
    pub map_build_base_z: i32,
    pub map_build_centre_zone_x: i32,
    pub map_build_centre_zone_z: i32,
    pub map_build_prev_base_x: i32,
    pub map_build_prev_base_z: i32,
    pub within_tutorial_island: bool,
    pub awaiting_player_info: bool,
    pub scene_load_start_time: Instant,
    pub map_build_index: Vec<i32>,
    pub map_build_ground_file: Vec<i32>,
    pub map_build_location_file: Vec<i32>,
    pub map_build_ground_data: Vec<Option<Vec<u8>>>,
    pub map_build_location_data: Vec<Option<Vec<u8>>>,

    pub use_mode: i32,
    pub target_mode: i32,
    pub redraw_side: bool,
    pub target_com_id: i32,
    pub obj_com_id: i32,
    pub obj_selected_slot: i32,
    pub obj_selected_com_id: i32,

    pub out: Packet,
    pub r#in: Packet,
    pub ptype: i32,
    pub ptype0: i32,
    pub ptype1: i32,
    pub ptype2: i32,
    pub psize: i32,

    pub stream: Option<ClientStream>,
    pub staffmodlevel: i32,
    pub mouse_tracked: bool,
    pub random_in: Option<Isaac>,
    pub jag_checksum: [i32; 9],

    pub login_user: String,
    pub login_pass: String,
    pub login_mes1: String,
    pub login_mes2: String,
    pub loop_cycle: i32,
}

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        let jag_checksum = Self::read_jag_checksums(&config.cache_dir);
        let groundh: LevelHeightmaps =
            vec![
                vec![vec![0i32; (BUILD_AREA_SIZE + 1) as usize]; (BUILD_AREA_SIZE + 1) as usize];
                BuildArea::LEVELS as usize
            ];
        Client {
            shell: GameShell::new(),
            config,
            cache: Cache::default(),

            ingame: false,
            scene_state: 0,
            local_player: None,
            players: vec![None; MAX_PLAYER_COUNT],
            npc: vec![None; MAX_NPC_COUNT],

            player_count: 0,
            player_ids: vec![0; MAX_PLAYER_COUNT],
            npc_count: 0,
            npc_ids: vec![0; MAX_NPC_COUNT],
            entity_removal_count: 0,
            entity_removal_ids: vec![0; 1000],
            entity_update_count: 0,
            entity_update_ids: vec![0; MAX_PLAYER_COUNT],
            player_appearance_buffer: (0..MAX_PLAYER_COUNT).map(|_| None).collect(),

            groundh: groundh.clone(),
            world: World::new(groundh, BuildArea::SIZE, BuildArea::LEVELS, BuildArea::SIZE),
            collision: [
                CollisionMap::new(),
                CollisionMap::new(),
                CollisionMap::new(),
                CollisionMap::new(),
            ],
            minusedlevel: 0,
            zone_update_x: 0,
            zone_update_z: 0,

            stat_base_level: vec![0; Skill::count],
            stat_effective_level: vec![0; Skill::count],
            stat_xp: vec![0; Skill::count],
            var: Vec::new(),
            var_serv: Vec::new(),
            runenergy: 0,

            midi_active: true,
            midi_song: -1,
            next_midi_song: -1,
            next_music_delay: 0,
            midi_fading: true,

            menu_num_entries: 0,
            menu_option: Vec::new(),
            menu_action: vec![0; MENU_CAPACITY],
            menu_param_a: vec![0; MENU_CAPACITY],
            menu_param_b: vec![0; MENU_CAPACITY],
            menu_param_c: vec![0; MENU_CAPACITY],

            dir_map: vec![0; BUILD_AREA_TILES],
            dist_map: vec![0; BUILD_AREA_TILES],
            route_x: vec![0; ROUTE_BUFFER],
            route_z: vec![0; ROUTE_BUFFER],
            try_move_nearest: 0,
            minimap_flag_x: 0,
            minimap_flag_z: 0,
            map_build_base_x: 0,
            map_build_base_z: 0,
            map_build_centre_zone_x: 0,
            map_build_centre_zone_z: 0,
            map_build_prev_base_x: 0,
            map_build_prev_base_z: 0,
            within_tutorial_island: false,
            awaiting_player_info: false,
            scene_load_start_time: Instant::now(),
            map_build_index: Vec::new(),
            map_build_ground_file: Vec::new(),
            map_build_location_file: Vec::new(),
            map_build_ground_data: Vec::new(),
            map_build_location_data: Vec::new(),

            use_mode: 0,
            target_mode: 0,
            redraw_side: false,
            target_com_id: 0,
            obj_com_id: 0,
            obj_selected_slot: 0,
            obj_selected_com_id: 0,

            out: Packet::alloc(1),
            r#in: Packet::alloc(1),
            ptype: 0,
            ptype0: 0,
            ptype1: 0,
            ptype2: 0,
            psize: 0,

            stream: None,
            staffmodlevel: 0,
            mouse_tracked: false,
            random_in: None,
            jag_checksum,

            login_user: String::new(),
            login_pass: String::new(),
            login_mes1: String::new(),
            login_mes2: String::new(),
            loop_cycle: 0,
        }
    }

    /// CRC of each JAG pack file under `cache_dir`, in the 9-slot layout the
    /// login wrapper sends (title..sounds at slots 1-8, slot 0 = 0). Missing
    /// files read as 0, matching a client with an empty cache.
    fn read_jag_checksums(cache_dir: &str) -> [i32; 9] {
        let mut checksum = [0i32; 9];
        for (slot, name) in JAG_FILES.iter().enumerate() {
            if let Ok(bytes) = std::fs::read(format!("{cache_dir}/{name}")) {
                checksum[slot + 1] = Packet::getcrc(&bytes, 0, bytes.len());
            }
        }
        checksum
    }

    /// Login handshake, 1:1 of `Client.ts` `login` (1719-1867) / Java
    /// `Client.login`: probe, seed, RSA blob, opcode 16/18 wrapper. Response 1
    /// waits 2 s and retries the same attempt; response 2 enters the game;
    /// anything else is `LoginError` with the code and title-screen messages.
    pub fn login(
        &mut self,
        username: &str,
        password: &str,
        reconnect: bool,
    ) -> Result<(), LoginError> {
        if !reconnect {
            self.login_mes1.clear();
            self.login_mes2 = "Connecting to server...".into();
        }

        let mut stream = match ClientStream::connect(&self.config.host, self.config.port) {
            Ok(s) => s,
            Err(_) => return Err(io_error()),
        };

        let userhash = JString::to_userhash(username);
        let login_server = ((userhash >> 16) & 0x1f) as i32;

        self.out.pos = 0;
        self.out.p1(14);
        self.out.p1(login_server);
        stream.write(self.out.data(), 2).map_err(|_| io_error())?;

        for _ in 0..8 {
            stream.read().map_err(|_| io_error())?;
        }
        let mut response = stream.read().map_err(|_| io_error())?;

        if response == 0 {
            stream
                .read_bytes(self.r#in.data_mut(), 0, 8)
                .map_err(|_| io_error())?;
            self.r#in.pos = 0;
            let login_seed = self.r#in.g8();
            let mut seed = [
                login_random(),
                login_random(),
                (login_seed >> 32) as i32,
                (login_seed & 0xffff_ffff) as i32,
            ];

            self.out.pos = 0;
            self.out.p1(10);
            self.out.p4(seed[0]);
            self.out.p4(seed[1]);
            self.out.p4(seed[2]);
            self.out.p4(seed[3]);
            self.out.p4(LOGIN_UID);
            self.out.pjstr(username);
            self.out.pjstr(password);
            let n = BigUint::from_str(LOGIN_RSAN).unwrap();
            let e = BigUint::from_str(LOGIN_RSAE).unwrap();
            self.out.rsaenc(&n, &e);

            let mut loginout = Packet::alloc(1);
            if reconnect {
                loginout.p1(18);
            } else {
                loginout.p1(16);
            }
            loginout.p1((self.out.pos + 36 + 1 + 1 + 2) as i32);
            loginout.p1(255);
            loginout.p2(CLIENT_VERSION);
            loginout.p1(if self.config.lowmem { 1 } else { 0 });
            for i in 0..9 {
                loginout.p4(self.jag_checksum[i]);
            }
            loginout.pdata(self.out.data(), 0, self.out.pos);

            self.out.random = Some(Isaac::new(&seed));
            for s in seed.iter_mut() {
                *s = s.wrapping_add(50);
            }
            self.random_in = Some(Isaac::new(&seed));

            stream
                .write(loginout.data(), loginout.pos)
                .map_err(|_| io_error())?;
            response = stream.read().map_err(|_| io_error())?;
        }

        if response == 1 {
            thread::sleep(Duration::from_millis(2000));
            // old stream is dropped (closed); each attempt opens a fresh one
            return self.login(username, password, reconnect);
        }

        if response == 2 {
            self.staffmodlevel = stream.read().map_err(|_| io_error())?;
            self.mouse_tracked = stream.read().map_err(|_| io_error())? == 1;
            self.ingame = true;
            self.out.pos = 0;
            self.r#in.pos = 0;
            self.ptype = -1;
            self.ptype0 = -1;
            self.ptype1 = -1;
            self.ptype2 = -1;
            self.psize = 0;
            self.scene_state = 0;
            self.menu_num_entries = 0;
            self.stream = Some(stream);
            return Ok(());
        }

        let (mes1, mes2): (String, String) = match response {
            3 => (String::new(), "Invalid username or password.".into()),
            4 => (
                "Your account has been disabled.".into(),
                "Please check your message-centre for details.".into(),
            ),
            5 => (
                "Your account is already logged in.".into(),
                "Try again in 60 secs...".into(),
            ),
            6 => (
                "RuneScape has been updated!".into(),
                "Wrong RSA key - run tools/redeploy.sh and rebuild.".into(),
            ),
            7 => (
                "This world is full.".into(),
                "Please use a different world.".into(),
            ),
            8 => ("Unable to connect.".into(), "Login server offline.".into()),
            9 => (
                "Login limit exceeded.".into(),
                "Too many connections from your address.".into(),
            ),
            10 => ("Unable to connect.".into(), "Bad session id.".into()),
            11 => (String::new(), "Please try again.".into()),
            12 => (
                "You need a members account to login to this world.".into(),
                "Please subscribe, or use a different world.".into(),
            ),
            13 => (
                "Could not complete login.".into(),
                "Please try using a different world.".into(),
            ),
            14 => (
                "The server is being updated.".into(),
                "Please wait 1 minute and try again.".into(),
            ),
            16 => (
                "Login attempts exceeded.".into(),
                "Please wait 1 minute and try again.".into(),
            ),
            17 => (
                "You are standing in a members-only area.".into(),
                "To play on this world move to a free area first".into(),
            ),
            20 => (
                "Invalid loginserver requested".into(),
                "Please try using a different world.".into(),
            ),
            -1 => (
                "No response from server".into(),
                "Please try using a different world.".into(),
            ),
            _ => (
                "Unexpected server response".into(),
                "Please try using a different world.".into(),
            ),
        };
        self.login_mes1 = mes1.clone();
        self.login_mes2 = mes2.clone();
        self.stream = Some(stream);
        Err(LoginError {
            code: response,
            mes1,
            mes2,
        })
    }

    /// Linear build-area index, `CollisionMap.index(x, z) = x * SIZE + z`.
    fn collision_index(x: i32, z: i32) -> usize {
        (x * BUILD_AREA_SIZE + z) as usize
    }

    /// Menu dispatch, port of client-ts `Client.ts` `doAction` (8548-9273).
    /// Headless encode paths only: the branches that need config types, chat,
    /// or the scene (`OP_OBJ*`, `OP_LOC*`, `OP_PLAYER*`, `OP_HELD*`,
    /// `INV_BUTTON*`, buttons, examine, friend/ignore) land with Tasks 13/15.
    #[allow(non_snake_case)] // Java name kept for the RawClient mapping
    pub fn doAction(&mut self, option_id: i32) {
        if option_id < 0 {
            return;
        }

        let mut action = self.menu_action[option_id as usize];
        let a = self.menu_param_a[option_id as usize];
        let b = self.menu_param_b[option_id as usize];
        let c = self.menu_param_c[option_id as usize];

        if action >= MiniMenuAction::_PRIORITY {
            action -= MiniMenuAction::_PRIORITY;
        }

        if action == MiniMenuAction::OP_NPC1
            || action == MiniMenuAction::OP_NPC2
            || action == MiniMenuAction::OP_NPC3
            || action == MiniMenuAction::OP_NPC4
            || action == MiniMenuAction::OP_NPC5
        {
            // The original guards on the NPC entity and walks to its tile
            // first (MOVE_OPCLICK), then encodes the action opcode.
            let npc_route = self
                .npc
                .get(a as usize)
                .and_then(|n| n.as_ref())
                .map(|n| (n.route_x[0], n.route_z[0]));
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let (Some((npc_x, npc_z)), Some((px, pz))) = (npc_route, local_route) {
                self.tryMove(px, pz, npc_x, npc_z, false, 1, 1, 0, 0, 0, 2);

                let opcode = match action {
                    MiniMenuAction::OP_NPC1 => ClientProt::OPNPC1.id,
                    MiniMenuAction::OP_NPC2 => ClientProt::OPNPC2.id,
                    MiniMenuAction::OP_NPC3 => ClientProt::OPNPC3.id,
                    MiniMenuAction::OP_NPC4 => ClientProt::OPNPC4.id,
                    _ => ClientProt::OPNPC5.id,
                };
                self.out.p1_enc(opcode);
                self.out.p2(a);
            }
        }

        if action == MiniMenuAction::TGT_NPC {
            self.out.p1_enc(ClientProt::OPNPCT.id);
            self.out.p2(a);
            self.out.p2(self.target_com_id);
        }

        if action == MiniMenuAction::USEHELD_ONNPC {
            self.out.p1_enc(ClientProt::OPNPCU.id);
            self.out.p2(a);
            self.out.p2(self.obj_com_id);
            self.out.p2(self.obj_selected_slot);
            self.out.p2(self.obj_selected_com_id);
        }

        if action == MiniMenuAction::WALK {
            // Headless: no World mouse picking — menuParamB/C are the local
            // destination tiles, walked to straight from the local player.
            // TODO(task 16): the original only picks the tile here and the
            // frame loop writes MOVE_GAMECLICK; drop this inline write when
            // the frame loop is ported or the walk packet double-emits.
            if let Some((px, pz)) = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]))
            {
                self.tryMove(px, pz, b, c, true, 0, 0, 0, 0, 0, 0);
            }
        }

        self.use_mode = 0;
        self.target_mode = 0;
        self.redraw_side = true;
    }

    /// Walk-path encode, port of client-ts `Client.ts` `tryMove` (5608-5869)
    /// against `collision[minusedlevel]`: the BFS honours the `CollisionMap`
    /// flags and the loc-shape / loc-width arrival shortcuts
    /// (`testWall`/`testWDecor`/`testLoc`), then writes the
    /// MOVE_GAMECLICK / MOVE_MINIMAPCLICK / MOVE_OPCLICK packet as the
    /// original. `r#type` matches the TS `type` parameter (0 walk, 1 minimap,
    /// 2 op).
    #[allow(non_snake_case)] // Java name kept for the RawClient mapping
    #[allow(clippy::too_many_arguments)] // Java/TS tryMove signature is fixed
    pub fn tryMove(
        &mut self,
        src_x: i32,
        src_z: i32,
        dx: i32,
        dz: i32,
        try_nearest: bool,
        loc_width: i32,
        loc_length: i32,
        loc_angle: i32,
        loc_shape: i32,
        forceapproach: i32,
        r#type: i32,
    ) -> bool {
        let collision_map = &self.collision[self.minusedlevel as usize];
        let scene_width = BuildArea::SIZE;
        let scene_length = BuildArea::SIZE;

        for x in 0..scene_width {
            for z in 0..scene_length {
                let index = Self::collision_index(x, z);
                self.dir_map[index] = 0;
                self.dist_map[index] = 99999999;
            }
        }

        let mut x = src_x;
        let mut z = src_z;

        self.dir_map[Self::collision_index(src_x, src_z)] = 99;
        self.dist_map[Self::collision_index(src_x, src_z)] = 0;

        let mut steps: usize = 0;
        let mut length: usize = 0;
        self.route_x[steps] = src_x;
        self.route_z[steps] = src_z;
        steps += 1;

        let mut arrived = false;
        let buffer_size = self.route_x.len();
        let flags = &collision_map.flags;

        while length != steps {
            x = self.route_x[length];
            z = self.route_z[length];
            length = (length + 1) % buffer_size;

            if x == dx && z == dz {
                arrived = true;
                break;
            }

            if loc_shape != LocShape::WALL_STRAIGHT {
                if (loc_shape < LocShape::WALLDECOR_STRAIGHT_OFFSET
                    || loc_shape == LocShape::CENTREPIECE_STRAIGHT)
                    && collision_map.test_wall(x, z, dx, dz, loc_shape - 1, loc_angle)
                {
                    arrived = true;
                    break;
                }

                if loc_shape < LocShape::CENTREPIECE_STRAIGHT
                    && collision_map.test_w_decor(x, z, dx, dz, loc_shape - 1, loc_angle)
                {
                    arrived = true;
                    break;
                }
            }

            if loc_width != 0
                && loc_length != 0
                && collision_map.test_loc(x, z, dx, dz, loc_width, loc_length, forceapproach)
            {
                arrived = true;
                break;
            }

            let next_cost = self.dist_map[Self::collision_index(x, z)] + 1;

            if x > 0 {
                let index = Self::collision_index(x - 1, z);
                if self.dir_map[index] == 0
                    && (flags[x as usize - 1][z as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x - 1;
                    self.route_z[steps] = z;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 2;
                    self.dist_map[index] = next_cost;
                }
            }

            if x < scene_width - 1 {
                let index = Self::collision_index(x + 1, z);
                if self.dir_map[index] == 0
                    && (flags[x as usize + 1][z as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x + 1;
                    self.route_z[steps] = z;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 8;
                    self.dist_map[index] = next_cost;
                }
            }

            if z > 0 {
                let index = Self::collision_index(x, z - 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize][z as usize - 1] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x;
                    self.route_z[steps] = z - 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 1;
                    self.dist_map[index] = next_cost;
                }
            }

            if z < scene_length - 1 {
                let index = Self::collision_index(x, z + 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize][z as usize + 1] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x;
                    self.route_z[steps] = z + 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 4;
                    self.dist_map[index] = next_cost;
                }
            }

            if x > 0 && z > 0 {
                let index = Self::collision_index(x - 1, z - 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize - 1][z as usize - 1] & CollisionFlag::PL_WALK_NE) == 0
                    && (flags[x as usize - 1][z as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                    && (flags[x as usize][z as usize - 1] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x - 1;
                    self.route_z[steps] = z - 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 3;
                    self.dist_map[index] = next_cost;
                }
            }

            if x < scene_width - 1 && z > 0 {
                let index = Self::collision_index(x + 1, z - 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize + 1][z as usize - 1] & CollisionFlag::PL_WALK_NW) == 0
                    && (flags[x as usize + 1][z as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                    && (flags[x as usize][z as usize - 1] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x + 1;
                    self.route_z[steps] = z - 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 9;
                    self.dist_map[index] = next_cost;
                }
            }

            if x > 0 && z < scene_length - 1 {
                let index = Self::collision_index(x - 1, z + 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize - 1][z as usize + 1] & CollisionFlag::PL_WALK_SE) == 0
                    && (flags[x as usize - 1][z as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                    && (flags[x as usize][z as usize + 1] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x - 1;
                    self.route_z[steps] = z + 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 6;
                    self.dist_map[index] = next_cost;
                }
            }

            if x < scene_width - 1 && z < scene_length - 1 {
                let index = Self::collision_index(x + 1, z + 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize + 1][z as usize + 1] & CollisionFlag::PL_WALK_SW) == 0
                    && (flags[x as usize + 1][z as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                    && (flags[x as usize][z as usize + 1] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x + 1;
                    self.route_z[steps] = z + 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 12;
                    self.dist_map[index] = next_cost;
                }
            }
        }

        self.try_move_nearest = 0;

        if !arrived {
            if try_nearest {
                let mut min = 100;
                for padding in 1..2 {
                    for px in (dx - padding)..=(dx + padding) {
                        for pz in (dz - padding)..=(dz + padding) {
                            if px >= 0 && pz >= 0 && px < scene_width && pz < scene_length {
                                let index = Self::collision_index(px, pz);
                                if self.dist_map[index] < min {
                                    min = self.dist_map[index];
                                    x = px;
                                    z = pz;
                                    self.try_move_nearest = 1;
                                    arrived = true;
                                }
                            }
                        }
                    }
                    if arrived {
                        break;
                    }
                }
            }

            if !arrived {
                return false;
            }
        }

        length = 0;
        self.route_x[length] = x;
        self.route_z[length] = z;
        length += 1;

        let mut dir = self.dir_map[Self::collision_index(x, z)];
        let mut next = dir;
        while x != src_x || z != src_z {
            if next != dir {
                dir = next;
                self.route_x[length] = x;
                self.route_z[length] = z;
                length += 1;
            }

            if next & DirectionFlag::EAST != 0 {
                x += 1;
            } else if next & DirectionFlag::WEST != 0 {
                x -= 1;
            }

            if next & DirectionFlag::NORTH != 0 {
                z += 1;
            } else if next & DirectionFlag::SOUTH != 0 {
                z -= 1;
            }

            next = self.dir_map[Self::collision_index(x, z)];
        }

        if length > 0 {
            // max number of turns in a single pf request
            let buffer_size = length.min(25);
            length -= 1;

            let start_x = self.route_x[length];
            let start_z = self.route_z[length];

            match r#type {
                0 => {
                    self.out.p1_enc(ClientProt::MOVE_GAMECLICK.id);
                    self.out.p1((buffer_size + buffer_size + 3) as i32);
                }
                1 => {
                    self.out.p1_enc(ClientProt::MOVE_MINIMAPCLICK.id);
                    self.out.p1((buffer_size + buffer_size + 3 + 14) as i32);
                }
                2 => {
                    self.out.p1_enc(ClientProt::MOVE_OPCLICK.id);
                    self.out.p1((buffer_size + buffer_size + 3) as i32);
                }
                _ => {}
            }

            if self.shell.key_held[5] == 1 {
                self.out.p1(1);
            } else {
                self.out.p1(0);
            }

            self.out.p2(start_x + self.map_build_base_x);
            self.out.p2(start_z + self.map_build_base_z);

            self.minimap_flag_x = self.route_x[0];
            self.minimap_flag_z = self.route_z[0];

            let mut i = 1;
            while i < buffer_size {
                length -= 1;
                self.out.p1(self.route_x[length] - start_x);
                self.out.p1(self.route_z[length] - start_z);
                i += 1;
            }

            return true;
        }

        r#type != 1
    }

    /// In-game inbound read, 1:1 of `Client.ts` `tcpIn` (5871-7150) on the
    /// Java-style blocking stream: Isaac-decoded `ptype`, `psize` from
    /// `SERVER_PROT_SIZES` (-1/-2 variable-length forms), full-payload read,
    /// then the `ptype` switch. Returns false when no complete frame is
    /// available yet; `gameLoop` drives it up to 5 times per frame.
    pub fn tcp_in(&mut self) -> bool {
        match self.read_packet() {
            Ok(true) => {
                // `handle_packet` takes the payload out of `self` (it is
                // callable with an external packet in tests), so swap the
                // `in` buffer out for the dispatch and put it back.
                let ptype = self.ptype;
                let mut packet = std::mem::replace(&mut self.r#in, Packet::alloc(1));
                self.handle_packet(ptype, &mut packet);
                self.r#in = packet;
                true
            }
            Ok(false) => false,
            Err(_) => {
                // Java `catch (Exception)`: report and log out. The
                // `lostCon` reconnect path (IO errors) lands with the
                // connection-loss task.
                eprintln!("T2 - {},{},{}", self.ptype, self.ptype1, self.ptype2);
                self.logout();
                true
            }
        }
    }

    /// Read one frame's header and payload into `in`; `Ok(true)` when a full
    /// packet is ready to dispatch.
    fn read_packet(&mut self) -> io::Result<bool> {
        let stream = match self.stream.as_mut() {
            Some(s) => s,
            None => return Ok(false),
        };

        let mut available = stream.available()?;
        if available == 0 {
            return Ok(false);
        }

        if self.ptype == -1 {
            stream.read_bytes(self.r#in.data_mut(), 0, 1)?;
            self.ptype = self.r#in.data()[0] as i32 & 0xff;
            if let Some(random) = self.random_in.as_mut() {
                self.ptype = self.ptype.wrapping_sub(random.next_int()) & 0xff;
            }
            self.psize = SERVER_PROT_SIZES[self.ptype as usize];
            available -= 1;
        }

        if self.psize == -1 {
            if available <= 0 {
                return Ok(false);
            }
            stream.read_bytes(self.r#in.data_mut(), 0, 1)?;
            self.psize = self.r#in.data()[0] as i32 & 0xff;
            available -= 1;
        }

        if self.psize == -2 {
            if available <= 1 {
                return Ok(false);
            }
            stream.read_bytes(self.r#in.data_mut(), 0, 2)?;
            self.r#in.pos = 0;
            self.psize = self.r#in.g2();
            available -= 2;
        }

        if available < self.psize {
            return Ok(false);
        }

        // a length over the `in` buffer is the same AIOOBE the Java client
        // catches as a logic error
        if self.psize as usize > self.r#in.length() {
            return Err(io::Error::other("packet exceeds in buffer"));
        }

        self.r#in.pos = 0;
        stream.read_bytes(self.r#in.data_mut(), 0, self.psize as usize)?;
        self.ptype2 = self.ptype1;
        self.ptype1 = self.ptype0;
        self.ptype0 = self.ptype;
        Ok(true)
    }

    /// Inner `ptype` switch, 1:1 of the `Client.ts` `tcpIn` dispatch. Every
    /// handled packet resets `ptype` to -1 as the TS does; every `ServerProt`
    /// id has a branch here or an explicit no-op matching the TS handler's
    /// effect on the ported state. Unknown opcodes report `T1` and log out
    /// exactly like the TS default. Callable from tests without a socket.
    pub fn handle_packet(&mut self, ptype: i32, payload: &mut Packet) {
        match ptype {
            ServerProt::REBUILD_NORMAL => {
                let zone_x = payload.g2();
                let zone_z = payload.g2();

                if self.map_build_centre_zone_x == zone_x
                    && self.map_build_centre_zone_z == zone_z
                    && self.scene_state == 2
                {
                    self.ptype = -1;
                    return;
                }

                self.map_build_centre_zone_x = zone_x;
                self.map_build_centre_zone_z = zone_z;
                self.map_build_base_x = (self.map_build_centre_zone_x - 6) * 8;
                self.map_build_base_z = (self.map_build_centre_zone_z - 6) * 8;

                self.within_tutorial_island = ((self.map_build_centre_zone_x / 8 == 48
                    || self.map_build_centre_zone_x / 8 == 49)
                    && self.map_build_centre_zone_z / 8 == 48)
                    || (self.map_build_centre_zone_x / 8 == 48
                        && self.map_build_centre_zone_z / 8 == 148);

                self.scene_state = 1;
                self.scene_load_start_time = Instant::now();

                // the "Loading - please wait." splash draws are render-only

                let start_x = (self.map_build_centre_zone_x - 6) / 8;
                let end_x = (self.map_build_centre_zone_x + 6) / 8;
                let start_z = (self.map_build_centre_zone_z - 6) / 8;
                let end_z = (self.map_build_centre_zone_z + 6) / 8;
                let regions = ((end_x - start_x + 1) * (end_z - start_z + 1)) as usize;

                self.map_build_ground_data = vec![None; regions];
                self.map_build_location_data = vec![None; regions];
                self.map_build_index = vec![0; regions];
                self.map_build_ground_file = vec![0; regions];
                self.map_build_location_file = vec![0; regions];

                let mut map_count = 0;
                for x in start_x..=end_x {
                    for z in start_z..=end_z {
                        self.map_build_index[map_count] = (x << 8) + z;

                        if self.within_tutorial_island
                            && (z == 49 || z == 149 || z == 147 || x == 50 || (x == 49 && z == 47))
                        {
                            self.map_build_ground_file[map_count] = -1;
                            self.map_build_location_file[map_count] = -1;
                            map_count += 1;
                        }
                        // TODO(task 18): the OnDemand branch (getMapFile/request
                        // per square) fills ground/location files and bumps
                        // map_count; without it the arrays stay empty and the
                        // scene waits, as TS with `onDemand === null`.
                    }
                }

                let dx = self.map_build_base_x - self.map_build_prev_base_x;
                let dz = self.map_build_base_z - self.map_build_prev_base_z;
                self.map_build_prev_base_x = self.map_build_base_x;
                self.map_build_prev_base_z = self.map_build_base_z;

                for npc in self.npc.iter_mut().flatten() {
                    for j in 0..10 {
                        npc.route_x[j] -= dx;
                        npc.route_z[j] -= dz;
                    }
                    npc.x -= dx * 128;
                    npc.z -= dz * 128;
                }

                for player in self.players.iter_mut().flatten() {
                    for j in 0..10 {
                        player.route_x[j] -= dx;
                        player.route_z[j] -= dz;
                    }
                    player.x -= dx * 128;
                    player.z -= dz * 128;
                }

                self.awaiting_player_info = true;

                // TODO(zone task): groundObj/locChanges tile shifts carry
                // stacked objects and loc changes across a build-area move
                // (those fields land with the zone-packet task).
                if self.minimap_flag_x != 0 {
                    self.minimap_flag_x -= dx;
                    self.minimap_flag_z -= dz;
                }

                self.ptype = -1;
            }

            // interface draw/modal state (mainModalId, sideIcon, activeIcon,
            // chatModalId, tutComId) is not ported yet; these reset ptype
            // like their TS handlers.
            ServerProt::IF_OPENCHAT
            | ServerProt::IF_OPENMAIN_SIDE
            | ServerProt::IF_CLOSE
            | ServerProt::IF_SETICON
            | ServerProt::IF_SHOWICON
            | ServerProt::IF_OPENMAIN
            | ServerProt::IF_OPENSIDE
            | ServerProt::IF_OPENOVERLAY
            | ServerProt::TUT_FLASH
            | ServerProt::TUT_OPEN => {
                self.ptype = -1;
            }

            ServerProt::IF_SETCOLOUR => {
                let com_id = payload.g2();
                let colour = payload.g2();

                let r = (colour >> 10) & 0x1f;
                let g = (colour >> 5) & 0x1f;
                let b = colour & 0x1f;

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.colour = (r << 19) + (g << 11) + (b << 3);
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETHIDE => {
                let com_id = payload.g2();
                let hide = payload.g1() == 1;

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.hide = hide;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETOBJECT => {
                let com_id = payload.g2();
                let obj_id = payload.g2();
                let zoom = payload.g2();

                let (xan2d, yan2d, zoom2d) = if (obj_id as usize) < self.cache.objs.len() {
                    let r#type = self.cache.obj(obj_id as usize);
                    (r#type.xan2d, r#type.yan2d, r#type.zoom2d)
                } else {
                    (0, 0, 0)
                };

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.model1_type = 4;
                    com.model1_id = obj_id;
                    com.model_xan = xan2d;
                    com.model_yan = yan2d;
                    // JS `(x * 100) / zoom | 0` with zoom 0 is Infinity | 0 = 0
                    com.model_zoom = if zoom == 0 { 0 } else { (zoom2d * 100) / zoom };
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETMODEL => {
                let com_id = payload.g2();
                let model_id = payload.g2();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.model1_type = 1;
                    com.model1_id = model_id;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETANIM => {
                let com_id = payload.g2();
                let seq_id = payload.g2b();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.model_anim = seq_id;
                    if seq_id == -1 {
                        com.anim_frame = 0;
                        com.anim_cycle = 0;
                    }
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETPLAYERHEAD => {
                let com_id = payload.g2();

                if let (Some(com), Some(local)) = (
                    self.cache
                        .ifaces
                        .get_mut(com_id as usize)
                        .and_then(|o| o.as_mut()),
                    self.local_player.as_ref(),
                ) {
                    com.model1_type = 3;
                    com.model1_id = (local.appearance[8] as i32) << 6
                        | (local.appearance[0] as i32) << 12
                        | (local.colour[0] as i32) << 24
                        | (local.colour[4] as i32) << 18
                        | local.appearance[11] as i32;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETTEXT => {
                let com_id = payload.g2();
                let text = payload.gjstr();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.text = text;
                    // TS also redraws the side when
                    // layerId === sideIcon[activeIcon] (not ported yet).
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETNPCHEAD => {
                let com_id = payload.g2();
                let npc_id = payload.g2();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.model1_type = 2;
                    com.model1_id = npc_id;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETPOSITION => {
                let com_id = payload.g2();
                let x = payload.g2b();
                let y = payload.g2b();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.x = x;
                    com.y = y;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETSCROLLPOS => {
                let com_id = payload.g2();
                let mut pos = payload.g2();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    if com.r#type == ComponentType::TYPE_LAYER {
                        if pos < 0 {
                            pos = 0;
                        }
                        if pos > com.scroll_height - com.height {
                            pos = com.scroll_height - com.height;
                        }
                        com.scroll_pos = pos;
                    }
                }
                self.ptype = -1;
            }

            // TS `IfType.list[comId].linkObj*` writes; the interface table is
            // not loaded yet so these are inert until the interface jag task.
            ServerProt::UPDATE_INV_STOP_TRANSMIT => {
                let com_id = payload.g2();

                if let Some(inv) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    if let Some(link_types) = inv.link_obj_type.as_mut() {
                        // [sic] TS writes -1 then 0; the final value is 0
                        for t in link_types.iter_mut() {
                            *t = 0;
                        }
                    }
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_INV_FULL => {
                self.redraw_side = true;

                let com_id = payload.g2();
                let size = payload.g1();

                if let Some(inv) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    if let (Some(link_types), Some(link_numbers)) =
                        (inv.link_obj_type.as_mut(), inv.link_obj_number.as_mut())
                    {
                        // TS writes past the end for a short component, growing
                        // the arrays; here the component fixes its own size.
                        let n = size.min(link_types.len() as i32) as usize;
                        for i in 0..n {
                            link_types[i] = payload.g2();

                            let mut count = payload.g1();
                            if count == 255 {
                                count = payload.g4();
                            }
                            link_numbers[i] = count;
                        }
                        for i in n..link_types.len() {
                            link_types[i] = 0;
                            link_numbers[i] = 0;
                        }
                    }
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_INV_PARTIAL => {
                self.redraw_side = true;

                let com_id = payload.g2();

                if let Some(inv) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    if let (Some(link_types), Some(link_numbers)) =
                        (inv.link_obj_type.as_mut(), inv.link_obj_number.as_mut())
                    {
                        // TS loop bound is `in.pos < psize`; the payload is
                        // the whole frame here, so length is the same bound.
                        while payload.pos < payload.length() {
                            let slot = payload.g1();
                            let id = payload.g2();

                            let mut count = payload.g1();
                            if count == 255 {
                                count = payload.g4();
                            }

                            if slot >= 0 && (slot as usize) < link_types.len() {
                                link_types[slot as usize] = id;
                                link_numbers[slot as usize] = count;
                            }
                        }
                    }
                }
                self.ptype = -1;
            }

            // camera state (cinemaCam, camX/Y/Z, camShake*) is not ported yet
            ServerProt::CAM_LOOKAT
            | ServerProt::CAM_SHAKE
            | ServerProt::CAM_MOVETO
            | ServerProt::CAM_RESET => {
                self.ptype = -1;
            }

            ServerProt::NPC_INFO => {
                self.get_npc_pos(payload, self.psize);
                self.ptype = -1;
            }

            ServerProt::PLAYER_INFO => {
                self.get_player_pos(payload, self.psize);
                self.awaiting_player_info = false;
                self.ptype = -1;
            }

            // chat, friends and ignore-list state is not ported yet
            ServerProt::MESSAGE_GAME
            | ServerProt::UPDATE_IGNORELIST
            | ServerProt::CHAT_FILTER_SETTINGS
            | ServerProt::MESSAGE_PRIVATE
            | ServerProt::FRIENDLIST_LOADED
            | ServerProt::UPDATE_FRIENDLIST => {
                self.ptype = -1;
            }

            ServerProt::UNSET_MAP_FLAG => {
                self.minimap_flag_x = 0;
                self.ptype = -1;
            }

            // runweight, hint arrows and the reboot timer are not ported yet
            ServerProt::UPDATE_RUNWEIGHT
            | ServerProt::HINT_ARROW
            | ServerProt::UPDATE_REBOOT_TIMER => {
                self.ptype = -1;
            }

            ServerProt::UPDATE_STAT => {
                self.redraw_side = true;

                let stat = payload.g1();
                let xp = payload.g4();
                let level = payload.g1();

                // TS Int32Array stores silently ignore out-of-range stats
                if stat >= 0 && (stat as usize) < self.stat_xp.len() {
                    self.stat_xp[stat as usize] = xp;
                    self.stat_effective_level[stat as usize] = level;
                    let mut base: i32 = 1;
                    for i in 0..98 {
                        if xp >= level_experience()[i] {
                            base = (i + 2) as i32;
                        }
                    }
                    self.stat_base_level[stat as usize] = base;
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_RUNENERGY => {
                // TS also redraws the side when activeIcon === 12 (not ported)
                self.runenergy = payload.g1();
                self.ptype = -1;
            }

            ServerProt::RESET_ANIMS => {
                for player in self.players.iter_mut().flatten() {
                    player.primary_anim = -1;
                }
                for npc in self.npc.iter_mut().flatten() {
                    npc.primary_anim = -1;
                }
                self.ptype = -1;
            }

            // selfSlot/membersAccount, last-login info, dialog input and
            // friend-slot ops are not ported yet
            ServerProt::UPDATE_PID
            | ServerProt::LAST_LOGIN_INFO
            | ServerProt::P_COUNTDIALOG
            | ServerProt::SET_MULTIWAY
            | ServerProt::SET_PLAYER_OP
            | ServerProt::MINIMAP_TOGGLE => {
                self.ptype = -1;
            }

            ServerProt::LOGOUT => {
                self.logout();
                self.ptype = -1;
            }

            ServerProt::VARP_SMALL => {
                let varp_id = payload.g2();
                let value = payload.g1b();

                grow_write(&mut self.var_serv, varp_id, value);
                if self.var.get(varp_id as usize).copied() != Some(value) {
                    grow_write(&mut self.var, varp_id, value);
                    // TS also calls clientVar(varpId) (varbit/stat mapping,
                    // midi clientcode 3) — lands with the varp/music tasks.
                    self.redraw_side = true;
                }
                self.ptype = -1;
            }

            ServerProt::VARP_LARGE => {
                let varp_id = payload.g2();
                let value = payload.g4();

                grow_write(&mut self.var_serv, varp_id, value);
                if self.var.get(varp_id as usize).copied() != Some(value) {
                    grow_write(&mut self.var, varp_id, value);
                    // TS also calls clientVar(varpId) (varbit/stat mapping,
                    // midi clientcode 3) — lands with the varp/music tasks.
                    self.redraw_side = true;
                }
                self.ptype = -1;
            }

            ServerProt::VARP_SYNC => {
                // "Resetting variables to authoritative set"
                for i in 0..self.var.len() {
                    if self.var_serv.get(i).copied() != Some(self.var[i]) {
                        if let Some(&value) = self.var_serv.get(i) {
                            self.var[i] = value;
                        } else {
                            // JS `var[i] = varServ[i]` with a missing entry
                            // stores undefined; i32 cannot, so leave 0.
                            self.var[i] = 0;
                        }
                        // TS also calls clientVar(i) (varp task).
                        self.redraw_side = true;
                    }
                }
                self.ptype = -1;
            }

            ServerProt::SYNTH_SOUND => {
                // g2 soundId + g1 loops + g2 delay feed the waveEnabled/waveIds
                // queue, which lands with the JagFX/sound task.
                let _sound_id = payload.g2();
                let _loops = payload.g1();
                let _delay = payload.g2();
                self.ptype = -1;
            }

            ServerProt::MIDI_SONG => {
                let mut song_id = payload.g2();
                if song_id == 65535 {
                    song_id = -1;
                }

                if self.next_midi_song != song_id
                    && self.midi_active
                    && !self.config.lowmem
                    && self.next_music_delay == 0
                {
                    self.midi_song = song_id;
                    self.midi_fading = true;
                    // TS requests archive 2 via onDemand (Task 18).
                }

                self.next_midi_song = song_id;
                self.ptype = -1;
            }

            ServerProt::MIDI_JINGLE => {
                let jingle_id = payload.g2();
                let delay = payload.g2();

                if self.midi_active && !self.config.lowmem {
                    self.midi_song = jingle_id;
                    self.midi_fading = false;
                    self.next_music_delay = delay;
                    // TS requests archive 2 via onDemand (Task 18).
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_ZONE_PARTIAL_FOLLOWS => {
                self.zone_update_x = payload.g1();
                self.zone_update_z = payload.g1();
                self.ptype = -1;
            }

            ServerProt::UPDATE_ZONE_FULL_FOLLOWS => {
                self.zone_update_x = payload.g1();
                self.zone_update_z = payload.g1();
                // TS clears groundObj and expires locChanges in the 8x8 zone;
                // that scene state lands with the zone task.
                self.ptype = -1;
            }

            ServerProt::UPDATE_ZONE_PARTIAL_ENCLOSED => {
                self.zone_update_x = payload.g1();
                self.zone_update_z = payload.g1();

                // TS loop bound is `in.pos < psize`; the payload is the whole
                // frame here, so length is the same bound.
                while payload.pos < payload.length() {
                    let opcode = payload.g1();
                    self.zone_packet(payload, opcode);
                }
                self.ptype = -1;
            }

            // zone protocol, direct dispatch like the TS
            ServerProt::OBJ_COUNT
            | ServerProt::P_LOCMERGE
            | ServerProt::OBJ_REVEAL
            | ServerProt::MAP_ANIM
            | ServerProt::MAP_PROJANIM
            | ServerProt::OBJ_DEL
            | ServerProt::OBJ_ADD
            | ServerProt::LOC_ANIM
            | ServerProt::LOC_DEL
            | ServerProt::LOC_ADD_CHANGE => {
                self.zone_packet(payload, ptype);
                self.ptype = -1;
            }

            _ => {
                // Java/TS report unknown opcodes to the world and log out
                eprintln!(
                    "T1 - {ptype},{} - {},{}",
                    self.psize, self.ptype1, self.ptype2
                );
                self.logout();
            }
        }
    }

    /// `getPlayerPos(buf, size)` from client-ts: unpack the `PLAYER_INFO`
    /// frame into the local player and the `players` table.
    fn get_player_pos(&mut self, buf: &mut Packet, size: i32) {
        self.entity_removal_count = 0;
        self.entity_update_count = 0;

        self.get_player_pos_local(buf);
        self.get_player_pos_old_vis(buf);
        self.get_player_pos_new_vis(buf, size);
        self.get_player_pos_extended(buf);

        for i in 0..self.entity_removal_count as usize {
            let index = self.entity_removal_ids[i] as usize;
            if index < self.players.len() {
                if let Some(player) = &self.players[index] {
                    if player.cycle != self.loop_cycle {
                        self.players[index] = None;
                    }
                }
            }
        }

        if buf.pos != size as usize {
            // TS throws here; the tcpIn catch reports T2 and logs out
            eprintln!(
                "T2 - Error packet size mismatch in getplayer pos:{} psize:{}",
                buf.pos, size
            );
            self.logout();
            return;
        }

        for i in 0..self.player_count as usize {
            let index = self.player_ids[i] as usize;
            if self.players.get(index).is_none_or(|p| p.is_none()) {
                eprintln!(
                    "T2 - {} null entry in pl list - pos:{} size:{}",
                    self.login_user, i, self.player_count
                );
                self.logout();
                return;
            }
        }
    }

    /// `getPlayerPosLocal` from client-ts.
    fn get_player_pos_local(&mut self, buf: &mut Packet) {
        buf.gbit_start();

        let info = buf.gbit(1);
        if info != 0 {
            let op = buf.gbit(2);

            if op == 0 {
                self.entity_update_ids[self.entity_update_count as usize] = LOCAL_PLAYER_INDEX;
                self.entity_update_count += 1;
            } else if op == 1 {
                let walk_dir = buf.gbit(3);
                if let Some(local) = self.local_player.as_mut() {
                    local.move_code(&self.cache, false, walk_dir);
                }

                let extended_info = buf.gbit(1);
                if extended_info == 1 {
                    self.entity_update_ids[self.entity_update_count as usize] = LOCAL_PLAYER_INDEX;
                    self.entity_update_count += 1;
                }
            } else if op == 2 {
                let walk_dir = buf.gbit(3);
                if let Some(local) = self.local_player.as_mut() {
                    local.move_code(&self.cache, true, walk_dir);
                }

                let run_dir = buf.gbit(3);
                if let Some(local) = self.local_player.as_mut() {
                    local.move_code(&self.cache, true, run_dir);
                }

                let extended_info = buf.gbit(1);
                if extended_info == 1 {
                    self.entity_update_ids[self.entity_update_count as usize] = LOCAL_PLAYER_INDEX;
                    self.entity_update_count += 1;
                }
            } else if op == 3 {
                self.minusedlevel = buf.gbit(2);
                let local_x = buf.gbit(7);
                let local_z = buf.gbit(7);
                let jump = buf.gbit(1);

                if let Some(local) = self.local_player.as_mut() {
                    local.teleport(&self.cache, jump == 1, local_x, local_z);
                }

                let extended_info = buf.gbit(1);
                if extended_info == 1 {
                    self.entity_update_ids[self.entity_update_count as usize] = LOCAL_PLAYER_INDEX;
                    self.entity_update_count += 1;
                }
            }
        }
    }

    /// `getPlayerPosOldVis` from client-ts.
    fn get_player_pos_old_vis(&mut self, buf: &mut Packet) {
        let count = buf.gbit(8);

        if count < self.player_count {
            for i in count as usize..self.player_count as usize {
                self.entity_removal_ids[self.entity_removal_count as usize] = self.player_ids[i];
                self.entity_removal_count += 1;
            }
        }

        if count > self.player_count {
            eprintln!("T2 - {} Too many players", self.login_user);
            self.logout();
            return;
        }

        self.player_count = 0;
        for i in 0..count as usize {
            let index = self.player_ids[i] as usize;

            let info = buf.gbit(1);
            if info == 0 {
                self.player_ids[self.player_count as usize] = index as i32;
                self.player_count += 1;
                if let Some(player) = self.players[index].as_mut() {
                    player.cycle = self.loop_cycle;
                }
            } else {
                let op = buf.gbit(2);

                if op == 0 {
                    self.player_ids[self.player_count as usize] = index as i32;
                    self.player_count += 1;
                    if let Some(player) = self.players[index].as_mut() {
                        player.cycle = self.loop_cycle;
                    }
                    self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                    self.entity_update_count += 1;
                } else if op == 1 {
                    self.player_ids[self.player_count as usize] = index as i32;
                    self.player_count += 1;
                    if let Some(player) = self.players[index].as_mut() {
                        player.cycle = self.loop_cycle;
                    }

                    let walk_dir = buf.gbit(3);
                    if let Some(player) = self.players[index].as_mut() {
                        player.move_code(&self.cache, false, walk_dir);
                    }

                    let extended_info = buf.gbit(1);
                    if extended_info == 1 {
                        self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                        self.entity_update_count += 1;
                    }
                } else if op == 2 {
                    self.player_ids[self.player_count as usize] = index as i32;
                    self.player_count += 1;
                    if let Some(player) = self.players[index].as_mut() {
                        player.cycle = self.loop_cycle;
                    }

                    let walk_dir = buf.gbit(3);
                    if let Some(player) = self.players[index].as_mut() {
                        player.move_code(&self.cache, true, walk_dir);
                    }

                    let run_dir = buf.gbit(3);
                    if let Some(player) = self.players[index].as_mut() {
                        player.move_code(&self.cache, true, run_dir);
                    }

                    let extended_info = buf.gbit(1);
                    if extended_info == 1 {
                        self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                        self.entity_update_count += 1;
                    }
                } else {
                    self.entity_removal_ids[self.entity_removal_count as usize] = index as i32;
                    self.entity_removal_count += 1;
                }
            }
        }
    }

    /// `getPlayerPosNewVis` from client-ts.
    fn get_player_pos_new_vis(&mut self, buf: &mut Packet, size: i32) {
        while buf.bit_pos + 10 < (size as usize) * 8 {
            let index = buf.gbit(11);
            if index == 2047 {
                break;
            }
            let index = index as usize;

            if self.players[index].is_none() {
                self.players[index] = Some(ClientPlayer::default());

                if let Some(appearance) = self.player_appearance_buffer[index].take() {
                    if let Some(player) = self.players[index].as_mut() {
                        let mut appearance = appearance;
                        appearance.pos = 0;
                        player.set_appearance(&mut appearance, &self.cache);
                        self.player_appearance_buffer[index] = Some(appearance);
                    }
                }
            }

            self.player_ids[self.player_count as usize] = index as i32;
            self.player_count += 1;
            if let Some(player) = self.players[index].as_mut() {
                player.cycle = self.loop_cycle;
            }

            let mut dx = buf.gbit(5);
            if dx > 15 {
                dx -= 32;
            }
            let mut dz = buf.gbit(5);
            if dz > 15 {
                dz -= 32;
            }
            let jump = buf.gbit(1);

            if let Some(local) = &self.local_player {
                if let Some(player) = self.players[index].as_mut() {
                    player.teleport(
                        &self.cache,
                        jump == 1,
                        local.route_x[0] + dx,
                        local.route_z[0] + dz,
                    );
                }
            }

            let extended_info = buf.gbit(1);
            if extended_info == 1 {
                self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                self.entity_update_count += 1;
            }
        }

        buf.gbit_end();
    }

    /// `getPlayerPosExtended` from client-ts.
    fn get_player_pos_extended(&mut self, buf: &mut Packet) {
        for i in 0..self.entity_update_count as usize {
            let index = self.entity_update_ids[i] as usize;
            if self.players[index].is_none() {
                continue;
            }

            let mut mask = buf.g1();
            if mask & player_update::BIG_UPDATE != 0 {
                mask = mask.wrapping_add(buf.g1() << 8);
            }

            self.get_player_pos_decode_extended(index, mask, buf);
        }
    }

    /// `getPlayerPosDecodeExtended` from client-ts.
    fn get_player_pos_decode_extended(&mut self, index: usize, mask: i32, buf: &mut Packet) {
        if mask & player_update::APPEARANCE != 0 {
            let length = buf.g1() as usize;

            let mut data = vec![0u8; length];
            buf.gdata(length, 0, &mut data);

            self.player_appearance_buffer[index] = Some(Packet::new(data));
            if let Some(player) = self.players[index].as_mut() {
                let mut appearance = self.player_appearance_buffer[index].take().unwrap();
                appearance.pos = 0;
                player.set_appearance(&mut appearance, &self.cache);
                self.player_appearance_buffer[index] = Some(appearance);
            }
        }

        if mask & player_update::ANIM != 0 {
            let mut seq_id = buf.g2();
            if seq_id == 65535 {
                seq_id = -1;
            }

            if let Some(player) = self.players[index].as_mut() {
                if seq_id == player.primary_anim {
                    player.primary_anim_loop = 0;
                }

                let delay = buf.g1();
                if player.primary_anim == seq_id && seq_id != -1 {
                    if (seq_id as usize) < self.cache.seqs.len() {
                        let restart_mode = self.cache.seq(seq_id as usize).duplicatebehaviour;
                        if restart_mode == RESTART_RESET {
                            player.primary_anim_frame = 0;
                            player.primary_anim_cycle = 0;
                            player.primary_anim_delay = delay;
                            player.primary_anim_loop = 0;
                        } else if restart_mode == RESTART_RESETLOOP {
                            player.primary_anim_loop = 0;
                        }
                    }
                } else if seq_id == -1
                    || player.primary_anim == -1
                    // unloaded seq table: TS would resolve the priority via
                    // SeqType.list; treat as "priority passes" to keep state
                    || (seq_id as usize) >= self.cache.seqs.len()
                    || (player.primary_anim as usize) >= self.cache.seqs.len()
                    || self.cache.seq(seq_id as usize).priority
                        >= self.cache.seq(player.primary_anim as usize).priority
                {
                    player.primary_anim = seq_id;
                    player.primary_anim_frame = 0;
                    player.primary_anim_cycle = 0;
                    player.primary_anim_delay = delay;
                    player.primary_anim_loop = 0;
                    player.preanim_route_length = player.route_length;
                }
            }
        }

        if mask & player_update::FACEENTITY != 0 {
            let mut face_entity = buf.g2();
            if face_entity == 65535 {
                face_entity = -1;
            }
            if let Some(player) = self.players[index].as_mut() {
                player.face_entity = face_entity;
            }
        }

        if mask & player_update::SAY != 0 {
            let message = buf.gjstr();
            if let Some(player) = self.players[index].as_mut() {
                player.chat_message = Some(message);
                player.chat_colour = 0;
                player.chat_effect = 0;
                player.chat_timer = 150;
                // TS also addChat(2, message, name) when the player has a
                // name; the chat display lands with the chat task.
            }
        }

        if mask & player_update::HITMARK != 0 {
            let damage = buf.g1();
            let damage_type = buf.g1();
            if let Some(player) = self.players[index].as_mut() {
                player.add_hitmark(self.loop_cycle, damage_type, damage);
                player.combat_cycle = self.loop_cycle + 400;
                player.health = buf.g1();
                player.total_health = buf.g1();
            }
        }

        if mask & player_update::FACESQUARE != 0 {
            let x = buf.g2();
            let z = buf.g2();
            if let Some(player) = self.players[index].as_mut() {
                player.face_square_x = x;
                player.face_square_z = z;
            }
        }

        if mask & player_update::CHAT != 0 {
            let _colour_effect = buf.g2();
            let _chat_type = buf.g1();
            let length = buf.g1();
            let start = buf.pos;

            // TS unpacks/filters the chat via WordPack/WordFilter and
            // addChat; the chat task owns that. The payload is still skipped
            // so the frame stays aligned.
            buf.pos = start + length as usize;
        }

        if mask & player_update::SPOTANIM != 0 {
            let spotanim_id = buf.g2();
            let height_delay = buf.g4();
            if let Some(player) = self.players[index].as_mut() {
                player.spotanim_id = spotanim_id;
                player.spotanim_height = height_delay >> 16;
                player.spotanim_last_cycle = self.loop_cycle + (height_delay & 0xffff);
                player.spotanim_frame = 0;
                player.spotanim_cycle = 0;

                if player.spotanim_last_cycle > self.loop_cycle {
                    player.spotanim_frame = -1;
                }

                if spotanim_id == 65535 {
                    player.spotanim_id = -1;
                }
            }
        }

        if mask & player_update::EXACTMOVE != 0 {
            let exact_start_x = buf.g1();
            let exact_start_z = buf.g1();
            let exact_end_x = buf.g1();
            let exact_end_z = buf.g1();
            let exact_move_end = buf.g2() + self.loop_cycle;
            let exact_move_start = buf.g2() + self.loop_cycle;
            let exact_move_facing = buf.g1();
            if let Some(player) = self.players[index].as_mut() {
                player.exact_start_x = exact_start_x;
                player.exact_start_z = exact_start_z;
                player.exact_end_x = exact_end_x;
                player.exact_end_z = exact_end_z;
                player.exact_move_end = exact_move_end;
                player.exact_move_start = exact_move_start;
                player.exact_move_facing = exact_move_facing;

                player.abort_route();
            }
        }

        if mask & player_update::HITMARK2 != 0 {
            let damage = buf.g1();
            let damage_type = buf.g1();
            if let Some(player) = self.players[index].as_mut() {
                player.add_hitmark(self.loop_cycle, damage_type, damage);
                player.combat_cycle = self.loop_cycle + 400;
                player.health = buf.g1();
                player.total_health = buf.g1();
            }
        }
    }

    /// `getNpcPos(buf, size)` from client-ts: unpack the `NPC_INFO` frame
    /// into the `npc` table.
    fn get_npc_pos(&mut self, buf: &mut Packet, size: i32) {
        self.entity_removal_count = 0;
        self.entity_update_count = 0;

        self.get_npc_pos_old_vis(buf);
        self.get_npc_pos_new_vis(buf, size);
        self.get_npc_pos_extended(buf);

        for i in 0..self.entity_removal_count as usize {
            let index = self.entity_removal_ids[i] as usize;
            if index < self.npc.len() {
                if let Some(npc) = &self.npc[index] {
                    if npc.cycle != self.loop_cycle {
                        self.npc[index] = None;
                    }
                }
            }
        }

        if buf.pos != size as usize {
            // TS throws here; the tcpIn catch reports T2 and logs out
            eprintln!(
                "T2 - {} size mismatch in getnpcpos - pos:{} psize:{}",
                self.login_user, buf.pos, size
            );
            self.logout();
            return;
        }

        for i in 0..self.npc_count as usize {
            let index = self.npc_ids[i] as usize;
            if self.npc.get(index).is_none_or(|n| n.is_none()) {
                eprintln!(
                    "T2 - {} null entry in npc list - pos:{} size:{}",
                    self.login_user, i, self.npc_count
                );
                self.logout();
                return;
            }
        }
    }

    /// `getNpcPosOldVis` from client-ts.
    fn get_npc_pos_old_vis(&mut self, buf: &mut Packet) {
        buf.gbit_start();

        let count = buf.gbit(8);
        if count < self.npc_count {
            for i in count as usize..self.npc_count as usize {
                self.entity_removal_ids[self.entity_removal_count as usize] = self.npc_ids[i];
                self.entity_removal_count += 1;
            }
        }

        if count > self.npc_count {
            eprintln!("T2 - {} Too many npcs", self.login_user);
            self.logout();
            return;
        }

        self.npc_count = 0;
        for i in 0..count as usize {
            let index = self.npc_ids[i] as usize;

            let info = buf.gbit(1);
            if info == 0 {
                self.npc_ids[self.npc_count as usize] = index as i32;
                self.npc_count += 1;
                if let Some(npc) = self.npc[index].as_mut() {
                    npc.cycle = self.loop_cycle;
                }
            } else {
                let op = buf.gbit(2);

                if op == 0 {
                    self.npc_ids[self.npc_count as usize] = index as i32;
                    self.npc_count += 1;
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.cycle = self.loop_cycle;
                    }
                    self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                    self.entity_update_count += 1;
                } else if op == 1 {
                    self.npc_ids[self.npc_count as usize] = index as i32;
                    self.npc_count += 1;
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.cycle = self.loop_cycle;
                    }

                    let walk_dir = buf.gbit(3);
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.move_code(&self.cache, false, walk_dir);
                    }

                    let extended_info = buf.gbit(1);
                    if extended_info == 1 {
                        self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                        self.entity_update_count += 1;
                    }
                } else if op == 2 {
                    self.npc_ids[self.npc_count as usize] = index as i32;
                    self.npc_count += 1;
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.cycle = self.loop_cycle;
                    }

                    let walk_dir = buf.gbit(3);
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.move_code(&self.cache, true, walk_dir);
                    }

                    let run_dir = buf.gbit(3);
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.move_code(&self.cache, true, run_dir);
                    }

                    let extended_info = buf.gbit(1);
                    if extended_info == 1 {
                        self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                        self.entity_update_count += 1;
                    }
                } else {
                    self.entity_removal_ids[self.entity_removal_count as usize] = index as i32;
                    self.entity_removal_count += 1;
                }
            }
        }
    }

    /// `getNpcPosNewVis` from client-ts.
    fn get_npc_pos_new_vis(&mut self, buf: &mut Packet, size: i32) {
        while buf.bit_pos + 21 < (size as usize) * 8 {
            let index = buf.gbit(14);
            if index == 16383 {
                break;
            }
            let index = index as usize;

            if self.npc[index].is_none() {
                self.npc[index] = Some(ClientNpc::default());
            }

            let npc = self.npc[index].as_mut().unwrap();
            self.npc_ids[self.npc_count as usize] = index as i32;
            self.npc_count += 1;
            npc.cycle = self.loop_cycle;

            let type_id = buf.gbit(11);
            if type_id >= 0 && (type_id as usize) < self.cache.npcs.len() {
                let r#type = self.cache.npc(type_id as usize);
                npc.r#type = Some(type_id as usize);
                npc.size = r#type.size;
                npc.turnspeed = r#type.turnspeed;
                npc.walkanim = r#type.walkanim;
                npc.walkanim_b = r#type.walkanim_b;
                // [sic] TS swaps left/right here
                npc.walkanim_l = r#type.walkanim_r;
                npc.walkanim_r = r#type.walkanim_l;
                npc.readyanim = r#type.readyanim;
            } else {
                npc.r#type = None;
            }

            let mut dx = buf.gbit(5);
            if dx > 15 {
                dx -= 32;
            }
            let mut dz = buf.gbit(5);
            if dz > 15 {
                dz -= 32;
            }
            let jump = buf.gbit(1);
            if let Some(local) = &self.local_player {
                npc.teleport(
                    &self.cache,
                    jump == 1,
                    local.route_x[0] + dx,
                    local.route_z[0] + dz,
                );
            }

            let extended_info = buf.gbit(1);
            if extended_info == 1 {
                self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                self.entity_update_count += 1;
            }
        }

        buf.gbit_end();
    }

    /// `getNpcPosExtended` from client-ts.
    fn get_npc_pos_extended(&mut self, buf: &mut Packet) {
        for i in 0..self.entity_update_count as usize {
            let id = self.entity_update_ids[i] as usize;
            if self.npc[id].is_none() {
                continue;
            }

            let mask = buf.g1();

            if mask & npc_update::HITMARK2 != 0 {
                let damage = buf.g1();
                let damage_type = buf.g1();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.add_hitmark(self.loop_cycle, damage_type, damage);
                    npc.combat_cycle = self.loop_cycle + 400;
                    npc.health = buf.g1();
                    npc.total_health = buf.g1();
                }
            }

            if mask & npc_update::ANIM != 0 {
                let mut anim = buf.g2();
                if anim == 65535 {
                    anim = -1;
                }

                if let Some(npc) = self.npc[id].as_mut() {
                    if anim == npc.primary_anim {
                        npc.primary_anim_loop = 0;
                    }

                    let delay = buf.g1();
                    if npc.primary_anim == anim && anim != -1 {
                        if (anim as usize) < self.cache.seqs.len() {
                            let restart_mode = self.cache.seq(anim as usize).duplicatebehaviour;
                            if restart_mode == RESTART_RESET {
                                npc.primary_anim_frame = 0;
                                npc.primary_anim_cycle = 0;
                                npc.primary_anim_delay = delay;
                                npc.primary_anim_loop = 0;
                            } else if restart_mode == RESTART_RESETLOOP {
                                npc.primary_anim_loop = 0;
                            }
                        }
                    } else if anim == -1
                        || npc.primary_anim == -1
                        // unloaded seq table: treat as "priority passes"
                        || (anim as usize) >= self.cache.seqs.len()
                        || (npc.primary_anim as usize) >= self.cache.seqs.len()
                        || self.cache.seq(anim as usize).priority
                            >= self.cache.seq(npc.primary_anim as usize).priority
                    {
                        npc.primary_anim = anim;
                        npc.primary_anim_frame = 0;
                        npc.primary_anim_cycle = 0;
                        npc.primary_anim_delay = delay;
                        npc.primary_anim_loop = 0;
                        npc.preanim_route_length = npc.route_length;
                    }
                }
            }

            if mask & npc_update::FACEENTITY != 0 {
                let mut face_entity = buf.g2();
                if face_entity == 65535 {
                    face_entity = -1;
                }
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.face_entity = face_entity;
                }
            }

            if mask & npc_update::SAY != 0 {
                let message = buf.gjstr();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.chat_message = Some(message);
                    npc.chat_timer = 100;
                }
            }

            if mask & npc_update::HITMARK != 0 {
                let damage = buf.g1();
                let damage_type = buf.g1();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.add_hitmark(self.loop_cycle, damage_type, damage);
                    npc.combat_cycle = self.loop_cycle + 400;
                    npc.health = buf.g1();
                    npc.total_health = buf.g1();
                }
            }

            if mask & npc_update::CHANGETYPE != 0 {
                let type_id = buf.g2();
                if let Some(npc) = self.npc[id].as_mut() {
                    if type_id >= 0 && (type_id as usize) < self.cache.npcs.len() {
                        let r#type = self.cache.npc(type_id as usize);
                        npc.r#type = Some(type_id as usize);
                        npc.size = r#type.size;
                        npc.turnspeed = r#type.turnspeed;
                        npc.walkanim = r#type.walkanim;
                        npc.walkanim_b = r#type.walkanim_b;
                        // [sic] TS swaps left/right here
                        npc.walkanim_l = r#type.walkanim_r;
                        npc.walkanim_r = r#type.walkanim_l;
                        npc.readyanim = r#type.readyanim;
                    } else {
                        npc.r#type = None;
                    }
                }
            }

            if mask & npc_update::SPOTANIM != 0 {
                let spotanim_id = buf.g2();
                let info = buf.g4();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.spotanim_id = spotanim_id;
                    npc.spotanim_height = info >> 16;
                    npc.spotanim_last_cycle = self.loop_cycle + (info & 0xffff);
                    npc.spotanim_frame = 0;
                    npc.spotanim_cycle = 0;

                    if npc.spotanim_last_cycle > self.loop_cycle {
                        npc.spotanim_frame = -1;
                    }

                    if spotanim_id == 65535 {
                        npc.spotanim_id = -1;
                    }
                }
            }

            if mask & npc_update::FACESQUARE != 0 {
                let x = buf.g2();
                let z = buf.g2();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.face_square_x = x;
                    npc.face_square_z = z;
                }
            }
        }
    }

    /// `zonePacket(buf, opcode)` from client-ts: reads the 8-tile zone
    /// position byte and dispatches by opcode. The zone scene state
    /// (groundObj/locChanges/spotanims/projectiles, world edits) lands with
    /// the zone task; each opcode is an explicit no-op for now, keeping the
    /// TS read structure.
    fn zone_packet(&mut self, buf: &mut Packet, opcode: i32) {
        let pos = buf.g1();
        let _x = self.zone_update_x + ((pos >> 4) & 0x7);
        let _z = self.zone_update_z + (pos & 0x7);

        match opcode {
            ServerProt::LOC_ADD_CHANGE => {
                // g1 info (shape = info >> 2, rotate = info & 3) + g2 id →
                // locChangeCreate(level, x, z, layer, id, shape, rotate, 0, -1)
            }
            ServerProt::LOC_DEL => {
                // g1 info → locChangeCreate(level, x, z, layer, -1, shape, rotate, 0, -1)
            }
            ServerProt::LOC_ANIM => {
                // g1 info + g2 seq → ClientLocAnim on the wall/decor/scene/gd
            }
            ServerProt::OBJ_ADD => {
                // g2 type + g2 count → groundObj push + showObject
            }
            ServerProt::OBJ_DEL => {
                // g2 type → groundObj unlink + showObject
            }
            ServerProt::MAP_PROJANIM => {
                // g1b/g1b/g2b/g2/g1*2/g2*2/g1/g1 → projectiles push
            }
            ServerProt::MAP_ANIM => {
                // g2 spotanim + g1 height + g2 time → spotanims push
            }
            ServerProt::OBJ_REVEAL => {
                // g2 id + g2 count + g2 pid → groundObj push unless pid === selfSlot
            }
            ServerProt::P_LOCMERGE => {
                // g1 info + g2 id + g2 t1 + g2 t2 + g2 pid + g1b*4 → loc merge
            }
            ServerProt::OBJ_COUNT => {
                // g2 type + g2 ocount + g2 count → groundObj count update
            }
            _ => {}
        }
    }

    /// `logout()` from client-ts: close the stream and return to the login
    /// screen. The midi stops, `loginscreen` and the model-cache clears land
    /// with the music/login-screen tasks.
    pub fn logout(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            stream.close();
        }
        self.ingame = false;
        self.login_user.clear();
        self.login_pass.clear();
        self.world.reset_map();
        for collision in &mut self.collision {
            collision.reset();
        }
    }
}

fn io_error() -> LoginError {
    LoginError {
        code: -1,
        mes1: String::new(),
        mes2: "Error connecting to server.".into(),
    }
}

/// `Client.levelExperience` from client-ts: cumulative XP thresholds for
/// levels 1..99, computed in the TS static initializer.
fn level_experience() -> &'static [i32; 99] {
    static TABLE: OnceLock<[i32; 99]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0i32; 99];
        let mut acc: i32 = 0;
        for (i, entry) in table.iter_mut().enumerate() {
            let level = (i + 1) as f64;
            // TS `(level + Math.pow(2.0, level / 7.0) * 300.0) | 0`
            let delta = (level + 2.0f64.powf(level / 7.0) * 300.0) as i32;
            acc += delta;
            // TS `(acc / 4) | 0`
            *entry = acc / 4;
        }
        table
    })
}

/// Grow-on-write store matching the TS plain-array assignment `var[id] = v`
/// (plain arrays extend on out-of-range writes).
fn grow_write(table: &mut Vec<i32>, id: i32, value: i32) {
    let index = id as usize;
    if index >= table.len() {
        table.resize(index + 1, 0);
    }
    table[index] = value;
}

/// Stand-in for Java `(int)(Math.random() * 99999999)`: a non-negative
/// value below 100_000_000 for the login Isaac seed.
fn login_random() -> i32 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    let r = x.wrapping_mul(0x2545_f491_4f6c_dd1d);
    ((r >> 32) % 99_999_999) as i32
}
