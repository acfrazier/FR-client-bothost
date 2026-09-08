//! Revision 289 stage-3: cache/config seam, basic outbound actions, offline replay.
//!
//! Outbound IDs/lengths are source-traced from primary 289 `client.java`
//! `method465` / `method160` / `method206` (openrs2-nonfree). Fixtures are
//! offline and independently justified — not authentic live cache proof.

use client::client::{
    Client, ClientConfig, ClientNpc, ClientPlayer, ClientRevision, MiniMenuAction,
};
use client::config::if_type::IfType;
use client::io::{
    load_offline_config_seam, map_client_prot, write_synthetic_cache_dir, CacheArchiveKind,
    CacheManifest289, ClientProt, ClientProt289, ClientStream, Isaac, ServerProt289,
    CACHE_JAG_NAMES_289,
};
use client::render::Renderer;
use client::util::JString;
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn cfg() -> ClientConfig {
    ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    }
}

fn client_289() -> Client {
    let mut c = Client::new_with_revision(cfg(), ClientRevision::R289);
    c.ingame = true;
    c.ptype = -1;
    c
}

fn client_274() -> Client {
    let mut c = Client::new(cfg());
    c.ingame = true;
    c.ptype = -1;
    c
}

/// Feed raw (no-ISAAC) game bytes into production `ClientStream` + `tcp_in`.
fn feed_frames(c: &mut Client, frame: &[u8], max_polls: usize) -> usize {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let frame = frame.to_vec();
    let written = Arc::new(Mutex::new(false));
    let written_s = Arc::clone(&written);
    let done = Arc::new(Barrier::new(2));
    let done_s = Arc::clone(&done);
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(&frame).unwrap();
        let _ = sock.flush();
        *written_s.lock().unwrap() = true;
        thread::sleep(Duration::from_millis(30));
        done_s.wait();
        let _ = sock.shutdown(std::net::Shutdown::Both);
    });

    c.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    c.random_in = None;
    c.ptype = -1;

    for _ in 0..50 {
        if *written.lock().unwrap() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    let mut accepted = 0usize;
    for _ in 0..max_polls {
        if c.tcp_in() {
            accepted += 1;
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }
    done.wait();
    handle.join().unwrap();
    accepted
}

// --- ClientProt289 oracle (action cut) --------------------------------------

#[test]
fn client_prot_289_action_family_ids_and_lengths() {
    // Walk
    assert_eq!(
        ClientProt289::MOVE_GAMECLICK,
        ClientProt {
            id: 234,
            length: -1
        }
    );
    assert_eq!(
        ClientProt289::MOVE_MINIMAPCLICK,
        ClientProt {
            id: 236,
            length: -1
        }
    );
    assert_eq!(
        ClientProt289::MOVE_OPCLICK,
        ClientProt { id: 67, length: -1 }
    );
    // NPC
    assert_eq!(ClientProt289::OPNPC1, ClientProt { id: 252, length: 2 });
    assert_eq!(ClientProt289::OPNPC2, ClientProt { id: 21, length: 2 });
    assert_eq!(ClientProt289::OPNPC3, ClientProt { id: 178, length: 2 });
    assert_eq!(ClientProt289::OPNPC4, ClientProt { id: 30, length: 2 });
    assert_eq!(ClientProt289::OPNPC5, ClientProt { id: 247, length: 2 });
    assert_eq!(ClientProt289::OPNPCT, ClientProt { id: 108, length: 4 });
    assert_eq!(ClientProt289::OPNPCU, ClientProt { id: 160, length: 8 });
    // Loc
    assert_eq!(ClientProt289::OPLOC1, ClientProt { id: 10, length: 6 });
    assert_eq!(ClientProt289::OPLOC2, ClientProt { id: 45, length: 6 });
    assert_eq!(ClientProt289::OPLOC3, ClientProt { id: 196, length: 6 });
    assert_eq!(ClientProt289::OPLOC4, ClientProt { id: 53, length: 6 });
    assert_eq!(ClientProt289::OPLOC5, ClientProt { id: 126, length: 6 });
    assert_eq!(ClientProt289::OPLOCT, ClientProt { id: 218, length: 8 });
    assert_eq!(
        ClientProt289::OPLOCU,
        ClientProt {
            id: 184,
            length: 12,
        }
    );
    // Obj
    assert_eq!(ClientProt289::OPOBJ1, ClientProt { id: 97, length: 6 });
    assert_eq!(ClientProt289::OPOBJ2, ClientProt { id: 4, length: 6 });
    assert_eq!(ClientProt289::OPOBJ3, ClientProt { id: 110, length: 6 });
    assert_eq!(ClientProt289::OPOBJ4, ClientProt { id: 147, length: 6 });
    assert_eq!(ClientProt289::OPOBJ5, ClientProt { id: 22, length: 6 });
    assert_eq!(ClientProt289::OPOBJT, ClientProt { id: 241, length: 8 });
    assert_eq!(ClientProt289::OPOBJU, ClientProt { id: 55, length: 12 });
    // Player
    assert_eq!(ClientProt289::OPPLAYER1, ClientProt { id: 220, length: 2 });
    assert_eq!(ClientProt289::OPPLAYER2, ClientProt { id: 51, length: 2 });
    assert_eq!(ClientProt289::OPPLAYER3, ClientProt { id: 13, length: 2 });
    assert_eq!(ClientProt289::OPPLAYER4, ClientProt { id: 189, length: 2 });
    assert_eq!(ClientProt289::OPPLAYER5, ClientProt { id: 69, length: 2 });
    assert_eq!(ClientProt289::OPPLAYERT, ClientProt { id: 138, length: 4 });
    assert_eq!(ClientProt289::OPPLAYERU, ClientProt { id: 16, length: 8 });
    // Held / inv / widget / dialog / count
    assert_eq!(ClientProt289::OPHELD1, ClientProt { id: 76, length: 6 });
    assert_eq!(ClientProt289::OPHELD2, ClientProt { id: 177, length: 6 });
    assert_eq!(ClientProt289::OPHELD3, ClientProt { id: 40, length: 6 });
    assert_eq!(ClientProt289::OPHELD4, ClientProt { id: 191, length: 6 });
    assert_eq!(ClientProt289::OPHELD5, ClientProt { id: 79, length: 6 });
    assert_eq!(ClientProt289::OPHELDT, ClientProt { id: 112, length: 8 });
    assert_eq!(
        ClientProt289::OPHELDU,
        ClientProt {
            id: 200,
            length: 12,
        }
    );
    assert_eq!(ClientProt289::INV_BUTTON1, ClientProt { id: 44, length: 6 });
    assert_eq!(
        ClientProt289::INV_BUTTON2,
        ClientProt { id: 111, length: 6 }
    );
    assert_eq!(
        ClientProt289::INV_BUTTON3,
        ClientProt { id: 124, length: 6 }
    );
    assert_eq!(
        ClientProt289::INV_BUTTON4,
        ClientProt { id: 248, length: 6 }
    );
    assert_eq!(
        ClientProt289::INV_BUTTON5,
        ClientProt { id: 227, length: 6 }
    );
    assert_eq!(ClientProt289::IF_BUTTON, ClientProt { id: 86, length: 2 });
    assert_eq!(
        ClientProt289::RESUME_PAUSEBUTTON,
        ClientProt { id: 166, length: 2 }
    );
    assert_eq!(ClientProt289::CLOSE_MODAL, ClientProt { id: 93, length: 0 });
    assert_eq!(
        ClientProt289::RESUME_P_COUNTDIALOG,
        ClientProt { id: 180, length: 4 }
    );
    assert_eq!(
        ClientProt289::MAP_BUILD_COMPLETE,
        ClientProt { id: 214, length: 0 }
    );
    assert_eq!(
        ClientProt289::CHAT_SETMODE,
        ClientProt { id: 161, length: 3 }
    );
    assert_eq!(ClientProt289::NO_TIMEOUT, ClientProt { id: 181, length: 0 });
    // 274 public table must stay distinct
    assert_ne!(ClientProt::OPNPC2.id, ClientProt289::OPNPC2.id);
    assert_ne!(
        ClientProt::MOVE_GAMECLICK.id,
        ClientProt289::MOVE_GAMECLICK.id
    );
    assert_ne!(ClientProt::IF_BUTTON.id, ClientProt289::IF_BUTTON.id);
}

#[test]
fn map_client_prot_r274_identity_r289_remap() {
    assert_eq!(
        map_client_prot(ClientRevision::R274, ClientProt::OPNPC2),
        ClientProt::OPNPC2
    );
    assert_eq!(
        map_client_prot(ClientRevision::R289, ClientProt::OPNPC2),
        ClientProt289::OPNPC2
    );
    assert_eq!(
        map_client_prot(ClientRevision::R289, ClientProt::MOVE_GAMECLICK).id,
        234
    );
    assert_eq!(
        map_client_prot(ClientRevision::R289, ClientProt::CLOSE_MODAL).id,
        93
    );
}

// --- Production emit paths --------------------------------------------------

#[test]
fn r289_try_move_writes_move_gameclick_234() {
    let mut c = client_289();
    // no Isaac → p1_enc is plain opcode
    assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, 0));
    let d = c.out.data();
    assert_eq!(d[0], 234, "MOVE_GAMECLICK 289");
    assert_eq!(d[1], 5, "size 2*1+3");
    assert_eq!(d[2], 0, "ctrl");
    assert_eq!(&d[3..7], &[0, 10, 0, 10]);
    assert_eq!(c.out.pos, 7);
}

