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
use crate::client::skill::Skill;
use crate::io::{ClientStream, Isaac, Packet};
use crate::login_rsa::{LOGIN_RSAE, LOGIN_RSAN};
use crate::util::JString;

const MAX_PLAYER_COUNT: usize = 2048;
const MAX_NPC_COUNT: usize = 16384;
const MENU_CAPACITY: usize = 500;
const CLIENT_VERSION: i32 = 274;
const LOGIN_UID: i32 = 1337;

/// JAG archives whose CRC values go out in the login wrapper; slot 0 of the
/// 9-slot `getJagChecksums` layout has no pack file and stays 0.
const JAG_FILES: [&str; 8] = [
    "title", "config", "interface", "media", "versionlist", "textures", "wordenc", "sounds",
];

/// Placeholder for the dash3d entity; replaced when `ClientPlayer` is ported.
#[derive(Clone)]
pub struct ClientPlayer;

/// Placeholder for the dash3d entity; replaced when `ClientNpc` is ported.
#[derive(Clone)]
pub struct ClientNpc;

pub struct Client {
    pub shell: GameShell,
    pub config: ClientConfig,

    pub ingame: bool,
    pub scene_state: i32,
    pub local_player: Option<ClientPlayer>,
    pub players: Vec<Option<ClientPlayer>>,
    pub npc: Vec<Option<ClientNpc>>,

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
        Client {
            shell: GameShell::new(),
            config,

            ingame: false,
            scene_state: 0,
            local_player: None,
            players: vec![None; MAX_PLAYER_COUNT],
            npc: vec![None; MAX_NPC_COUNT],

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
