//! Client machine: 1:1 skeleton of `webclient/src/client/Client.ts`.
//!
//! Java-public fields used by login and later `RawClient`-style reads start
//! here (`ingame`, `loop_cycle` as an instance field, `loginUser`, `loginPass`,
//! `out`, `in`, menu arrays). `login` runs the 274 handshake (wrapper opcode
//! 16 cold / 18 reconnect) over Java-style TCP `ClientStream`.
//! There is no snapshot/query API.

use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use num_bigint::BigUint;

use crate::client::config::ClientConfig;
use crate::client::game_shell::GameShell;
use crate::client::login_error::LoginError;
use crate::client::mini_menu_action::MiniMenuAction;
use crate::client::skill::Skill;
use crate::config::Cache;
use crate::dash3d::world::LevelHeightmaps;
use crate::dash3d::{BuildArea, CollisionFlag, CollisionMap, DirectionFlag, LocShape, World};
pub use crate::dash3d::{ClientNpc, ClientPlayer};
use crate::io::{ClientProt, ClientStream, Isaac, Packet};
use crate::login_rsa::{LOGIN_RSAE, LOGIN_RSAN};
use crate::util::JString;

const MAX_PLAYER_COUNT: usize = 2048;
const MAX_NPC_COUNT: usize = 16384;
const MENU_CAPACITY: usize = 500;
const CLIENT_VERSION: i32 = 274;
const LOGIN_UID: i32 = 1337;

/// Side of the build area, `BuildArea.SIZE` (13 << 3) in client-ts.
const BUILD_AREA_SIZE: i32 = 104;
const BUILD_AREA_TILES: usize = (BUILD_AREA_SIZE * BUILD_AREA_SIZE) as usize;
const ROUTE_BUFFER: usize = 4000;

/// JAG archives whose CRC values go out in the login wrapper; slot 0 of the
/// 9-slot `getJagChecksums` layout has no pack file and stays 0.
const JAG_FILES: [&str; 8] = [
    "title", "config", "interface", "media", "versionlist", "textures", "wordenc", "sounds",
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

    /// Scene height map, `groundh[level][x][z]` sized `[4][105][105]`; owned
    /// here and copied into `world` (Task 16's `ClientBuild` owns the write
    /// side and will reconcile the sharing).
    pub groundh: LevelHeightmaps,
    pub world: World,
    /// One collision grid per level, `CollisionMap` for the 4 build levels.
    pub collision: [CollisionMap; 4],
    pub minusedlevel: i32,

    pub stat_base_level: Vec<i32>,
    pub stat_effective_level: Vec<i32>,
    pub stat_xp: Vec<i32>,
    pub var: Vec<i32>,

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
        let groundh: LevelHeightmaps = vec![
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

            groundh: groundh.clone(),
            world: World::new(
                groundh,
                BuildArea::SIZE,
                BuildArea::LEVELS,
                BuildArea::SIZE,
            ),
            collision: [
                CollisionMap::new(),
                CollisionMap::new(),
                CollisionMap::new(),
                CollisionMap::new(),
            ],
            minusedlevel: 0,

            stat_base_level: vec![0; Skill::count],
            stat_effective_level: vec![0; Skill::count],
            stat_xp: vec![0; Skill::count],
            var: Vec::new(),

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

            stream.write(loginout.data(), loginout.pos).map_err(|_| io_error())?;
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
            7 => ("This world is full.".into(), "Please use a different world.".into()),
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
        Err(LoginError { code: response, mes1, mes2 })
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
            let npc_route =
                self.npc.get(a as usize).and_then(|n| n.as_ref()).map(|n| (n.route_x[0], n.route_z[0]));
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
            if let Some((px, pz)) =
                self.local_player.as_ref().map(|p| (p.route_x[0], p.route_z[0]))
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
}

fn io_error() -> LoginError {
    LoginError { code: -1, mes1: String::new(), mes2: "Error connecting to server.".into() }
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