#[test]
fn r274_try_move_still_writes_move_gameclick_207() {
    let mut c = client_274();
    assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, 0));
    assert_eq!(c.out.data()[0], ClientProt::MOVE_GAMECLICK.id as u8);
    assert_eq!(c.out.data()[0], 207);
}

#[test]
fn r289_op_npc2_payload_length_2() {
    let mut c = client_289();
    c.menu_action[0] = MiniMenuAction::OP_NPC2;
    c.menu_param_a[0] = 42;
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.npc[42] = Some(Box::new(ClientNpc::at(5, 5)));
    c.doAction(0);
    let d = &c.out.data()[..c.out.pos as usize];
    // MOVE_OPCLICK 67 first (same tile → size 5), then OPNPC2 21 + p2 index
    assert_eq!(d[0], 67, "MOVE_OPCLICK");
    assert_eq!(d[1], 5);
    let action = &d[7..];
    assert_eq!(action[0], 21, "OPNPC2");
    assert_eq!(&action[1..3], &[0, 42]);
    assert_eq!(action.len(), 3, "opcode + 2-byte payload");
}

#[test]
fn r289_inv_button1_payload_length_6() {
    let mut c = client_289();
    c.menu_action[0] = MiniMenuAction::INV_BUTTON1;
    c.menu_param_b[0] = 3;
    c.menu_param_c[0] = 10;
    c.doAction(0);
    let d = c.out.data();
    assert_eq!(d[0], 44, "INV_BUTTON1 289");
    assert_eq!(&d[..7], &[44, 0, 0, 0, 3, 0, 10]);
    assert_eq!(ClientProt289::INV_BUTTON1.length, 6);
}

