// Port of `~/experiments/Server/engine/src/network/game/client/ClientGameProt.ts`:
// opcode id + fixed length for every client→server game packet.
// `-1` marks a variable-length packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientProt {
    pub id: i32,
    pub length: i32,
}

impl ClientProt {
    pub const NO_TIMEOUT: ClientProt = ClientProt { id: 120, length: 0 };

    pub const IDLE_TIMER: ClientProt = ClientProt { id: 209, length: 0 };
    pub const EVENT_MOUSE_CLICK: ClientProt = ClientProt { id: 20, length: 4 };
    pub const EVENT_MOUSE_MOVE: ClientProt = ClientProt {
        id: 222,
        length: -1,
    };
    pub const EVENT_APPLET_FOCUS: ClientProt = ClientProt { id: 73, length: 1 };
    pub const EVENT_CAMERA_POSITION: ClientProt = ClientProt { id: 53, length: 4 };

    pub const ANTICHEAT_OPLOGIC1: ClientProt = ClientProt { id: 219, length: 4 };
    pub const ANTICHEAT_OPLOGIC2: ClientProt = ClientProt { id: 201, length: 2 };
    pub const ANTICHEAT_OPLOGIC3: ClientProt = ClientProt { id: 41, length: 4 };
    pub const ANTICHEAT_OPLOGIC4: ClientProt = ClientProt { id: 80, length: 1 };
    pub const ANTICHEAT_OPLOGIC5: ClientProt = ClientProt { id: 235, length: 1 };
    pub const ANTICHEAT_OPLOGIC6: ClientProt = ClientProt { id: 250, length: 2 };
    pub const ANTICHEAT_OPLOGIC7: ClientProt = ClientProt { id: 25, length: 4 };
    pub const ANTICHEAT_OPLOGIC8: ClientProt = ClientProt { id: 0, length: 1 };
    pub const ANTICHEAT_OPLOGIC9: ClientProt = ClientProt { id: 24, length: 3 };

    pub const ANTICHEAT_CYCLELOGIC1: ClientProt = ClientProt { id: 12, length: -1 };
    pub const ANTICHEAT_CYCLELOGIC2: ClientProt = ClientProt {
        id: 149,
        length: -1,
    };
    pub const ANTICHEAT_CYCLELOGIC3: ClientProt = ClientProt { id: 52, length: 1 };
    pub const ANTICHEAT_CYCLELOGIC4: ClientProt = ClientProt { id: 230, length: 1 };
    pub const ANTICHEAT_CYCLELOGIC5: ClientProt = ClientProt { id: 100, length: 0 };
    pub const ANTICHEAT_CYCLELOGIC6: ClientProt = ClientProt { id: 188, length: 1 };
    pub const ANTICHEAT_CYCLELOGIC7: ClientProt = ClientProt { id: 89, length: 0 };

    pub const OPOBJ1: ClientProt = ClientProt { id: 247, length: 6 };
    pub const OPOBJ2: ClientProt = ClientProt { id: 169, length: 6 };
    pub const OPOBJ3: ClientProt = ClientProt { id: 108, length: 6 };
    pub const OPOBJ4: ClientProt = ClientProt { id: 62, length: 6 };
    pub const OPOBJ5: ClientProt = ClientProt { id: 117, length: 6 };
    pub const OPOBJT: ClientProt = ClientProt { id: 91, length: 8 };
    pub const OPOBJU: ClientProt = ClientProt { id: 39, length: 12 };

    pub const OPNPC1: ClientProt = ClientProt { id: 236, length: 2 };
    pub const OPNPC2: ClientProt = ClientProt { id: 233, length: 2 };
    pub const OPNPC3: ClientProt = ClientProt { id: 223, length: 2 };
    pub const OPNPC4: ClientProt = ClientProt { id: 147, length: 2 };
    pub const OPNPC5: ClientProt = ClientProt { id: 189, length: 2 };
    pub const OPNPCT: ClientProt = ClientProt { id: 181, length: 4 };
    pub const OPNPCU: ClientProt = ClientProt { id: 150, length: 8 };

