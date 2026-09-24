use client::client::{Client, ClientConfig, ClientNpc, ClientPlayer};
use client::io::Packet;
use client::render::Renderer;
use client::util::JString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn to_userhash_matches_client_ts() {
    let _r = Renderer::new(false);
    // values generated with webclient JString.ts toUserhash
    assert_eq!(JString::to_userhash("bob"), 3295);
    assert_eq!(JString::to_userhash("admin"), 2094917);
    assert_eq!(JString::to_userhash("Zz0_9"), 50082163);
    assert_eq!(JString::to_userhash("runescape"), 65254502242866);
    assert_eq!(JString::to_userhash("RuneScape"), 65254502242866);
    assert_eq!(JString::to_userhash("  bob  "), 3295);
    assert_eq!(
        JString::to_userhash("aaaaaaaaaaaaaaaaaaaa"),
        182859777940000980
    );
    assert_eq!(((JString::to_userhash("bob") >> 16) & 0x1f), 0); // loginServer byte
}

#[test]
fn cold_login_opcode_16_success() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        assert_eq!(hdr[0], 14); // login server probe
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap(); // response 0 → send seed
        s.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap(); // g8 seed
                                                         // read client loginout (variable); then grant
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf[0], 16); // cold login
        let size = buf[1] as usize;
        assert_eq!(size, n - 2);
        assert_eq!(buf[2], 255); // rev marker
        assert_eq!((buf[3] as usize) << 8 | buf[4] as usize, 274); // client version
        assert_eq!(buf[5], 0); // info: lowmem off
        assert_eq!(n, 2 + size);
        if client::login_rsa::login_rsan()
            .starts_with("7162900525229798032761816791230527296329313291")
        {
            // Java `Packet.rsaenc` writes `BigInteger.toByteArray()` length:
            // 64, or 65 with the leading 0x00 two's-complement byte when the
            // ciphertext MSB is set (random per login).
            let rsa_len = buf[42] as usize;
            assert!(rsa_len == 64 || rsa_len == 65, "rsa len byte {rsa_len}");
            assert_eq!(n, 2 + 40 + 1 + rsa_len);
        }
        s.write_all(&[2, 0, 0]).unwrap(); // response 2, staff=0, mouseTrack=0
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(c.gens.session, 1);
    assert_eq!(c.session_start_gens().session, c.gens.session);
    assert_eq!(c.session_start_gens().invalidations, c.gens.invalidations);
    assert!(c.local_player.is_some());
    assert!(c.players[2047].is_some());
    assert_eq!(c.login_user, "bob");
    assert_eq!(c.login_pass, "pw");
    server.join().unwrap();
}

/// Task 9: response 2 (cold login) must zero the entity counts and null
/// every player/npc table slot like Java `Client.java` 3647-3656, so a
/// second login does not draw leftover first-session NPCs/players. The
/// local player is then re-seeded fresh at `players[2047]` (ready = false).
#[test]
fn cold_login_clears_entity_tables() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        assert_eq!(hdr[0], 14); // login server probe
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap(); // response 0 → send seed
        s.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap(); // g8 seed
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf[0], 16); // cold login
        s.write_all(&[2, 0, 0]).unwrap(); // response 2, staff=0, mouseTrack=0
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    // dirty state a previous session left behind (Java logout keeps the
    // tables in place; response 2 is where they are cleared)
    c.npc_count = 3;
    c.npc[1] = Some(Box::new(ClientNpc::default()));
    c.player_count = 2;
    c.players[5] = Some(Box::new(ClientPlayer::default()));
    c.player_appearance_buffer[5] = Some(Box::new(Packet::new(vec![])));
    let local = ClientPlayer {
        ready: true,
        name: Some("leftover".into()),
        ..Default::default()
    };
    c.local_player = Some(local);

    c.login("bob", "pw", false).unwrap();
    assert_eq!(c.npc_count, 0, "response 2 must zero npc_count");
    assert!(
        c.npc[1].is_none(),
        "response 2 must null leftover npc slots"
    );
    assert_eq!(c.player_count, 0, "response 2 must zero player_count");
    assert!(
        c.players[5].is_none(),
        "response 2 must null leftover player slots"
    );
    assert!(
        c.player_appearance_buffer[5].is_none(),
        "response 2 must null leftover appearance buffers"
    );
    assert!(
        c.players[2047].is_some(),
        "response 2 must re-seed players[2047]"
    );
    assert!(c.local_player.is_some());
    assert!(
        !c.local_player.as_ref().unwrap().ready,
        "response 2 must re-seed a fresh local player"
    );
    server.join().unwrap();
}