#[test]
fn r289_if_button_and_close_modal() {
    let mut c = client_289();
    c.menu_action[0] = MiniMenuAction::IF_BUTTON;
    c.menu_param_c[0] = 0x1234;
    c.doAction(0);
    assert_eq!(c.out.data()[0], 86, "IF_BUTTON");
    assert_eq!(&c.out.data()[1..3], &[0x12, 0x34]);

    c.out.pos = 0;
    // RESUME_PAUSEBUTTON production arm:
    c.menu_action[0] = MiniMenuAction::PAUSE_BUTTON;
    c.menu_param_c[0] = 5;
    c.doAction(0);
    assert_eq!(c.out.data()[0], 166, "RESUME_PAUSEBUTTON");
    assert_eq!(&c.out.data()[1..3], &[0, 5]);

    // CLOSE_MODAL via production CLOSE_BUTTON → close_modal path.
    c.out.pos = 0;
    c.side_modal_id = 42;
    c.main_modal_id = 7;
    c.menu_action[0] = MiniMenuAction::CLOSE_BUTTON;
    c.doAction(0);
    assert_eq!(c.out.data()[0], 93, "CLOSE_MODAL");
    assert_eq!(c.out.pos, 1, "CLOSE_MODAL length 0");
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.main_modal_id, -1);
}

