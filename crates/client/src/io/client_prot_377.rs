//! Generated revision-377 client-to-server packet metadata.
//! Source: Client-Java Client.java, pinned in docs/revision-377/protocol-377.json.
//! Regenerate with tools/generate_revision_377_contract.py. No revision dispatch is installed.

use super::ClientProt;

pub struct ClientProt377;

impl ClientProt377 {
    // src/main/java/jagex2/client/Client.java:9924-9941
    pub const OPHELDU: ClientProt = ClientProt { id: 1, length: 12 };
    // src/main/java/jagex2/client/Client.java:9782-9796
    pub const INV_BUTTON1: ClientProt = ClientProt { id: 3, length: 6 };
    // src/main/java/jagex2/client/Client.java:10143-10157
    pub const OPHELD5: ClientProt = ClientProt { id: 4, length: 6 };
    // src/main/java/jagex2/client/Client.java:3500-3502
    pub const MAP_BUILD_COMPLETE: ClientProt = ClientProt { id: 6, length: 0 };
    // src/main/java/jagex2/client/Client.java:9997-9999
    pub const OPNPC5: ClientProt = ClientProt { id: 8, length: 2 };
    // src/main/java/jagex2/client/Client.java:9976-9978
    pub const OPNPC3: ClientProt = ClientProt { id: 13, length: 2 };
    // src/main/java/jagex2/client/Client.java:3123-3125
    pub const EVENT_MOUSE_CLICK: ClientProt = ClientProt { id: 19, length: 4 };
    // src/main/java/jagex2/client/Client.java:4561-4563
    pub const ANTICHEAT_CYCLELOGIC6: ClientProt = ClientProt { id: 22, length: 2 };
    // src/main/java/jagex2/client/Client.java:9859-9873
    pub const OPHELD2: ClientProt = ClientProt { id: 24, length: 6 };
    // src/main/java/jagex2/client/Client.java:7184-7186
    pub const MOVE_GAMECLICK: ClientProt = ClientProt { id: 28, length: -1 };
    // src/main/java/jagex2/client/Client.java:10013-10016
    pub const OPPLAYERT: ClientProt = ClientProt { id: 31, length: 4 };
    // src/main/java/jagex2/client/Client.java:9944-9959
    pub const OPHELDT: ClientProt = ClientProt { id: 36, length: 8 };
    // src/main/java/jagex2/client/Client.java:3409-3410
    pub const NO_TIMEOUT: ClientProt = ClientProt { id: 40, length: 0 };
    // src/main/java/jagex2/client/Client.java:9806-9808
    pub const OPNPC4: ClientProt = ClientProt { id: 42, length: 2 };
    // src/main/java/jagex2/client/Client.java:9729-9731
    pub const OPPLAYER5: ClientProt = ClientProt { id: 45, length: 2 };
    // src/main/java/jagex2/client/Client.java:5037-5068
    pub const MESSAGE_PUBLIC: ClientProt = ClientProt { id: 49, length: -1 };
    // src/main/java/jagex2/client/Client.java:10194-10198
    pub const OPLOC3: ClientProt = ClientProt { id: 50, length: 6 };
    // src/main/java/jagex2/client/Client.java:9852-9856
    pub const OPOBJ4: ClientProt = ClientProt { id: 54, length: 6 };
    // src/main/java/jagex2/client/Client.java:10178-10182
    pub const OPLOC5: ClientProt = ClientProt { id: 55, length: 6 };
    // src/main/java/jagex2/client/Client.java:4972-4975
    pub const CLIENT_CHEAT: ClientProt = ClientProt { id: 56, length: -1 };
    // src/main/java/jagex2/client/Client.java:9883-9888
    pub const OPNPCU: ClientProt = ClientProt { id: 57, length: 8 };
    // src/main/java/jagex2/client/Client.java:9742-9744
    pub const OPNPC2: ClientProt = ClientProt { id: 67, length: 2 };
    // src/main/java/jagex2/client/Client.java:10066-10070
    pub const OPOBJ3: ClientProt = ClientProt { id: 71, length: 6 };
    // src/main/java/jagex2/client/Client.java:4896-4898
    pub const RESUME_P_COUNTDIALOG: ClientProt = ClientProt { id: 75, length: 4 };
    // src/main/java/jagex2/client/Client.java:10042-10046
    pub const OPOBJ1: ClientProt = ClientProt { id: 77, length: 6 };
    // src/main/java/jagex2/client/Client.java:3655-3657
    pub const EVENT_HAS_WINDOW: ClientProt = ClientProt { id: 78, length: 4 };
    // src/main/java/jagex2/client/Client.java:9892-9901
    pub const IF_BUTTON: ClientProt = ClientProt { id: 79, length: 2 };
    // src/main/java/jagex2/client/Client.java:3844-3846
    pub const EVENT_SYNTH_ERROR: ClientProt = ClientProt { id: 80, length: 2 };
    // src/main/java/jagex2/client/Client.java:10471-10476
    pub const OPOBJT: ClientProt = ClientProt { id: 83, length: 8 };
    // src/main/java/jagex2/client/Client.java:10357-10371
    pub const INV_BUTTON3: ClientProt = ClientProt { id: 91, length: 6 };
    // src/main/java/jagex2/client/Client.java:10345-10348
    pub const ANTICHEAT_OPLOGIC1: ClientProt = ClientProt { id: 95, length: 4 };
    // src/main/java/jagex2/client/Client.java:10350-10354
    pub const OPOBJ2: ClientProt = ClientProt { id: 100, length: 6 };
    // src/main/java/jagex2/client/Client.java:10027-10030
    pub const OPNPCT: ClientProt = ClientProt { id: 104, length: 4 };
    // src/main/java/jagex2/client/Client.java:4569-4596
    pub const CLOSE_MODAL: ClientProt = ClientProt { id: 110, length: 0 };
    // src/main/java/jagex2/client/Client.java:10457-10459
    pub const OPNPC1: ClientProt = ClientProt { id: 112, length: 2 };
    // src/main/java/jagex2/client/Client.java:9819-9821
    pub const OPPLAYER4: ClientProt = ClientProt { id: 116, length: 2 };
    // src/main/java/jagex2/client/Client.java:5841-5843
    pub const TUT_CLICKSIDE: ClientProt = ClientProt { id: 119, length: 1 };
    // src/main/java/jagex2/client/Client.java:12461-12463
    pub const FRIENDLIST_ADD: ClientProt = ClientProt { id: 120, length: 8 };
    // src/main/java/jagex2/client/Client.java:3257-3262
    pub const INV_BUTTOND: ClientProt = ClientProt { id: 123, length: 7 };
    // src/main/java/jagex2/client/Client.java:9750-9753
    pub const ANTICHEAT_OPLOGIC5: ClientProt = ClientProt { id: 126, length: 1 };
    // src/main/java/jagex2/client/Client.java:10261-10265
    pub const OPLOC4: ClientProt = ClientProt { id: 136, length: 6 };
    // src/main/java/jagex2/client/Client.java:3140-3143
    pub const EVENT_CAMERA_POSITION: ClientProt = ClientProt { id: 140, length: 4 };
    // src/main/java/jagex2/client/Client.java:12482-12485
    pub const FRIENDLIST_DEL: ClientProt = ClientProt { id: 141, length: 8 };
    // src/main/java/jagex2/client/Client.java:10290-10295
    pub const OPPLAYERU: ClientProt = ClientProt { id: 143, length: 8 };
    // src/main/java/jagex2/client/Client.java:9772-9779
    pub const OPLOCU: ClientProt = ClientProt {
        id: 152,
        length: 12,
    };
    // src/main/java/jagex2/client/Client.java:9971-9974
    pub const ANTICHEAT_OPLOGIC3: ClientProt = ClientProt { id: 157, length: 4 };
    // src/main/java/jagex2/client/Client.java:10160-10174
    pub const INV_BUTTON5: ClientProt = ClientProt { id: 158, length: 6 };
    // src/main/java/jagex2/client/Client.java:12531-12534
    pub const IGNORELIST_DEL: ClientProt = ClientProt { id: 160, length: 8 };
    // src/main/java/jagex2/client/Client.java:10201-10215
    pub const OPHELD3: ClientProt = ClientProt { id: 161, length: 6 };
    // src/main/java/jagex2/client/Client.java:12032-12041
    pub const IDK_SAVEDESIGN: ClientProt = ClientProt {
        id: 163,
        length: 13,
    };
    // src/main/java/jagex2/client/Client.java:9700-9703
    pub const ANTICHEAT_OPLOGIC2: ClientProt = ClientProt { id: 165, length: 1 };
    // src/main/java/jagex2/client/Client.java:5746-5747
    pub const ANTICHEAT_CYCLELOGIC2: ClientProt = ClientProt { id: 168, length: 0 };
    // src/main/java/jagex2/client/Client.java:3040-3095
    pub const EVENT_MOUSE_MOVE: ClientProt = ClientProt {
        id: 171,
        length: -1,
    };
    // src/main/java/jagex2/client/Client.java:3748-3750
    pub const ANTICHEAT_CYCLELOGIC4: ClientProt = ClientProt { id: 173, length: 3 };
    // src/main/java/jagex2/client/Client.java:4522-4526
    pub const CHAT_SETMODE: ClientProt = ClientProt { id: 176, length: 3 };
    // src/main/java/jagex2/client/Client.java:10103-10117
    pub const INV_BUTTON2: ClientProt = ClientProt { id: 177, length: 6 };
    // src/main/java/jagex2/client/Client.java:10186-10190
    pub const OPLOC1: ClientProt = ClientProt { id: 181, length: 6 };
    // src/main/java/jagex2/client/Client.java:12049-12053
    pub const SEND_SNAPSHOT: ClientProt = ClientProt {
        id: 184,
        length: 10,
    };
    // src/main/java/jagex2/client/Client.java:3149-3151
    pub const EVENT_APPLET_FOCUS: ClientProt = ClientProt { id: 187, length: 1 };
    // src/main/java/jagex2/client/Client.java:10511-10513
    pub const OPPLAYER3: ClientProt = ClientProt { id: 194, length: 2 };
    // src/main/java/jagex2/client/Client.java:6696-6698
    pub const ANTICHEAT_CYCLELOGIC3: ClientProt = ClientProt { id: 197, length: 4 };
    // src/main/java/jagex2/client/Client.java:3344-3345
    pub const IDLE_TIMER: ClientProt = ClientProt { id: 202, length: 0 };
    // src/main/java/jagex2/client/Client.java:9755-9769
    pub const OPHELD1: ClientProt = ClientProt { id: 203, length: 6 };
    // src/main/java/jagex2/client/Client.java:4917-4919
    pub const RESUME_P_NAMEDIALOG: ClientProt = ClientProt { id: 206, length: 8 };
    // src/main/java/jagex2/client/Client.java:9982-9987
    pub const OPLOCT: ClientProt = ClientProt { id: 210, length: 8 };
    // src/main/java/jagex2/client/Client.java:10307-10314
    pub const OPOBJU: ClientProt = ClientProt {
        id: 211,
        length: 12,
    };
    // src/main/java/jagex2/client/Client.java:7189-7191
    pub const MOVE_MINIMAPCLICK: ClientProt = ClientProt {
        id: 213,
        length: -1,
    };
    // src/main/java/jagex2/client/Client.java:12514-12516
    pub const IGNORELIST_ADD: ClientProt = ClientProt { id: 217, length: 8 };
    // src/main/java/jagex2/client/Client.java:10061-10064
    pub const ANTICHEAT_OPLOGIC4: ClientProt = ClientProt { id: 222, length: 3 };
    // src/main/java/jagex2/client/Client.java:10254-10257
    pub const RESUME_PAUSEBUTTON: ClientProt = ClientProt { id: 226, length: 2 };
    // src/main/java/jagex2/client/Client.java:4841-4859
    pub const MESSAGE_PRIVATE: ClientProt = ClientProt {
        id: 227,
        length: -1,
    };
    // src/main/java/jagex2/client/Client.java:9705-9719
    pub const OPHELD4: ClientProt = ClientProt { id: 228, length: 6 };
    // src/main/java/jagex2/client/Client.java:10276-10280
    pub const OPOBJ5: ClientProt = ClientProt { id: 230, length: 6 };
    // src/main/java/jagex2/client/Client.java:10396-10410
    pub const INV_BUTTON4: ClientProt = ClientProt { id: 231, length: 6 };
    // src/main/java/jagex2/client/Client.java:9911-9913
    pub const OPPLAYER2: ClientProt = ClientProt { id: 233, length: 2 };
    // src/main/java/jagex2/client/Client.java:10389-10393
    pub const OPLOC2: ClientProt = ClientProt { id: 241, length: 6 };
    // src/main/java/jagex2/client/Client.java:6232-6248
    pub const ANTICHEAT_CYCLELOGIC1: ClientProt = ClientProt {
        id: 244,
        length: -1,
    };
    // src/main/java/jagex2/client/Client.java:9692-9694
    pub const OPPLAYER1: ClientProt = ClientProt { id: 245, length: 2 };
    // src/main/java/jagex2/client/Client.java:7194-7196
    pub const MOVE_OPCLICK: ClientProt = ClientProt {
        id: 247,
        length: -1,
    };
    // src/main/java/jagex2/client/Client.java:6170-6171
    pub const ANTICHEAT_CYCLELOGIC5: ClientProt = ClientProt { id: 248, length: 0 };

