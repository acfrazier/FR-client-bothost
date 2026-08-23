// doAction/tryMove encode: menu arrays → MOVE_GAMECLICK / OPNPC2 through
// Packet::p1_enc with the client's outbound Isaac.
use std::sync::Arc;

use client::client::{Client, ClientConfig, ClientNpc, ClientPlayer, MiniMenuAction};
use client::config::if_type::{ButtonType, IfType};
use client::config::ObjType;
use client::dash3d::{Model, SceneModel};
use client::io::{ClientProt, Isaac};

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

// doAction(WALK) arms World mouse picking with the applet click converted to
// scene-local (b-4, c-4); the walk packet is written by game_loop's ground
// consume after the next render (Task 7).
#[test]
fn walk_menu_arms_mouse_picking() {
    let mut c = client();
    c.ingame = true;
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    // the menu-open WALK uses the stored param coords (TS 9218-9219)
    c.is_menu_open = true;
    c.menu_action[0] = MiniMenuAction::WALK;
    c.menu_param_a[0] = 0;
    c.menu_param_b[0] = 104; // applet mouse x
    c.menu_param_c[0] = 84; // applet mouse y
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.doAction(0);
    assert!(c.world.click);
    assert_eq!(c.world.click_x, 100);
    assert_eq!(c.world.click_y, 80);
    assert_eq!(c.out.pos, 0);
}

/// Spellbook tiles are BUTTON_TARGET (`magic.if` wind_strike). Clicking
/// one must arm target mode, not be a no-op.
#[test]
fn tgt_button_arms_spell_target_mode() {
    let mut c = client();
    c.ifaces.resize(8, None);
    c.ifaces[7] = Some(IfType {
        id: 7,
        button_type: ButtonType::BUTTON_TARGET,
        target_verb: "Cast on".into(),
        target_base: "Wind strike".into(),
        target_mask: 3,
        ..IfType::default()
    });
    c.menu_action[0] = MiniMenuAction::TGT_BUTTON;
    c.menu_param_c[0] = 7;
    c.doAction(0);
    assert_eq!(c.target_mode, 1);
    assert_eq!(c.target_com_id, 7);
    assert_eq!(c.target_mask, 3);
    assert_eq!(c.use_mode, 0);
}

#[test]
fn op_npc2_writes_opnpc2() {
    let mut c = client();
    c.ingame = true;
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    c.menu_action[0] = MiniMenuAction::OP_NPC2;
    c.menu_param_a[0] = 42; // npc index
                            // the OP_NPC branches guard on the scene entity and walk to it first:
                            // both stand on (5,5), so the walk is a zero-step MOVE_OPCLICK
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.npc[42] = Some(ClientNpc::at(5, 5));
    c.doAction(0);
    // MOVE_OPCLICK id 138: (138 + -621246914) & 0xff
    let walk_enc = (ClientProt::MOVE_OPCLICK.id.wrapping_add(-621246914)) as u8;
    assert_eq!(walk_enc, 200);
    // size 5 (2*1+3), ctrl 0, single absolute tile (5,5), no steps
    assert_eq!(&c.out.data()[..7], &[walk_enc, 5, 0, 0, 5, 0, 5]);
    // then the action itself: OPNPC2 id 233 encoded with the *second* Isaac
    // value (the walk consumed the first): (233 + 1957022519) & 0xff
    let enc = (ClientProt::OPNPC2.id.wrapping_add(1957022519)) as u8;
    assert_eq!(enc, 32);
    assert_eq!(&c.out.data()[7..10], [enc, 0, 42]);
    assert_eq!(c.out.pos, 10);
}

#[test]
fn try_move_writes_move_gameclick() {
    let mut c = client();
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, 0));
    assert_eq!(&c.out.data()[..c.out.pos], &[13, 5, 0, 0, 10, 0, 10]);
}

#[test]
fn try_move_type_1_writes_minimapclick_size_plus_14() {
    let mut c = client();
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, 1));
    // MOVE_MINIMAPCLICK id 86: (86 + -621246914) & 0xff
    let enc = (ClientProt::MOVE_MINIMAPCLICK.id.wrapping_add(-621246914)) as u8;
    assert_eq!(c.out.data()[0], enc);
    // size byte includes the extra 14 bytes of the minimap packet
    assert_eq!(c.out.data()[1], 1 + 1 + 3 + 14);
}

#[test]
fn try_move_out_of_area_destination_returns_false() {
    let mut c = client();
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    // a destination outside the 104x104 build area is never reached
    assert!(!c.tryMove(5, 5, 200, 200, false, 0, 0, 0, 0, 0, 0));
    assert_eq!(c.out.pos, 0);
}

/// After login, `tryMove` + `game_loop` must flush `MOVE_GAMECLICK` onto
/// the socket (opcode Isaac-encoded; payload plaintext).
#[test]
fn try_move_flushes_move_gameclick_to_socket() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap();
        s.write_all(&[0u8; 8]).unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        s.write_all(&[2, 0, 0]).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut out = [0u8; 32];
        let n = s.read(&mut out).unwrap();
        tx.send(out[..n].to_vec()).unwrap();
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.login("bob", "pw", false).unwrap();
    assert!(c.local_player.is_some());
    assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, 0));
    c.game_loop();
    let bytes = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // opcode is Isaac-encoded; payload is size 5, ctrl 0, tile (10,10)
    assert!(bytes.len() >= 7);
    assert_eq!(&bytes[1..7], &[5, 0, 0, 10, 0, 10]);
    server.join().unwrap();
}

// ---- Task 5: remaining doAction arms ----

