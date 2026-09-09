//! Revision 289 client→server packet table.
//!
//! Additive beside the public 274 [`super::ClientProt`] constants. IDs are
//! source-grounded from primary `client.java` `method465` / `method160` /
//! `method206` call sites (openrs2-nonfree 0c00ef24) and match the named
//! `CLIENT_VERSION = 289` engine table. Payload lengths for the basic
//! action cut (walk, NPC/loc/object/player/held/widget/dialog/count, map
//! build, chat mode) are counted from the write sequences after each
//! opcode; shapes match 274 for these families. Cleanup H adds R289-only
//! input/lifecycle emitters and ordered-byte production-path coverage for
//! all 82 named rows; see docs/revision-289/cleanup-h-report.md.

use super::ClientProt;

/// Named 289 outbound opcodes. Public 274 [`ClientProt`] constants stay
/// unchanged; sessions select this table only when `ClientRevision::R289`.
pub struct ClientProt289;

impl ClientProt289 {
    pub const NO_TIMEOUT: ClientProt = ClientProt { id: 181, length: 0 };
    pub const IDLE_TIMER: ClientProt = ClientProt { id: 145, length: 0 };

    pub const EVENT_MOUSE_CLICK: ClientProt = ClientProt { id: 224, length: 4 };
    pub const EVENT_MOUSE_MOVE: ClientProt = ClientProt {
        id: 229,
        length: -1,
    };
    pub const EVENT_APPLET_FOCUS: ClientProt = ClientProt { id: 149, length: 1 };
    pub const EVENT_CAMERA_POSITION: ClientProt = ClientProt { id: 193, length: 4 };

    pub const ANTICHEAT_OPLOGIC1: ClientProt = ClientProt { id: 195, length: 4 };
    pub const ANTICHEAT_OPLOGIC2: ClientProt = ClientProt { id: 81, length: 2 };
    pub const ANTICHEAT_OPLOGIC3: ClientProt = ClientProt { id: 122, length: 4 };
    pub const ANTICHEAT_OPLOGIC4: ClientProt = ClientProt { id: 49, length: 1 };
    pub const ANTICHEAT_OPLOGIC5: ClientProt = ClientProt { id: 46, length: 1 };
    pub const ANTICHEAT_OPLOGIC6: ClientProt = ClientProt { id: 73, length: 2 };
    pub const ANTICHEAT_OPLOGIC7: ClientProt = ClientProt { id: 133, length: 4 };
    pub const ANTICHEAT_OPLOGIC8: ClientProt = ClientProt { id: 168, length: 1 };
    pub const ANTICHEAT_OPLOGIC9: ClientProt = ClientProt { id: 88, length: 3 };

    pub const ANTICHEAT_CYCLELOGIC1: ClientProt = ClientProt {
        id: 130,
        length: -1,
    };
    pub const ANTICHEAT_CYCLELOGIC2: ClientProt = ClientProt {
        id: 154,
        length: -1,
    };
    pub const ANTICHEAT_CYCLELOGIC3: ClientProt = ClientProt { id: 125, length: 1 };
    pub const ANTICHEAT_CYCLELOGIC4: ClientProt = ClientProt { id: 137, length: 1 };
    pub const ANTICHEAT_CYCLELOGIC5: ClientProt = ClientProt { id: 85, length: 0 };
    pub const ANTICHEAT_CYCLELOGIC6: ClientProt = ClientProt { id: 255, length: 1 };
    pub const ANTICHEAT_CYCLELOGIC7: ClientProt = ClientProt { id: 232, length: 0 };