#[test]
fn reconnect_uses_opcode_18() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
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
        assert_eq!(buf[0], 18);
        s.write_all(&[2, 0, 0]).unwrap();
    });
    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.login("bob", "pw", true).unwrap();
    server.join().unwrap();
}

#[test]
fn reconnect_response_15_keeps_game_and_local_player() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        // cold login grant (response 2)
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        assert_eq!(hdr[0], 14);
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap();
        s.write_all(&[0u8; 8]).unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        s.write_all(&[2, 0, 0]).unwrap();

        // reconnect grant (response 15, Java `Client.java` 3737)
        let (mut s2, _) = listener.accept().unwrap();
        let mut hdr2 = [0u8; 2];
        s2.read_exact(&mut hdr2).unwrap();
        assert_eq!(hdr2[0], 14);
        for _ in 0..8 {
            let _ = s2.write_all(&[0]);
        }
        s2.write_all(&[0]).unwrap();
        s2.write_all(&[0u8; 8]).unwrap();
        let mut buf2 = [0u8; 512];
        let n2 = s2.read(&mut buf2).unwrap();
        assert!(n2 > 0);
        assert_eq!(buf2[0], 18); // reconnect wrapper
        s2.write_all(&[15]).unwrap();
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(c.gens.session, 1);
    let p = c.local_player.as_mut().unwrap();
    p.y = 77; // marker: response 15 must not replace localPlayer
    c.bump_gens(client::io::ServerProt::PLAYER_INFO);
    c.bump_gens(client::io::ServerProt::UPDATE_INV_FULL);
    let previous_start = c.session_start_gens();
    c.login("bob", "pw", true).unwrap();
    assert!(c.ingame);
    assert!(c.stream.is_some());
    assert_eq!(c.local_player.as_ref().unwrap().y, 77);
    assert_eq!(c.gens.session, 2);
    let new_start = c.session_start_gens();
    assert_eq!(new_start.player_info, previous_start.player_info + 1);
    assert_eq!(new_start.inv, c.gens.inv);
    c.bump_gens(client::io::ServerProt::PLAYER_INFO);
    assert_eq!(c.session_start_gens().player_info, new_start.player_info);
    assert_eq!(c.gens.player_info, new_start.player_info + 1);
    server.join().unwrap();
}

#[test]
fn cold_login_response_2_resets_tab_chat_and_rebuilds_frame() {
    let mut r = Renderer::new(false);
    // Task 4c: a cold login after logout must restore the Java response-2
    // defaults (`sideTab = 3`, closed modals, empty chat, no minimap flag)
    // and call `prepareGame` so the game frame the title draw consumed is
    // rebuilt. Rust `login` previously skipped the field reset and never
    // called `prepare_game`; the logout-tab (10) redstone survived and the
    // frame stayed on "Loading - please wait." with title flames.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        assert_eq!(hdr[0], 14);
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap();
        s.write_all(&[0u8; 8]).unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf[0], 16); // cold login
        s.write_all(&[2, 0, 0]).unwrap(); // response 2, staff=0, mouseTrack=0
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    // A logged-in session that left the logout tab selected, the minimap
    // flagged, chat history, and the scene mid-load.
    c.ingame = true;
    c.active_icon = 10;
    c.scene_state = 2;
    c.minimap_level = 0;
    c.minimap_flag_x = 1;
    c.minimap_flag_z = 2;
    c.chat_text[0] = "leftover".into();
    c.logout();
    r.title_screen_draw(&mut c); // consumes the game frame (Task 4b)
    assert!(r.area_chat.is_none());
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(c.active_icon, 3, "response 2 must select the inventory tab");
    assert_eq!(c.scene_state, 0);
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
    assert_eq!(c.main_modal_id, -1);
    assert_eq!(c.minimap_level, -1);
    assert_eq!(c.minimap_flag_x, 0);
    assert_eq!(c.minimap_flag_z, 0);
    assert!(
        c.chat_text.iter().all(|t| t.is_empty()),
        "response 2 must clear the chat history"
    );
    assert!(c.redraw_frame && c.redraw_side && c.redraw_icons);
    // `login` no longer runs `prepare_game` eagerly (task 2b: the renderer
    // owns the game areas); the first `game_draw` rebuilds the frame.
    r.game_draw(&mut c);
    assert!(
        r.area_chat.is_some(),
        "prepare_game must rebuild the game frame"
    );
    assert!(
        r.image_title2.is_none(),
        "prepare_game must unload the title regions"
    );
    server.join().unwrap();
}