    /// Every opcode emitted by the pinned Java client, ordered by wire ID.
    pub const ALL: [ClientProt; 80] = [
        Self::OPHELDU,
        Self::INV_BUTTON1,
        Self::OPHELD5,
        Self::MAP_BUILD_COMPLETE,
        Self::OPNPC5,
        Self::OPNPC3,
        Self::EVENT_MOUSE_CLICK,
        Self::ANTICHEAT_CYCLELOGIC6,
        Self::OPHELD2,
        Self::MOVE_GAMECLICK,
        Self::OPPLAYERT,
        Self::OPHELDT,
        Self::NO_TIMEOUT,
        Self::OPNPC4,
        Self::OPPLAYER5,
        Self::MESSAGE_PUBLIC,
        Self::OPLOC3,
        Self::OPOBJ4,
        Self::OPLOC5,
        Self::CLIENT_CHEAT,
        Self::OPNPCU,
        Self::OPNPC2,
        Self::OPOBJ3,
        Self::RESUME_P_COUNTDIALOG,
        Self::OPOBJ1,
        Self::EVENT_HAS_WINDOW,
        Self::IF_BUTTON,
        Self::EVENT_SYNTH_ERROR,
        Self::OPOBJT,
        Self::INV_BUTTON3,
        Self::ANTICHEAT_OPLOGIC1,
        Self::OPOBJ2,
        Self::OPNPCT,
        Self::CLOSE_MODAL,
        Self::OPNPC1,
        Self::OPPLAYER4,
        Self::TUT_CLICKSIDE,
        Self::FRIENDLIST_ADD,
        Self::INV_BUTTOND,
        Self::ANTICHEAT_OPLOGIC5,
        Self::OPLOC4,
        Self::EVENT_CAMERA_POSITION,
        Self::FRIENDLIST_DEL,
        Self::OPPLAYERU,
        Self::OPLOCU,
        Self::ANTICHEAT_OPLOGIC3,
        Self::INV_BUTTON5,
        Self::IGNORELIST_DEL,
        Self::OPHELD3,
        Self::IDK_SAVEDESIGN,
        Self::ANTICHEAT_OPLOGIC2,
        Self::ANTICHEAT_CYCLELOGIC2,
        Self::EVENT_MOUSE_MOVE,
        Self::ANTICHEAT_CYCLELOGIC4,
        Self::CHAT_SETMODE,
        Self::INV_BUTTON2,
        Self::OPLOC1,
        Self::SEND_SNAPSHOT,
        Self::EVENT_APPLET_FOCUS,
        Self::OPPLAYER3,
        Self::ANTICHEAT_CYCLELOGIC3,
        Self::IDLE_TIMER,
        Self::OPHELD1,
        Self::RESUME_P_NAMEDIALOG,
        Self::OPLOCT,
        Self::OPOBJU,
        Self::MOVE_MINIMAPCLICK,
        Self::IGNORELIST_ADD,
        Self::ANTICHEAT_OPLOGIC4,
        Self::RESUME_PAUSEBUTTON,
        Self::MESSAGE_PRIVATE,
        Self::OPHELD4,
        Self::OPOBJ5,
        Self::INV_BUTTON4,
        Self::OPPLAYER2,
        Self::OPLOC2,
        Self::ANTICHEAT_CYCLELOGIC1,
        Self::OPPLAYER1,
        Self::MOVE_OPCLICK,
        Self::ANTICHEAT_CYCLELOGIC5,
    ];
}