    // Object ops — method216 OPOBJ* : p2 x, p2 z, p2 objId (+ extras for T/U)
    pub const OPOBJ1: ClientProt = ClientProt { id: 97, length: 6 };
    pub const OPOBJ2: ClientProt = ClientProt { id: 4, length: 6 };
    pub const OPOBJ3: ClientProt = ClientProt { id: 110, length: 6 };
    pub const OPOBJ4: ClientProt = ClientProt { id: 147, length: 6 };
    pub const OPOBJ5: ClientProt = ClientProt { id: 22, length: 6 };
    pub const OPOBJT: ClientProt = ClientProt { id: 241, length: 8 };
    pub const OPOBJU: ClientProt = ClientProt { id: 55, length: 12 };

    // NPC ops — p2 index (+ spell/use fields for T/U)
    pub const OPNPC1: ClientProt = ClientProt { id: 252, length: 2 };
    pub const OPNPC2: ClientProt = ClientProt { id: 21, length: 2 };
    pub const OPNPC3: ClientProt = ClientProt { id: 178, length: 2 };
    pub const OPNPC4: ClientProt = ClientProt { id: 30, length: 2 };
    pub const OPNPC5: ClientProt = ClientProt { id: 247, length: 2 };
    pub const OPNPCT: ClientProt = ClientProt { id: 108, length: 4 };
    pub const OPNPCU: ClientProt = ClientProt { id: 160, length: 8 };

    // Loc ops — method160: p2 sceneX, p2 sceneZ, p2 locId (+ T/U extras)
    pub const OPLOC1: ClientProt = ClientProt { id: 10, length: 6 };
    pub const OPLOC2: ClientProt = ClientProt { id: 45, length: 6 };
    pub const OPLOC3: ClientProt = ClientProt { id: 196, length: 6 };
    pub const OPLOC4: ClientProt = ClientProt { id: 53, length: 6 };
    pub const OPLOC5: ClientProt = ClientProt { id: 126, length: 6 };
    pub const OPLOCT: ClientProt = ClientProt { id: 218, length: 8 };
    pub const OPLOCU: ClientProt = ClientProt {
        id: 184,
        length: 12,
    };

    // Player ops
    pub const OPPLAYER1: ClientProt = ClientProt { id: 220, length: 2 };
    pub const OPPLAYER2: ClientProt = ClientProt { id: 51, length: 2 };
    pub const OPPLAYER3: ClientProt = ClientProt { id: 13, length: 2 };
    pub const OPPLAYER4: ClientProt = ClientProt { id: 189, length: 2 };
    pub const OPPLAYER5: ClientProt = ClientProt { id: 69, length: 2 };
    pub const OPPLAYERT: ClientProt = ClientProt { id: 138, length: 4 };
    pub const OPPLAYERU: ClientProt = ClientProt { id: 16, length: 8 };

    // Held / inv button
    pub const OPHELD1: ClientProt = ClientProt { id: 76, length: 6 };
    pub const OPHELD2: ClientProt = ClientProt { id: 177, length: 6 };
    pub const OPHELD3: ClientProt = ClientProt { id: 40, length: 6 };
    pub const OPHELD4: ClientProt = ClientProt { id: 191, length: 6 };
    pub const OPHELD5: ClientProt = ClientProt { id: 79, length: 6 };
    pub const OPHELDT: ClientProt = ClientProt { id: 112, length: 8 };
    pub const OPHELDU: ClientProt = ClientProt {
        id: 200,
        length: 12,
    };

    pub const INV_BUTTON1: ClientProt = ClientProt { id: 44, length: 6 };
    pub const INV_BUTTON2: ClientProt = ClientProt { id: 111, length: 6 };
    pub const INV_BUTTON3: ClientProt = ClientProt { id: 124, length: 6 };
    pub const INV_BUTTON4: ClientProt = ClientProt { id: 248, length: 6 };
    pub const INV_BUTTON5: ClientProt = ClientProt { id: 227, length: 6 };

