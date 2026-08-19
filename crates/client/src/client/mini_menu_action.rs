// Port of `~/experiments/Server/webclient/src/client/MiniMenuAction.ts`: the
// `menuAction` values `Client::doAction` dispatches on.
pub struct MiniMenuAction;

impl MiniMenuAction {
    pub const _PRIORITY: i32 = 2000;

    // cast spell on
    pub const TGT_LOC: i32 = 899;
    pub const OP_LOC1: i32 = 625;
    pub const OP_LOC2: i32 = 721;
    pub const OP_LOC3: i32 = 743;
    pub const OP_LOC4: i32 = 357;
    pub const OP_LOC5: i32 = 1071;
    pub const USEHELD_ONLOC: i32 = 810; // use item on

    pub const TGT_NPC: i32 = 240; // cast spell on
    pub const OP_NPC1: i32 = 242;
    pub const OP_NPC2: i32 = 209;
    pub const OP_NPC3: i32 = 309;
    pub const OP_NPC4: i32 = 852;
    pub const OP_NPC5: i32 = 793;
    pub const USEHELD_ONNPC: i32 = 829; // use item on

    pub const TGT_OBJ: i32 = 370; // cast spell on
    pub const OP_OBJ1: i32 = 139;
    pub const OP_OBJ2: i32 = 778;
    pub const OP_OBJ3: i32 = 617;
    pub const OP_OBJ4: i32 = 224;
    pub const OP_OBJ5: i32 = 662;
    pub const USEHELD_ONOBJ: i32 = 111; // use item on

    pub const TGT_PLAYER: i32 = 131; // cast spell on
    pub const OP_PLAYER1: i32 = 639;
    pub const ACCEPT_DUELREQ: i32 = 957; // opplayer1
    pub const OP_PLAYER2: i32 = 499;
    pub const OP_PLAYER3: i32 = 27;
    pub const OP_PLAYER4: i32 = 387;
    pub const ACCEPT_TRADEREQ: i32 = 507; // opplayer4
    pub const OP_PLAYER5: i32 = 185;
    pub const USEHELD_ONPLAYER: i32 = 275; // use item on

    pub const TGT_HELD: i32 = 563; // cast spell on
    pub const OP_HELD1: i32 = 694;
    pub const OP_HELD2: i32 = 962;
    pub const OP_HELD3: i32 = 795;
    pub const OP_HELD4: i32 = 681;
    pub const OP_HELD5: i32 = 100;
    pub const USEHELD_ONHELD: i32 = 398; // use item on

    pub const INV_BUTTON1: i32 = 582;
    pub const INV_BUTTON2: i32 = 113;
    pub const INV_BUTTON3: i32 = 555;
    pub const INV_BUTTON4: i32 = 331;
    pub const INV_BUTTON5: i32 = 354;

    pub const WALK: i32 = 718;

    pub const IF_BUTTON: i32 = 231;
    pub const TGT_BUTTON: i32 = 274; // select target for spell
    pub const CLOSE_BUTTON: i32 = 737;
    pub const TOGGLE_BUTTON: i32 = 435;
    pub const SELECT_BUTTON: i32 = 225;
    pub const PAUSE_BUTTON: i32 = 997;

    pub const USEHELD_START: i32 = 102; // select target for item

    // examine
    pub const OP_LOC6: i32 = 1381;
    pub const OP_NPC6: i32 = 1714;
    pub const OP_OBJ6: i32 = 1152;
    pub const OP_HELD6: i32 = 1328;

    pub const CANCEL: i32 = 1106;

    pub const ABUSE_REPORT: i32 = 524;

    pub const FRIENDLIST_ADD: i32 = 605;
    pub const IGNORELIST_ADD: i32 = 47;
    pub const FRIENDLIST_DEL: i32 = 513;
    pub const IGNORELIST_DEL: i32 = 884;

    pub const MESSAGE_PRIVATE: i32 = 902;
}
