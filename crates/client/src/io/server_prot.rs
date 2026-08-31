// Port of `~/experiments/Server/webclient/src/io/ServerProt.ts`: server→client
// opcode consts plus the `ServerProtSizes` table (indices 0–255, verbatim;
// the TS array's trailing 257th zero is never indexed — `ptype` is a byte).
pub struct ServerProt;

impl ServerProt {
    // interfaces
    pub const IF_OPENCHAT: i32 = 166;
    pub const IF_OPENMAIN_SIDE: i32 = 158;
    pub const IF_CLOSE: i32 = 171;
    pub const IF_SETICON: i32 = 215;
    pub const IF_SHOWICON: i32 = 241;
    pub const IF_OPENMAIN: i32 = 211;
    pub const IF_OPENSIDE: i32 = 16;
    pub const IF_OPENOVERLAY: i32 = 240;

    // updating interfaces
    pub const IF_SETCOLOUR: i32 = 183;
    pub const IF_SETHIDE: i32 = 10;
    pub const IF_SETOBJECT: i32 = 28;
    pub const IF_SETMODEL: i32 = 129;
    pub const IF_SETANIM: i32 = 134;
    pub const IF_SETPLAYERHEAD: i32 = 192;
    pub const IF_SETTEXT: i32 = 44;
    pub const IF_SETNPCHEAD: i32 = 142;
    pub const IF_SETPOSITION: i32 = 77;
    pub const IF_SETSCROLLPOS: i32 = 54;

    // tutorial area
    pub const TUT_FLASH: i32 = 90;
    pub const TUT_OPEN: i32 = 130;

    // inventory
    pub const UPDATE_INV_STOP_TRANSMIT: i32 = 227;
    pub const UPDATE_INV_FULL: i32 = 106;
    pub const UPDATE_INV_PARTIAL: i32 = 172;

    // camera control
    pub const CAM_LOOKAT: i32 = 233;
    pub const CAM_SHAKE: i32 = 64;
    pub const CAM_MOVETO: i32 = 200;
    pub const CAM_RESET: i32 = 101;

    // entity updates
    pub const NPC_INFO: i32 = 197;
    pub const PLAYER_INFO: i32 = 167;

    // social
    pub const FRIENDLIST_LOADED: i32 = 185;
    pub const MESSAGE_GAME: i32 = 161;
    pub const UPDATE_IGNORELIST: i32 = 3;
    pub const CHAT_FILTER_SETTINGS: i32 = 114;
    pub const MESSAGE_PRIVATE: i32 = 235;
    pub const UPDATE_FRIENDLIST: i32 = 247;

    // misc
    pub const UNSET_MAP_FLAG: i32 = 115;
    pub const UPDATE_RUNWEIGHT: i32 = 67;
    pub const HINT_ARROW: i32 = 156;
    pub const UPDATE_REBOOT_TIMER: i32 = 89;
    pub const UPDATE_STAT: i32 = 105;
    pub const UPDATE_RUNENERGY: i32 = 83;
    pub const RESET_ANIMS: i32 = 47;
    pub const UPDATE_PID: i32 = 133;
    pub const LAST_LOGIN_INFO: i32 = 91;
    pub const LOGOUT: i32 = 88;
    pub const P_COUNTDIALOG: i32 = 210;
    pub const SET_MULTIWAY: i32 = 207;
    pub const SET_PLAYER_OP: i32 = 17;
    pub const MINIMAP_TOGGLE: i32 = 194;

    // maps
    pub const REBUILD_NORMAL: i32 = 231;

    // vars
    pub const VARP_SMALL: i32 = 203;
    pub const VARP_LARGE: i32 = 245;
    pub const VARP_SYNC: i32 = 190;

    // audio
    pub const SYNTH_SOUND: i32 = 34;
    pub const MIDI_SONG: i32 = 23;
    pub const MIDI_JINGLE: i32 = 15;

    // zones
    pub const UPDATE_ZONE_PARTIAL_FOLLOWS: i32 = 32;
    pub const UPDATE_ZONE_FULL_FOLLOWS: i32 = 153;
    pub const UPDATE_ZONE_PARTIAL_ENCLOSED: i32 = 195;

    // zone protocol
    pub const P_LOCMERGE: i32 = 176;
    pub const LOC_ANIM: i32 = 48;
    pub const OBJ_DEL: i32 = 52;
    pub const OBJ_REVEAL: i32 = 219;
    pub const LOC_ADD_CHANGE: i32 = 138;
    pub const MAP_PROJANIM: i32 = 107;
    pub const LOC_DEL: i32 = 173;
    pub const OBJ_COUNT: i32 = 95;
    pub const MAP_ANIM: i32 = 85;
    pub const OBJ_ADD: i32 = 81;
}

pub const SERVER_PROT_SIZES: [i32; 256] = [
    0, 0, 0, -2, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 4, 2, -1, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 6, 0, 0,
    0, 2, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, -2, 0, 0, 0, 4, 0, 0, 0, 3, 0, 4, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 4, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 5, 0, 1, 0, 6, 0, 0, 0, 2, 1, 10, 0,
    0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, -2, 15, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 6, 0, 0, 0, 4, 2, 0, 0, 3, 4, 0, 0, 0, 4, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0,
    6, 0, 4, 0, 0, -1, 0, 0, 0, 0, 2, -2, 0, 0, 0, 0, -2, 2, 0, 0, 14, 0, 0, 0, 0, 0, 0, 4, 0, 1,
    0, 0, 0, 0, 0, 0, 2, 0, 1, -2, 0, -2, 0, 0, 6, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0,
    0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 4, 0, 6, 0, -1, 0, 0, 0, 0, 2, 1, 0, 0, 0, 6, 0, 9,
    0, 0, 0, 0, 0, 0, 0, 0,
];