    pub const IF_BUTTON: ClientProt = ClientProt { id: 86, length: 2 };
    pub const RESUME_PAUSEBUTTON: ClientProt = ClientProt { id: 166, length: 2 };
    pub const CLOSE_MODAL: ClientProt = ClientProt { id: 93, length: 0 };
    pub const RESUME_P_COUNTDIALOG: ClientProt = ClientProt { id: 180, length: 4 };
    pub const TUT_CLICKSIDE: ClientProt = ClientProt { id: 146, length: 1 };

    pub const MAP_BUILD_COMPLETE: ClientProt = ClientProt { id: 214, length: 0 };
    pub const MOVE_OPCLICK: ClientProt = ClientProt { id: 67, length: -1 };
    pub const SEND_SNAPSHOT: ClientProt = ClientProt { id: 94, length: 10 };
    pub const MOVE_MINIMAPCLICK: ClientProt = ClientProt {
        id: 236,
        length: -1,
    };
    pub const INV_BUTTOND: ClientProt = ClientProt { id: 253, length: 7 };
    pub const IGNORELIST_DEL: ClientProt = ClientProt { id: 251, length: 8 };
    pub const IGNORELIST_ADD: ClientProt = ClientProt { id: 192, length: 8 };
    pub const IDK_SAVEDESIGN: ClientProt = ClientProt { id: 27, length: 13 };
    pub const CHAT_SETMODE: ClientProt = ClientProt { id: 161, length: 3 };
    pub const MESSAGE_PRIVATE: ClientProt = ClientProt {
        id: 107,
        length: -1,
    };
    pub const FRIENDLIST_DEL: ClientProt = ClientProt { id: 203, length: 8 };
    pub const FRIENDLIST_ADD: ClientProt = ClientProt { id: 235, length: 8 };
    pub const CLIENT_CHEAT: ClientProt = ClientProt { id: 34, length: -1 };
    pub const MESSAGE_PUBLIC: ClientProt = ClientProt {
        id: 156,
        length: -1,
    };
    pub const MOVE_GAMECLICK: ClientProt = ClientProt {
        id: 234,
        length: -1,
    };
}