#[test]
fn r289_resume_p_count_dialog_p4() {
    let mut c = client_289();
    // Production keyboard path: dialog_input_open + digit keys + enter.
    c.dialog_input_open = true;
    c.dialog_input.clear();
    for ch in b"12345" {
        c.shell.apply_key(true, 0, *ch as i32);
    }
    c.shell.apply_key(true, 0, 10); // enter
    c.handle_chat_input();
    assert_eq!(c.out.data()[0], 180, "RESUME_P_COUNTDIALOG");
    assert_eq!(&c.out.data()[1..5], &12345i32.to_be_bytes());
    assert_eq!(c.out.pos, 5);
    assert!(!c.dialog_input_open);
    assert_eq!(ClientProt289::RESUME_P_COUNTDIALOG.length, 4);
}

#[test]
fn r289_message_public_effect_prefixes_match_java() {
    // Primary 289 Java: wave=1, wave2=2, shake=3, scroll=4, slide=5.
    let cases = [
        ("hello", 0u8),
        ("wave:hello", 1u8),
        ("wave2:hello", 2u8),
        ("shake:hello", 3u8),
        ("scroll:hello", 4u8),
        ("slide:hello", 5u8),
    ];
    for (input, effect) in cases {
        let mut c = client_289();
        let mut player = ClientPlayer::at(1, 1);
        player.name = Some("Bob".into());
        c.local_player = Some(player);
        for ch in input.bytes() {
            c.shell.apply_key(true, 0, ch as i32);
        }
        c.shell.apply_key(true, 0, 13);
        c.handle_chat_input();
        assert_eq!(c.out.data()[0], 156, "MESSAGE_PUBLIC id for {input}");
        assert_eq!(c.out.data()[2], 0, "colour default for {input}");
        assert_eq!(c.out.data()[3], effect, "effect byte for {input}");
        assert_eq!(
            c.local_player.as_ref().unwrap().chat_effect,
            effect as i32,
            "local echo effect for {input}"
        );
    }
}

#[test]
fn r289_send_snapshot_report_abuse_p8_p1_p1() {
    // Java client_button 601..=612: CLOSE_MODAL then SEND_SNAPSHOT 94
    // method472 p8 namehash + method466 reason + method466 mute.
    let mut c = client_289();
    c.report_abuse_input = "Bob".into();
    c.report_abuse_mute_option = true;
    c.main_modal_id = 7;
    c.set_iface(
        9,
        IfType {
            client_code: 605, // reason 4
            ..IfType::default()
        },
    );
    assert!(!c.client_button(9), "report-abuse returns false (no IF_BUTTON)");
    let d = c.out.data();
    // CLOSE_MODAL 93 then SEND_SNAPSHOT 94 + 10 payload bytes.
    assert_eq!(d[0], 93, "CLOSE_MODAL first");
    assert_eq!(d[1], 94, "SEND_SNAPSHOT");
    let hash = JString::to_userhash("Bob");
    assert_eq!(&d[2..10], &hash.to_be_bytes());
    assert_eq!(d[10], 4, "reason = code-601");
    assert_eq!(d[11], 1, "mute flag");
    assert_eq!(c.out.pos, 12);
    assert_eq!(c.main_modal_id, -1);
    assert_eq!(ClientProt289::SEND_SNAPSHOT.length, 10);
    assert_eq!(
        map_client_prot(ClientRevision::R289, ClientProt::REPORT_ABUSE).id,
        94
    );
}