    pub const OPLOC1: ClientProt = ClientProt { id: 215, length: 6 };
    pub const OPLOC2: ClientProt = ClientProt { id: 103, length: 6 };
    pub const OPLOC3: ClientProt = ClientProt { id: 187, length: 6 };
    pub const OPLOC4: ClientProt = ClientProt { id: 157, length: 6 };
    pub const OPLOC5: ClientProt = ClientProt { id: 127, length: 6 };
    pub const OPLOCT: ClientProt = ClientProt { id: 213, length: 8 };
    pub const OPLOCU: ClientProt = ClientProt { id: 60, length: 12 };

    pub const OPPLAYER1: ClientProt = ClientProt { id: 109, length: 2 };
    pub const OPPLAYER2: ClientProt = ClientProt { id: 166, length: 2 };
    pub const OPPLAYER3: ClientProt = ClientProt { id: 196, length: 2 };
    pub const OPPLAYER4: ClientProt = ClientProt { id: 98, length: 2 };
    pub const OPPLAYER5: ClientProt = ClientProt { id: 174, length: 2 };
    pub const OPPLAYERT: ClientProt = ClientProt { id: 240, length: 4 };
    pub const OPPLAYERU: ClientProt = ClientProt { id: 36, length: 8 };

    pub const OPHELD1: ClientProt = ClientProt { id: 185, length: 6 };
    pub const OPHELD2: ClientProt = ClientProt { id: 2, length: 6 };
    pub const OPHELD3: ClientProt = ClientProt { id: 123, length: 6 };
    pub const OPHELD4: ClientProt = ClientProt { id: 216, length: 6 };
    pub const OPHELD5: ClientProt = ClientProt { id: 42, length: 6 };
    pub const OPHELDT: ClientProt = ClientProt { id: 135, length: 8 };
    pub const OPHELDU: ClientProt = ClientProt {
        id: 136,
        length: 12,
    };

    pub const INV_BUTTON1: ClientProt = ClientProt { id: 74, length: 6 };
    pub const INV_BUTTON2: ClientProt = ClientProt { id: 82, length: 6 };
    pub const INV_BUTTON3: ClientProt = ClientProt { id: 239, length: 6 };
    pub const INV_BUTTON4: ClientProt = ClientProt { id: 179, length: 6 };
    pub const INV_BUTTON5: ClientProt = ClientProt { id: 46, length: 6 };

    pub const IF_BUTTON: ClientProt = ClientProt { id: 9, length: 2 };
    pub const RESUME_PAUSEBUTTON: ClientProt = ClientProt { id: 72, length: 2 };
    pub const CLOSE_MODAL: ClientProt = ClientProt { id: 51, length: 0 };
    pub const RESUME_P_COUNTDIALOG: ClientProt = ClientProt { id: 102, length: 4 };
    pub const TUT_CLICKSIDE: ClientProt = ClientProt { id: 94, length: 1 };

    pub const MAP_BUILD_COMPLETE: ClientProt = ClientProt { id: 214, length: 0 };
    pub const MOVE_OPCLICK: ClientProt = ClientProt {
        id: 138,
        length: -1,
    };
    pub const REPORT_ABUSE: ClientProt = ClientProt {
        id: 137,
        length: 10,
    }; // todo: rename to SEND_SNAPSHOT
    pub const MOVE_MINIMAPCLICK: ClientProt = ClientProt { id: 86, length: -1 };
    pub const INV_BUTTOND: ClientProt = ClientProt { id: 93, length: 7 };
    pub const IGNORELIST_DEL: ClientProt = ClientProt { id: 101, length: 8 };
    pub const IGNORELIST_ADD: ClientProt = ClientProt { id: 255, length: 8 };
    pub const IDK_SAVEDESIGN: ClientProt = ClientProt {
        id: 125,
        length: 13,
    };
    pub const CHAT_SETMODE: ClientProt = ClientProt { id: 154, length: 3 };
    pub const MESSAGE_PRIVATE: ClientProt = ClientProt {
        id: 139,
        length: -1,
    };
    pub const FRIENDLIST_DEL: ClientProt = ClientProt { id: 106, length: 8 };
    pub const FRIENDLIST_ADD: ClientProt = ClientProt { id: 13, length: 8 };
    pub const CLIENT_CHEAT: ClientProt = ClientProt {
        id: 224,
        length: -1,
    };
    pub const MESSAGE_PUBLIC: ClientProt = ClientProt {
        id: 253,
        length: -1,
    };
    pub const MOVE_GAMECLICK: ClientProt = ClientProt {
        id: 207,
        length: -1,
    };
}