/// `OP_OBJ6` examines through `add_chat`: no desc → "It's a <name>."
#[test]
fn do_action_obj_examine_adds_chat() {
    let mut c = client();
    // Robust to an ambient real config pack: the premise is obj 0 with no
    // desc (examine falls back to "It's a <name>.").
    if c.cache.objs.is_empty() {
        Arc::get_mut(&mut c.cache).unwrap().objs.resize(1, ObjType::default());
    }
    Arc::get_mut(&mut c.cache).unwrap().objs[0].name = "Coins".into();
    Arc::get_mut(&mut c.cache).unwrap().objs[0].desc = String::new();
    c.menu_num_entries = 1;
    c.menu_action[0] = MiniMenuAction::OP_OBJ6;
    c.menu_param_a[0] = 0;
    c.doAction(0);
    assert_eq!(c.chat_text[0], "It's a Coins.");
}

/// `INV_BUTTON1` writes the raw opcode first (no Isaac → p1_enc is plain),
/// then `p2(a) p2(b) p2(c)` = obj, slot, com (TS 9099-9127).
#[test]
fn do_action_inv_button1_encodes() {
    let mut c = client();
    c.menu_action[0] = MiniMenuAction::INV_BUTTON1;
    c.menu_param_b[0] = 3; // slot
    c.menu_param_c[0] = 10; // com
    c.doAction(0);
    let d = c.out.data();
    assert_eq!(d[0], ClientProt::INV_BUTTON1.id as u8);
    // a (obj) defaults to 0; obj, slot, com big-endian after the opcode
    assert_eq!(&d[..7], &[74, 0, 0, 0, 3, 0, 10]);
}

/// `PAUSE_BUTTON` latches `resumed_pause_button` (TS 9186-9191).
#[test]
fn do_action_pause_button_latches_please_wait() {
    let mut c = client();
    c.menu_action[0] = MiniMenuAction::PAUSE_BUTTON;
    c.menu_param_c[0] = 5;
    c.doAction(0);
    assert!(c.resumed_pause_button);
    // the RESUME_PAUSEBUTTON frame is the raw opcode + p2(c)
    assert_eq!(&c.out.data()[..3], &[72, 0, 5]);
}

/// `OP_LOC1` routes through `interactWithLoc`: the packet is
/// `p1_enc(opcode) p2(x + base) p2(z + base) p2(locId)` — locId from the
/// typecode, not the naive shape/angle bytes.
#[test]
fn do_action_op_loc_encodes_loc_id_not_shape() {
    let mut c = client();
    c.local_player = Some(ClientPlayer::at(5, 5));
    // typecode: entity 2, locId 1, x=10, z=12
    let type_id = 1i32;
    let x = 10i32;
    let z = 12i32;
    let typecode = (2 << 29) | ((type_id & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.world.add_scenery(0, x, z, 0, Some(SceneModel::Model(Model::default())), typecode, 0, 1, 1, 0);
    assert!(c.world.type_code2(0, x, z, typecode) >= 0);
    c.menu_action[0] = MiniMenuAction::OP_LOC1;
    c.menu_param_a[0] = typecode;
    c.menu_param_b[0] = x;
    c.menu_param_c[0] = z;
    c.doAction(0);
    // the walk (MOVE_OPCLICK) precedes it, so assert the trailing frame:
    // OPLOC1 id 215 raw, p2(10) p2(12) p2(locId 1)
    let d = c.out.data();
    assert!(c.out.pos >= 7);
    assert_eq!(&d[c.out.pos - 7..c.out.pos], &[215, 0, 10, 0, 12, 0, 1]);
    assert_eq!(c.cross_mode, 2);
    assert_eq!(c.cross_x, c.shell.mouse_click_x);
}

/// `OP_HELD2` writes `p2(obj) p2(slot) p2(com)` in TS 8958-8980 order and
/// arms the `selected_*` outline fields.
#[test]
fn do_action_op_held_writes_p2_obj_slot_com() {
    let mut c = client();
    c.menu_action[0] = MiniMenuAction::OP_HELD2;
    c.menu_param_a[0] = 7; // obj
    c.menu_param_b[0] = 3; // slot
    c.menu_param_c[0] = 10; // com
    c.doAction(0);
    // OPHELD2 id 2 raw, then p2(obj) p2(slot) p2(com)
    assert_eq!(&c.out.data()[..7], &[2, 0, 7, 0, 3, 0, 10]);
    assert_eq!(c.selected_item, 3);
    assert_eq!(c.selected_com_id, 10);
    assert_eq!(c.selected_area, 2);
}

/// `USEHELD_START` arms Use mode and returns before the trailing
/// `use_mode = 0` wipe (TS 9013-9022), so the mode sticks.
#[test]
fn do_action_useheld_start_returns_with_use_mode_armed() {
    let mut c = client();
    if c.cache.objs.is_empty() {
        Arc::get_mut(&mut c.cache).unwrap().objs.resize(1, ObjType::default());
    }
    Arc::get_mut(&mut c.cache).unwrap().objs[0].name = "Coins".into();
    c.menu_action[0] = MiniMenuAction::USEHELD_START;
    c.menu_param_a[0] = 0; // obj
    c.menu_param_b[0] = 3; // slot
    c.menu_param_c[0] = 10; // com
    c.doAction(0);
    assert_eq!(c.use_mode, 1);
    assert_eq!(c.obj_com_id, 0);
    assert_eq!(c.obj_selected_slot, 3);
    assert_eq!(c.obj_selected_com_id, 10);
    assert_eq!(c.obj_selected_name, "Coins");
    assert_eq!(c.out.pos, 0);
}

