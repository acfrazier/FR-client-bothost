//! Client machine: 1:1 skeleton of `webclient/src/client/Client.ts`.
//!
//! Java-public fields used by login and later `RawClient`-style reads start
//! here (`ingame`, `loop_cycle` as an instance field, `loginUser`, `loginPass`,
//! `out`, `in`, menu arrays). `login` is a stub until the handshake task lands.
//! There is no snapshot/query API.

use crate::client::config::ClientConfig;
use crate::client::game_shell::GameShell;
use crate::client::login_error::LoginError;
use crate::client::skill::Skill;
use crate::io::Packet;

const MAX_PLAYER_COUNT: usize = 2048;
const MAX_NPC_COUNT: usize = 16384;
const MENU_CAPACITY: usize = 500;

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
    pub ptype0: i32,

    pub login_user: String,
    pub login_pass: String,
    pub login_mes1: String,
    pub login_mes2: String,
    pub loop_cycle: i32,
}

impl Client {
    pub fn new(config: ClientConfig) -> Self {
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
            ptype0: 0,

            login_user: String::new(),
            login_pass: String::new(),
            login_mes1: String::new(),
            login_mes2: String::new(),
            loop_cycle: 0,
        }
    }

    /// Login handshake; stub until the real opcode 16/18 handshake lands.
    pub fn login(
        &mut self,
        _username: &str,
        _password: &str,
        _reconnect: bool,
    ) -> Result<(), LoginError> {
        Err(LoginError { code: -1, mes1: String::new(), mes2: String::new() })
    }
}