/// Map a 274 public [`ClientProt`] constant to the revision-selected row.
/// R289 fails closed on any 274 constant that is not in the stage-3 table.
pub fn map_client_prot(revision: super::ClientRevision, p274: ClientProt) -> ClientProt {
    use super::ClientRevision;
    match revision {
        ClientRevision::R274 => p274,
        ClientRevision::R289 => match p274.id {
            x if x == ClientProt::NO_TIMEOUT.id => ClientProt289::NO_TIMEOUT,
            x if x == ClientProt::IDLE_TIMER.id => ClientProt289::IDLE_TIMER,
            x if x == ClientProt::EVENT_MOUSE_CLICK.id => ClientProt289::EVENT_MOUSE_CLICK,
            x if x == ClientProt::EVENT_MOUSE_MOVE.id => ClientProt289::EVENT_MOUSE_MOVE,
            x if x == ClientProt::EVENT_APPLET_FOCUS.id => ClientProt289::EVENT_APPLET_FOCUS,
            x if x == ClientProt::EVENT_CAMERA_POSITION.id => ClientProt289::EVENT_CAMERA_POSITION,
            x if x == ClientProt::ANTICHEAT_OPLOGIC1.id => ClientProt289::ANTICHEAT_OPLOGIC1,
            x if x == ClientProt::ANTICHEAT_OPLOGIC2.id => ClientProt289::ANTICHEAT_OPLOGIC2,
            x if x == ClientProt::ANTICHEAT_OPLOGIC3.id => ClientProt289::ANTICHEAT_OPLOGIC3,
            x if x == ClientProt::ANTICHEAT_OPLOGIC4.id => ClientProt289::ANTICHEAT_OPLOGIC4,
            x if x == ClientProt::ANTICHEAT_OPLOGIC5.id => ClientProt289::ANTICHEAT_OPLOGIC5,
            x if x == ClientProt::ANTICHEAT_OPLOGIC6.id => ClientProt289::ANTICHEAT_OPLOGIC6,
            x if x == ClientProt::ANTICHEAT_OPLOGIC7.id => ClientProt289::ANTICHEAT_OPLOGIC7,
            x if x == ClientProt::ANTICHEAT_OPLOGIC8.id => ClientProt289::ANTICHEAT_OPLOGIC8,
            x if x == ClientProt::ANTICHEAT_OPLOGIC9.id => ClientProt289::ANTICHEAT_OPLOGIC9,
            x if x == ClientProt::ANTICHEAT_CYCLELOGIC1.id => ClientProt289::ANTICHEAT_CYCLELOGIC1,
            x if x == ClientProt::ANTICHEAT_CYCLELOGIC2.id => ClientProt289::ANTICHEAT_CYCLELOGIC2,
            x if x == ClientProt::ANTICHEAT_CYCLELOGIC3.id => ClientProt289::ANTICHEAT_CYCLELOGIC3,
            x if x == ClientProt::ANTICHEAT_CYCLELOGIC4.id => ClientProt289::ANTICHEAT_CYCLELOGIC4,
            x if x == ClientProt::ANTICHEAT_CYCLELOGIC5.id => ClientProt289::ANTICHEAT_CYCLELOGIC5,
            x if x == ClientProt::ANTICHEAT_CYCLELOGIC6.id => ClientProt289::ANTICHEAT_CYCLELOGIC6,
            x if x == ClientProt::ANTICHEAT_CYCLELOGIC7.id => ClientProt289::ANTICHEAT_CYCLELOGIC7,
            x if x == ClientProt::OPOBJ1.id => ClientProt289::OPOBJ1,
            x if x == ClientProt::OPOBJ2.id => ClientProt289::OPOBJ2,
            x if x == ClientProt::OPOBJ3.id => ClientProt289::OPOBJ3,
            x if x == ClientProt::OPOBJ4.id => ClientProt289::OPOBJ4,
            x if x == ClientProt::OPOBJ5.id => ClientProt289::OPOBJ5,
            x if x == ClientProt::OPOBJT.id => ClientProt289::OPOBJT,
            x if x == ClientProt::OPOBJU.id => ClientProt289::OPOBJU,
            x if x == ClientProt::OPNPC1.id => ClientProt289::OPNPC1,
            x if x == ClientProt::OPNPC2.id => ClientProt289::OPNPC2,
            x if x == ClientProt::OPNPC3.id => ClientProt289::OPNPC3,
            x if x == ClientProt::OPNPC4.id => ClientProt289::OPNPC4,
            x if x == ClientProt::OPNPC5.id => ClientProt289::OPNPC5,
            x if x == ClientProt::OPNPCT.id => ClientProt289::OPNPCT,
            x if x == ClientProt::OPNPCU.id => ClientProt289::OPNPCU,
            x if x == ClientProt::OPLOC1.id => ClientProt289::OPLOC1,
            x if x == ClientProt::OPLOC2.id => ClientProt289::OPLOC2,
            x if x == ClientProt::OPLOC3.id => ClientProt289::OPLOC3,
            x if x == ClientProt::OPLOC4.id => ClientProt289::OPLOC4,
            x if x == ClientProt::OPLOC5.id => ClientProt289::OPLOC5,
            x if x == ClientProt::OPLOCT.id => ClientProt289::OPLOCT,
            x if x == ClientProt::OPLOCU.id => ClientProt289::OPLOCU,
            x if x == ClientProt::OPPLAYER1.id => ClientProt289::OPPLAYER1,
            x if x == ClientProt::OPPLAYER2.id => ClientProt289::OPPLAYER2,
            x if x == ClientProt::OPPLAYER3.id => ClientProt289::OPPLAYER3,
            x if x == ClientProt::OPPLAYER4.id => ClientProt289::OPPLAYER4,
            x if x == ClientProt::OPPLAYER5.id => ClientProt289::OPPLAYER5,
            x if x == ClientProt::OPPLAYERT.id => ClientProt289::OPPLAYERT,
            x if x == ClientProt::OPPLAYERU.id => ClientProt289::OPPLAYERU,
            x if x == ClientProt::OPHELD1.id => ClientProt289::OPHELD1,
            x if x == ClientProt::OPHELD2.id => ClientProt289::OPHELD2,
            x if x == ClientProt::OPHELD3.id => ClientProt289::OPHELD3,
            x if x == ClientProt::OPHELD4.id => ClientProt289::OPHELD4,
            x if x == ClientProt::OPHELD5.id => ClientProt289::OPHELD5,
            x if x == ClientProt::OPHELDT.id => ClientProt289::OPHELDT,
            x if x == ClientProt::OPHELDU.id => ClientProt289::OPHELDU,
            x if x == ClientProt::INV_BUTTON1.id => ClientProt289::INV_BUTTON1,
            x if x == ClientProt::INV_BUTTON2.id => ClientProt289::INV_BUTTON2,
            x if x == ClientProt::INV_BUTTON3.id => ClientProt289::INV_BUTTON3,
            x if x == ClientProt::INV_BUTTON4.id => ClientProt289::INV_BUTTON4,
            x if x == ClientProt::INV_BUTTON5.id => ClientProt289::INV_BUTTON5,
            x if x == ClientProt::IF_BUTTON.id => ClientProt289::IF_BUTTON,
            x if x == ClientProt::RESUME_PAUSEBUTTON.id => ClientProt289::RESUME_PAUSEBUTTON,
            x if x == ClientProt::CLOSE_MODAL.id => ClientProt289::CLOSE_MODAL,
            x if x == ClientProt::RESUME_P_COUNTDIALOG.id => ClientProt289::RESUME_P_COUNTDIALOG,
            x if x == ClientProt::TUT_CLICKSIDE.id => ClientProt289::TUT_CLICKSIDE,
            x if x == ClientProt::MAP_BUILD_COMPLETE.id => ClientProt289::MAP_BUILD_COMPLETE,
            x if x == ClientProt::MOVE_OPCLICK.id => ClientProt289::MOVE_OPCLICK,
            x if x == ClientProt::REPORT_ABUSE.id => ClientProt289::SEND_SNAPSHOT,
            x if x == ClientProt::MOVE_MINIMAPCLICK.id => ClientProt289::MOVE_MINIMAPCLICK,
            x if x == ClientProt::INV_BUTTOND.id => ClientProt289::INV_BUTTOND,
            x if x == ClientProt::IGNORELIST_DEL.id => ClientProt289::IGNORELIST_DEL,
            x if x == ClientProt::IGNORELIST_ADD.id => ClientProt289::IGNORELIST_ADD,
            x if x == ClientProt::IDK_SAVEDESIGN.id => ClientProt289::IDK_SAVEDESIGN,
            x if x == ClientProt::CHAT_SETMODE.id => ClientProt289::CHAT_SETMODE,
            x if x == ClientProt::MESSAGE_PRIVATE.id => ClientProt289::MESSAGE_PRIVATE,
            x if x == ClientProt::FRIENDLIST_DEL.id => ClientProt289::FRIENDLIST_DEL,
            x if x == ClientProt::FRIENDLIST_ADD.id => ClientProt289::FRIENDLIST_ADD,
            x if x == ClientProt::CLIENT_CHEAT.id => ClientProt289::CLIENT_CHEAT,
            x if x == ClientProt::MESSAGE_PUBLIC.id => ClientProt289::MESSAGE_PUBLIC,
            x if x == ClientProt::MOVE_GAMECLICK.id => ClientProt289::MOVE_GAMECLICK,
            other => panic!(
                "R289 outbound fail-closed: no stage-3 mapping for 274 ClientProt id {other}"
            ),
        },
    }
}
