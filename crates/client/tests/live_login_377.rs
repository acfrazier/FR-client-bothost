//! Explicit, ignored live proof. It can contact only Fairy Ring's local 377
//! endpoint. Ordinary test runs never open a live engine connection.
use client::client::{Client, ClientConfig, ClientRevision, MiniMenuAction};
use client::io::packet::PacketObserver;
use client::render::backend::FrameOutput;
use client::render::Renderer;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[path = "support/live_engine_377.rs"]
mod live_engine_377;

#[derive(Default)]
struct Outbound(Mutex<Vec<u8>>);
impl PacketObserver for Outbound {
    fn packet_body_length(&self, opcode: u8) -> i16 {
        client::io::ClientProt377::ALL
            .iter()
            .find(|packet| packet.id == i32::from(opcode))
            .expect("outbound packet must be declared by Java 377")
            .length as i16
    }
    fn packet_committed(&self, opcode: u8, _: usize) {
        eprintln!("377 outbound opcode={opcode}");
        self.0.lock().unwrap().push(opcode);
    }
}

fn save_frame(path: &Path, output: FrameOutput) {
    let FrameOutput::PixMap(map) = output else {
        panic!("377 proof requires the CPU renderer")
    };
    assert!(
        map.pixels.iter().any(|pixel| pixel & 0x00ff_ffff != 0),
        "a scene2 receipt must contain rendered pixels"
    );
    let rgb: Vec<u8> = map
        .pixels
        .iter()
        .flat_map(|&rgb| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
        .collect();
    image::codecs::jpeg::JpegEncoder::new_with_quality(std::fs::File::create(path).unwrap(), 95)
        .encode(
            &rgb,
            map.width as u32,
            map.height as u32,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    eprintln!(
        "377 screenshot {} {}x{}",
        path.display(),
        map.width,
        map.height
    );
}

fn reach_scene(client: &mut Client, renderer: &mut Renderer, path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut scene_ticks = 0;
    while Instant::now() < deadline {
        client.mainloop();
        assert!(
            client.ingame,
            "377 session ended before scene2: {:?}",
            client.take_session_exit_reason()
        );
        let output = renderer.mainredraw(client);
        if client.loop_cycle % 50 == 0 {
            let local = client.local_player.as_ref().unwrap();
            eprintln!(
                "377 cycle={} scene={} tile={},{} ready={} players={} npcs={}",
                client.loop_cycle,
                client.scene_state,
                client.map_build_base_x + local.route_x[0],
                client.map_build_base_z + local.route_z[0],
                local.ready,
                client.player_count,
                client.npc_count
            );
        }
        if client.scene_state == 2 && client.local_player.as_ref().is_some_and(|p| p.ready) {
            scene_ticks += 1;
            if scene_ticks == 75 {
                assert!(client.npc_count > 0, "real tutorial NPCs must arrive");
                let local = client.local_player.as_ref().unwrap();
                eprintln!(
                    "377 scene2 tile={},{} local={:?} npcs={}",
                    client.map_build_base_x + local.route_x[0],
                    client.map_build_base_z + local.route_z[0],
                    local.name,
                    client.npc_count
                );
                save_frame(path, output);
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "377 scene2 deadline: scene={} player_info={} maps={:?}/{:?}",
        client.scene_state,
        client.awaiting_player_info,
        client.map_build_ground_file,
        client.map_build_location_file
    );
}

fn server_logout(client: &mut Client, renderer: &mut Renderer) {
    // Feed the real interface action through the same `clientButton` gate as UI.
    assert_eq!(client.if_(2458).unwrap().client_code, 205);
    client.menu_action[0] = MiniMenuAction::IF_BUTTON;
    client.menu_param_a[0] = 0;
    client.menu_param_b[0] = 0;
    client.menu_param_c[0] = 2458;
    client.doAction(0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while client.ingame && Instant::now() < deadline {
        client.mainloop();
        drop(renderer.mainredraw(client));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!client.ingame, "377 engine must acknowledge logout");
    let reason = client.take_session_exit_reason().unwrap();
    eprintln!("377 logout receipt: {reason}");
    assert!(
        format!("{reason}").contains("server_logout"),
        "logout must come from the server, not local teardown"
    );
}

#[test]
#[ignore = "requires orchestrator-started Fairy Ring 377 engine on localhost:81/43595"]
fn first_login_scene2_logout_relog_scene2() {
    let evidence =
        std::env::var("CLIENT_377_LIVE_DIR").expect("explicit live evidence directory required");
    let cache_dir =
        std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("matched running 377 cache required");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("explicit t2a account required");
    assert!(user.starts_with("t2a"));
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists(), "quiet lock blocks live runs");
    }
    live_engine_377::require_engine(Path::new(&evidence), &user, &cache_dir);
    Renderer::set_prefer_gpu(false);
    let mut client = Client::new_with_revision_and_http_port(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir,
            members: true,
            lowmem: true,
        },
        ClientRevision::R377,
        81,
    );
    client.draw = true;
    let mut renderer = Renderer::new(true);
    client.maininit_with_progress(Some(&mut |client, message, percent| {
        eprintln!("377 init {percent}: {message}");
        renderer.draw_progress(client, message, percent);
    }));
    assert!(!client.error_loading, "377 cache/assets must initialize");
    let outbound = Arc::new(Outbound::default());
    client.out.set_observer(Some(outbound.clone()));
    for session in 1..=2 {
        eprintln!("377 login session={session} user={user}");
        client.set_login_deadline(Instant::now() + Duration::from_secs(10));
        client.login(&user, "t2apass", false).unwrap();
        assert!(client.ingame);
        reach_scene(
            &mut client,
            &mut renderer,
            &Path::new(&evidence).join(format!("rust-scene2-session-{session}.jpg")),
        );
        server_logout(&mut client, &mut renderer);
    }
    let packets = outbound.0.lock().unwrap();
    assert!(packets.contains(&6));
    assert!(packets.contains(&40));
    assert_eq!(packets.iter().filter(|&&opcode| opcode == 79).count(), 2);
    assert!(packets.iter().all(|opcode| {
        client::io::ClientProt377::ALL
            .iter()
            .any(|packet| packet.id == i32::from(*opcode))
    }));
    eprintln!("377 live proof complete: scene2, IF_BUTTON/server logout, relog scene2, second clean logout; outbound={packets:?}");
}
