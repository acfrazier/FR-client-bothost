//! Sound-enabled host-style shared/profile construction and real 377 login.
use client::client::{Client, ClientRevision, MiniMenuAction};
use client::config::runtime_377::load_cache_377;
use client::render::backend::FrameOutput;
use client::render::Renderer;
use client::{ClientSessionConfig, ClientSessionProfile, Transport};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[path = "support/live_engine_377.rs"]
mod live_engine_377;

#[test]
#[ignore = "requires orchestrator-started Fairy Ring 377 engine on localhost:81/43595"]
fn bound_highmem_constructor_login_scene2_and_server_logout() {
    let evidence = std::env::var("CLIENT_377_LIVE_DIR").expect("explicit live evidence required");
    let cache =
        std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("fresh runtime cache required");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("explicit account required");
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists(), "quiet lock blocks live runs");
    }
    let engine = live_engine_377::require_engine(Path::new(&evidence), &user, &cache);
    let (rsa_modulus, rsa_exponent) =
        client::login_rsa::rsa_from_pkcs1_pem_file(&engine.join("data/config/private.pem"))
            .unwrap();
    let versionlist = std::fs::read(Path::new(&cache).join("versionlist")).unwrap();
    let content_id = format!("{:x}", Sha256::digest(versionlist))[..16].to_owned();
    let profile = Arc::new(
        ClientSessionProfile::new(ClientSessionConfig {
            revision: ClientRevision::R377,
            transport: Transport::Tcp,
            game_host: "127.0.0.1".into(),
            game_port: 43595,
            asset_host: "127.0.0.1".into(),
            asset_port: 81,
            cache_dir: Path::new(&cache).into(),
            unpack_dir: client::unpack_dir(),
            rsa_modulus,
            rsa_exponent,
            expected_crc: None,
            content_id,
            file_store_dir: None,
            ondemand_persist_dir: None,
            map_archive: None,
        })
        .unwrap(),
    );
    let tables = load_cache_377(&cache, false).unwrap();
    // host-play SharedClientTemplate::prepare_client(uid, false) reaches this
    // same bound constructor over the high-memory 377 tables. No sound bypass.
    let mut client = Client::from_shared_with_profile(
        profile.client_config(true, false),
        Arc::new(tables.cache),
        Arc::new(tables.ifaces),
        tables.ifaces_mut,
        profile,
    )
    .unwrap();
    assert!(!client.config.lowmem);
    assert_eq!(client.jagfx.synth.len(), 5000);
    for id in [1000, 1278, 1863, 2726] {
        assert!(client.jagfx.synth[id].is_some(), "loaded sound {id}");
    }
    eprintln!(
        "377 bound highmem construction: sound enabled, 5000 slots, ids 1000/1278/1863/2726 loaded"
    );
    Renderer::set_prefer_gpu(false);
    client.draw = true;
    let mut renderer = Renderer::new(true);
    client.maininit_with_progress(Some(&mut |client, message, percent| {
        eprintln!("377 sound init {percent}: {message}");
        renderer.draw_progress(client, message, percent);
    }));
    assert!(!client.error_loading);
    client.set_login_deadline(Instant::now() + Duration::from_secs(10));
    client.login(&user, "t2apass", false).unwrap();
    assert!(client.ingame);
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut scene_ticks = 0;
    loop {
        assert!(Instant::now() < deadline, "scene2 deadline");
        client.mainloop();
        assert!(
            client.ingame,
            "session ended: {:?}",
            client.take_session_exit_reason()
        );
        let frame = renderer.mainredraw(&mut client);
        if client.scene_state == 2 && client.local_player.as_ref().is_some_and(|p| p.ready) {
            scene_ticks += 1;
            if scene_ticks == 75 {
                let FrameOutput::PixMap(map) = frame else {
                    panic!("CPU proof required")
                };
                assert!(map.pixels.iter().any(|p| p & 0x00ff_ffff != 0));
                let rgb: Vec<u8> = map
                    .pixels
                    .iter()
                    .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
                    .collect();
                image::codecs::jpeg::JpegEncoder::new_with_quality(
                    std::fs::File::create(Path::new(&evidence).join("sound-enabled-scene2.jpg"))
                        .unwrap(),
                    95,
                )
                .encode(
                    &rgb,
                    map.width as u32,
                    map.height as u32,
                    image::ExtendedColorType::Rgb8,
                )
                .unwrap();
                let local = client.local_player.as_ref().unwrap();
                eprintln!(
                    "377 sound-enabled scene2: tile={},{} npcs={} sound_slots={}",
                    client.map_build_base_x + local.route_x[0],
                    client.map_build_base_z + local.route_z[0],
                    client.npc_count,
                    client.jagfx.synth.len()
                );
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(client.if_(2458).unwrap().client_code, 205);
    client.menu_action[0] = MiniMenuAction::IF_BUTTON;
    client.menu_param_c[0] = 2458;
    client.doAction(0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while client.ingame && Instant::now() < deadline {
        client.mainloop();
        drop(renderer.mainredraw(&mut client));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!client.ingame);
    let reason = client.take_session_exit_reason().unwrap();
    assert!(reason.to_string().contains("server_logout"));
    eprintln!("377 sound-enabled proof complete: {reason}");
}