#[test]
fn login_code_6_is_error() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        let _ = s.read_exact(&mut hdr);
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        let _ = s.write_all(&[6]);
    });
    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    let e = c.login("bob", "pw", false).unwrap_err();
    assert_eq!(e.code, 6);
    assert!(!c.ingame);
    assert_eq!(c.loginscreen, 2, "failed login stays on the title form");
    assert_eq!(c.gens.session, 0);
    assert_eq!(c.session_start_gens().session, 0);
}

/// Response 5 ("already logged in") is a title-screen error, not a process
/// abort: `login_mes*` carry the Java lines, credentials stay, and a later
/// `login` on the same `Client` can succeed (bot-host retry).
#[test]
fn already_logged_in_leaves_title_ready_to_retry() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
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
        let _ = s.read(&mut buf).unwrap();
        s.write_all(&[5]).unwrap();
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    let e = c.login("bob", "pw", false).unwrap_err();
    assert_eq!(e.code, 5);
    assert_eq!(e.mes1, "Your account is already logged in.");
    assert_eq!(c.login_mes1, "Your account is already logged in.");
    assert_eq!(c.login_mes2, "Try again in 60 secs...");
    assert!(!c.ingame);
    assert_eq!(c.loginscreen, 2);
    assert_eq!(c.login_user, "bob");
    assert_eq!(c.login_pass, "pw");
    server.join().unwrap();
}

#[test]
fn failed_login_can_be_retried_on_the_same_client() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for grant in [5u8, 2u8] {
            let (mut s, _) = listener.accept().unwrap();
            let mut hdr = [0u8; 2];
            s.read_exact(&mut hdr).unwrap();
            for _ in 0..8 {
                let _ = s.write_all(&[0]);
            }
            s.write_all(&[0]).unwrap();
            s.write_all(&[0u8; 8]).unwrap();
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).unwrap();
            if grant == 2 {
                s.write_all(&[2, 0, 0]).unwrap();
            } else {
                s.write_all(&[grant]).unwrap();
            }
        }
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    assert!(c.login("bob", "pw", false).is_err());
    assert!(!c.ingame);
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    server.join().unwrap();
}

#[test]
fn externally_owned_response_one_returns_after_one_socket_attempt() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        stream.read_exact(&mut hdr).unwrap();
        for _ in 0..8 {
            stream.write_all(&[0]).unwrap();
        }
        stream.write_all(&[1]).unwrap();
    });
    let mut client = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    client.set_external_reconnect_owner(true);
    let error = client.login("bob", "pw", false).unwrap_err();
    assert_eq!(error.code, 1);
    server.join().unwrap();
}

#[test]
fn standalone_response_one_retries_after_two_seconds() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for response in [1, 2] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut hdr = [0u8; 2];
            stream.read_exact(&mut hdr).unwrap();
            for _ in 0..8 {
                stream.write_all(&[0]).unwrap();
            }
            stream.write_all(&[response]).unwrap();
            if response == 2 {
                stream.write_all(&[0, 0]).unwrap();
            }
        }
    });
    let mut client = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    client.login("bob", "pw", false).unwrap();
    assert!(client.ingame);
    server.join().unwrap();
}

#[test]
fn externally_owned_response_21_carries_server_delay() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        stream.read_exact(&mut hdr).unwrap();
        for _ in 0..8 {
            stream.write_all(&[0]).unwrap();
        }
        stream.write_all(&[21, 3]).unwrap();
    });
    let mut client = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    client.set_external_reconnect_owner(true);

    let error = client.login("bob", "pw", false).unwrap_err();

    assert_eq!(error.code, 21);
    assert_eq!(error.retry_after, Some(Duration::from_secs(3)));
    assert_eq!(error.mes1, "You have only just left another world");
    assert_eq!(
        error.mes2,
        "Your profile will be transferred in: 3 seconds"
    );
    server.join().unwrap();
}

#[test]
fn standalone_response_21_counts_down_then_retries() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for response in [21, 2] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut hdr = [0u8; 2];
            stream.read_exact(&mut hdr).unwrap();
            for _ in 0..8 {
                stream.write_all(&[0]).unwrap();
            }
            stream.write_all(&[response]).unwrap();
            match response {
                21 => stream.write_all(&[0]).unwrap(),
                2 => stream.write_all(&[0, 0]).unwrap(),
                _ => unreachable!(),
            }
        }
    });
    let mut client = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    let started = Instant::now();

    client.login("bob", "pw", false).unwrap();

    assert!(client.ingame);
    assert!(started.elapsed() >= Duration::from_millis(900));
    server.join().unwrap();
}

#[test]
fn lowmem_login_writes_info_byte_one() {
    let _r = Renderer::new(false);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap();
        s.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf[5], 1, "lowmem login info byte");
        s.write_all(&[2, 0, 0]).unwrap();
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: true,
    });
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    server.join().unwrap();
}