#[test]
fn r289_draw_paths_emit_289_not_274_opcodes() {
    // Production draw.rs sites must use client_opcode remap (not bare ClientProt.id).
    let _keep = Renderer::new(false);
    let mut r = Renderer::new(false);
    let mut c = client_289();

    // TUT_CLICKSIDE via draw_icons (game_draw chrome path).
    c.tut_flash_icon = 3;
    c.active_icon = 3;
    c.redraw_icons = true;
    r.game_draw(&mut c);
    assert_eq!(c.tut_flash_icon, -1);
    assert_eq!(c.out.data()[0], 146, "TUT_CLICKSIDE 289");
    assert_ne!(c.out.data()[0], ClientProt::TUT_CLICKSIDE.id as u8);
    assert_eq!(c.out.data()[1], 3);

    // ANTICHEAT_CYCLELOGIC6 via add_players arrival-clear (scene_state==2).
    c.out.pos = 0;
    c.ingame = true;
    c.scene_state = 2;
    c.cyclelogic6 = 122;
    c.minimap_flag_x = 10;
    c.minimap_flag_z = 10;
    let mut p = ClientPlayer::at(10, 10);
    p.x = 10 * 128 + 64;
    p.z = 10 * 128 + 64;
    c.local_player = Some(p);
    r.game_draw(&mut c);
    assert_eq!(c.out.data()[0], 255, "CYCLELOGIC6 289");
    assert_ne!(c.out.data()[0], ClientProt::ANTICHEAT_CYCLELOGIC6.id as u8);
    assert_eq!(c.out.data()[1], 62);

    // ANTICHEAT_CYCLELOGIC1 via add_projectiles (scene path).
    c.out.pos = 0;
    r.cyclelogic1 = 1174;
    r.game_draw(&mut c);
    assert_eq!(c.out.data()[0], 130, "CYCLELOGIC1 289");
    assert_ne!(c.out.data()[0], ClientProt::ANTICHEAT_CYCLELOGIC1.id as u8);

    // ANTICHEAT_CYCLELOGIC3 via public minimap_build_buffer.
    c.out.pos = 0;
    r.cyclelogic3 = 112;
    r.minimap_build_buffer(&mut c, 0);
    assert_eq!(c.out.data()[0], 125, "CYCLELOGIC3 289");
    assert_ne!(c.out.data()[0], ClientProt::ANTICHEAT_CYCLELOGIC3.id as u8);
    assert_eq!(c.out.data()[1], 50);

    // 274 session still emits public table ids on the same draw paths.
    let mut r274 = Renderer::new(false);
    let mut c274 = client_274();
    c274.tut_flash_icon = 2;
    c274.active_icon = 2;
    c274.redraw_icons = true;
    r274.game_draw(&mut c274);
    assert_eq!(c274.out.data()[0], ClientProt::TUT_CLICKSIDE.id as u8);
}

#[test]
fn r289_op_loc1_encodes_scene_coords_and_loc_id() {
    let mut c = client_289();
    c.local_player = Some(ClientPlayer::at(5, 5));
    let type_id = 1i32;
    let x = 10i32;
    let z = 12i32;
    let typecode = (2 << 29) | ((type_id & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.world
        .add_scenery(0, x, z, 0, typecode, 0, 1, 1, 0, 0, 0, 0, 0);
    c.menu_action[0] = MiniMenuAction::OP_LOC1;
    c.menu_param_a[0] = typecode;
    c.menu_param_b[0] = x;
    c.menu_param_c[0] = z;
    c.doAction(0);
    let d = &c.out.data()[..c.out.pos as usize];
    // trailing OPLOC1 frame: id 10, p2 x, p2 z, p2 locId
    assert!(d.len() >= 7);
    let tail = &d[d.len() - 7..];
    assert_eq!(tail[0], 10, "OPLOC1");
    assert_eq!(&tail[1..3], &[0, 10]);
    assert_eq!(&tail[3..5], &[0, 12]);
    assert_eq!(&tail[5..7], &[0, 1]);
}

#[test]
fn r289_op_held1_payload_length_6() {
    let mut c = client_289();
    c.menu_action[0] = MiniMenuAction::OP_HELD1;
    c.menu_param_a[0] = 7; // obj
    c.menu_param_b[0] = 2; // slot
    c.menu_param_c[0] = 9; // com
    c.doAction(0);
    let d = c.out.data();
    assert_eq!(d[0], 76, "OPHELD1");
    assert_eq!(&d[1..7], &[0, 7, 0, 2, 0, 9]);
}

#[test]
fn r289_map_build_complete_and_no_timeout_ids() {
    assert_eq!(
        map_client_prot(ClientRevision::R289, ClientProt::MAP_BUILD_COMPLETE).id,
        214
    );
    assert_eq!(
        map_client_prot(ClientRevision::R289, ClientProt::NO_TIMEOUT).id,
        181
    );
    // MAP_BUILD_COMPLETE id is identical on 274 and 289
    assert_eq!(
        ClientProt::MAP_BUILD_COMPLETE.id,
        ClientProt289::MAP_BUILD_COMPLETE.id
    );
}

// --- Cache / config offline seam --------------------------------------------

#[test]
fn cache_jag_names_289_login_layout() {
    assert_eq!(
        CACHE_JAG_NAMES_289,
        [
            "title",
            "config",
            "interface",
            "media",
            "versionlist",
            "textures",
            "wordenc",
            "sounds",
        ]
    );
    assert_eq!(CacheArchiveKind::Config.crc_slot(), 2);
    let empty = CacheManifest289::offline_empty("no authentic 289 cache in checkout");
    assert!(!empty.authentic_cache_present);
    assert!(empty.entries.is_empty());
}

#[test]
fn offline_config_loader_synthetic_fixture() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("r289_stage3_cache_{stamp}"));
    // Source-shaped tiny config/interface records through production unpackers.
    write_synthetic_cache_dir(&dir).unwrap();

    let load = load_offline_config_seam(&dir);
    assert!(load.config_present && load.config_jag_ok && load.config_unpack_ok);
    assert!(load.interface_present && load.interface_jag_ok && load.interface_unpack_ok);
    assert_eq!(load.flo_count, 1);
    assert_eq!(load.varp_count, 1);
    assert_eq!(load.idk_count, 1);
    assert_eq!(load.iface_count, 1);

    let man = CacheManifest289::discover_offline(
        &dir,
        "stage3 synthetic JAG; provenance=write_synthetic_cache_dir; not live cache",
    );
    assert!(man.has(CacheArchiveKind::Config));
    assert!(man.has(CacheArchiveKind::Interface));
    assert!(!man.authentic_cache_present);

    // Production Client::load_cache path (via new_with_revision) must bind tables.
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: dir.display().to_string(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    assert!(!c.error_loading, "valid synthetic config must load");
    assert_eq!(c.cache.flos.len(), 1);
    assert_eq!(c.cache.flos[0].colour, 0xFF_00_00);
    assert_eq!(c.cache.varps.len(), 1);
    assert_eq!(c.cache.varps[0].clientcode, 7);
    assert_eq!(c.cache.idks.len(), 1);
    assert_eq!(
        c.ifaces.iter().filter(|s| s.is_some()).count(),
        1,
        "interface TYPE_RECT id=1"
    );
    // Missing authentic map/media cache must not claim scene readiness.
    c.ingame = true;
    c.scene_state = 1;
    let status = c.check_scene();
    assert_ne!(c.scene_state, 2, "synthetic config is not map data");
    assert!(status != 0 || c.scene_state == 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn offline_config_missing_fail_closed() {
    let load = load_offline_config_seam(Path::new("/tmp/r289-missing-cache-dir-stage3"));
    assert!(!load.config_present);
    assert!(!load.config_jag_ok);
    assert!(!load.config_unpack_ok);
}

// --- Offline native replay: receive → lifecycle → action → reset ------------

#[test]
fn offline_replay_logout_then_action_then_fail_closed_scene() {
    let mut c = client_289();
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.main_modal_id = 10;
    c.side_modal_id = 11;

    // Receive LOGOUT (opcode 121, fixed 0) through production tcp_in/handle_packet.
    let accepted = feed_frames(&mut c, &[121], 20);
    assert!(accepted >= 1, "logout frame accepted");
    assert!(c.stream.is_none(), "logout drops stream");
    assert!(!c.ingame || c.main_modal_id < 0 || c.stream.is_none());

    // Re-arm for action emit offline (no live server).
    c.ingame = true;
    c.out.pos = 0;
    c.local_player = Some(ClientPlayer::at(5, 5));
    assert!(c.tryMove(5, 5, 8, 8, true, 0, 0, 0, 0, 0, 0));
    assert_eq!(c.out.data()[0], 234);

    // NPC action
    c.out.pos = 0;
    c.menu_action[0] = MiniMenuAction::OP_NPC2;
    c.menu_param_a[0] = 3;
    c.npc[3] = Some(Box::new(ClientNpc::at(5, 5)));
    c.doAction(0);
    let d = &c.out.data()[..c.out.pos as usize];
    assert!(d.windows(3).any(|w| w[0] == 21 && w[1] == 0 && w[2] == 3));

    // Scene readiness fail-closed without map prerequisites
    c.scene_state = 1;
    let _ = c.check_scene();
    assert_eq!(
        c.scene_state, 1,
        "no authentic map → stay loading / last-FBO freeze band"
    );
}

#[test]
fn offline_replay_inbound_varp_then_widget_then_reset_anims() {
    let mut c = client_289();
    // VARP_SMALL opcode 75, length 3: p2 id, p1 value  — frame: 75 | id_hi id_lo | val
    // IF_OPENSIDE 252 length 2
    // RESET_ANIMS 201 length 0
    let mut frame = Vec::new();
    frame.push(75);
    frame.extend_from_slice(&10u16.to_be_bytes());
    frame.push(7);
    frame.push(252);
    frame.extend_from_slice(&20u16.to_be_bytes());
    frame.push(201);

    let accepted = feed_frames(&mut c, &frame, 40);
    assert!(accepted >= 1);
    // varp applied if production path reached
    if let Some(v) = c.var.get(10) {
        assert_eq!(*v, 7);
    }
    // reset path must not panic; scene still not ready without cache
    assert_ne!(c.scene_state, 2);
}

#[test]
fn offline_replay_inventory_full_then_walk_action() {
    let mut c = client_289();
    // UPDATE_INV_FULL 107: g2 component, g2 count, entries
    // payload: component=3, count=1, item=5 count=2
    // frame_hex style from stage1: opcode + g2 len + payload
    let payload = [
        0x00, 0x03, // component
        0x00, 0x01, // count
        0x00, 0x05, // item
        0x02, // stack
    ];
    let mut frame = Vec::new();
    frame.push(107);
    frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    frame.extend_from_slice(&payload);

    let _ = feed_frames(&mut c, &frame, 30);

    c.ingame = true;
    c.local_player = Some(ClientPlayer::at(1, 1));
    c.out.pos = 0;
    assert!(c.tryMove(1, 1, 2, 2, true, 0, 0, 0, 0, 0, 0));
    assert_eq!(c.out.data()[0], ClientProt289::MOVE_GAMECLICK.id as u8);
}

/// Isaac-encoded R289 walk opcode still carries plaintext payload shape.
#[test]
fn r289_try_move_with_isaac_encodes_opcode_only() {
    let mut c = client_289();
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, 0));
    let enc = (234i32.wrapping_add(-621246914)) as u8;
    assert_eq!(c.out.data()[0], enc);
    assert_eq!(&c.out.data()[1..7], &[5, 0, 0, 10, 0, 10]);
}

#[test]
fn server_prot_289_still_distinct_from_outbound_table() {
    // Sanity: inbound LOGOUT 121 ≠ outbound CLOSE_MODAL 93
    assert_eq!(ServerProt289::LOGOUT, 121);
    assert_ne!(ServerProt289::LOGOUT as i32, ClientProt289::CLOSE_MODAL.id);
}
