//! Opt-in public-chat proof against an orchestrator-started Fairy Ring 377 engine.
//! Two prepared, prefix-owned accounts must be nearby with public chat enabled.
//! Sender GameShell keys must reach the other account's server-populated player.
use client::client::{Client, ClientConfig, ClientRevision};
use client::dash3d::ClientPlayer;
use client::io::{ClientProt377, PacketObserver};
use client::render::backend::FrameOutput;
use client::render::Renderer;
use client::util::JString;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[path = "support/live_engine_377.rs"]
mod live_engine_377;

#[derive(Default)]
struct Outbound(Mutex<Vec<(u8, usize)>>);

impl PacketObserver for Outbound {
    fn packet_body_length(&self, opcode: u8) -> i16 {
        ClientProt377::ALL
            .iter()
            .find(|packet| packet.id == i32::from(opcode))
            .expect("only declared Java 377 packets may be emitted")
            .length as i16
    }

    fn packet_committed(&self, opcode: u8, encoded_length: usize) {
        self.0.lock().unwrap().push((opcode, encoded_length));
    }
}

impl Outbound {
    fn public_count(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(opcode, _)| i32::from(*opcode) == ClientProt377::MESSAGE_PUBLIC.id)
            .count()
    }
}

fn load(cache: &str) -> (Client, Renderer) {
    let mut client = Client::new_with_revision_and_http_port(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir: cache.into(),
            members: true,
            lowmem: true,
        },
        ClientRevision::R377,
        81,
    );
    client.draw = true;
    let mut renderer = Renderer::new(true);
    client.maininit_with_progress(Some(&mut |client, message, percent| {
        renderer.draw_progress(client, message, percent);
    }));
    assert!(!client.error_loading, "matched 377 runtime cache must load");
    (client, renderer)
}

fn remote_player(client: &Client, hash: u64) -> Option<(usize, &ClientPlayer)> {
    client
        .player_ids
        .iter()
        .take(client.player_count as usize)
        .find_map(|&slot| {
            let player = client.players.get(slot as usize)?.as_deref()?;
            player
                .name
                .as_ref()
                .is_some_and(|name| JString::to_userhash(name) == hash)
                .then_some((slot as usize, player))
        })
}

fn received(client: &Client, hash: u64, text: &str, colour: i32, effect: i32) -> bool {
    remote_player(client, hash).is_some_and(|(_, player)| {
        player.chat_message.as_deref() == Some(text)
            && player.chat_colour == colour
            && player.chat_effect == effect
            && player.chat_timer > 0
    })
}

fn state(client: &Client) -> Value {
    json!({
        "player_info_generation": client.gens.player_info,
        "scene_state": client.scene_state,
        "local_name": client.local_player.as_ref().and_then(|player| player.name.as_deref()),
        "remote_players": client.player_ids.iter().take(client.player_count as usize).filter_map(|&slot| {
            let player = client.players.get(slot as usize)?.as_deref()?;
            Some(json!({"slot":slot,"name":player.name,"text":player.chat_message,
                "colour":player.chat_colour,"effect":player.chat_effect,"timer":player.chat_timer,
                "tile":[client.map_build_base_x+player.route_x[0],client.map_build_base_z+player.route_z[0]]}))
        }).collect::<Vec<_>>()
    })
}

fn screenshot(path: &Path, frame: FrameOutput) {
    let FrameOutput::PixMap(map) = frame else {
        panic!("CPU proof required");
    };
    let pixels: Vec<u8> = map
        .pixels
        .iter()
        .flat_map(|&pixel| [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8])
        .collect();
    image::codecs::jpeg::JpegEncoder::new_with_quality(std::fs::File::create(path).unwrap(), 95)
        .encode(
            &pixels,
            map.width as u32,
            map.height as u32,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
}

fn pump_pair(
    sender: &mut Client,
    sender_renderer: &mut Renderer,
    observer: &mut Client,
    observer_renderer: &mut Renderer,
    root: &Path,
    timeout: Duration,
    mut ready: impl FnMut(&Client, &Client) -> bool,
) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        sender.shell.latch_click();
        sender.mainloop();
        assert!(
            sender.ingame,
            "sender ended: {:?}",
            sender.take_session_exit_reason()
        );
        drop(sender_renderer.mainredraw(sender));
        observer.shell.latch_click();
        observer.mainloop();
        assert!(
            observer.ingame,
            "observer ended: {:?}",
            observer.take_session_exit_reason()
        );
        drop(observer_renderer.mainredraw(observer));
        if ready(sender, observer) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let failure = json!({"sender":state(sender),"observer":state(observer)});
    std::fs::write(
        root.join("deadline.json"),
        serde_json::to_vec_pretty(&failure).unwrap(),
    )
    .unwrap();
    screenshot(
        &root.join("deadline-sender.jpg"),
        sender_renderer.mainredraw(sender),
    );
    screenshot(
        &root.join("deadline-observer.jpg"),
        observer_renderer.mainredraw(observer),
    );
    panic!("server-delivery deadline: {failure}");
}

#[test]
#[ignore = "requires two prepared accounts and an orchestrator-started Fairy Ring 377 engine on localhost:81/43595"]
fn public_chat_colour_and_effect_reach_other_player() {
    let root = PathBuf::from(
        std::env::var("CLIENT_377_LIVE_DIR").expect("explicit new evidence directory required"),
    );
    assert!(root.is_absolute(), "evidence directory must be absolute");
    let cache =
        std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("matched running 377 cache required");
    let sender_user =
        std::env::var("CLIENT_377_LIVE_USER").expect("explicit prepared sender required");
    let sender_password =
        std::env::var("CLIENT_377_LIVE_PASSWORD").expect("explicit sender password required");
    let observer_user =
        std::env::var("CLIENT_377_LIVE_OBSERVER").expect("explicit prepared observer required");
    let observer_password = std::env::var("CLIENT_377_LIVE_OBSERVER_PASSWORD")
        .expect("explicit observer password required");
    let sender_hash = JString::to_userhash(&sender_user);
    let observer_hash = JString::to_userhash(&observer_user);
    assert_ne!(
        sender_hash, observer_hash,
        "proof requires distinct accounts"
    );
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists(), "quiet lock blocks live runs");
    }
    std::fs::create_dir(&root).expect("evidence directory must be new and its parent must exist");
    live_engine_377::require_engine(&root.join("sender"), &sender_user, &cache);
    live_engine_377::require_engine(&root.join("observer"), &observer_user, &cache);
    Renderer::set_prefer_gpu(false);
    // Finish both cache initializations before either session starts its watchdog.
    let (mut sender, mut sender_renderer) = load(&cache);
    let (mut observer, mut observer_renderer) = load(&cache);
    sender.set_login_deadline(Instant::now() + Duration::from_secs(10));
    sender.login(&sender_user, &sender_password, false).unwrap();
    observer.set_login_deadline(Instant::now() + Duration::from_secs(10));
    observer
        .login(&observer_user, &observer_password, false)
        .unwrap();
    let outbound = Arc::new(Outbound::default());
    sender.out.set_observer(Some(outbound.clone()));
    pump_pair(
        &mut sender,
        &mut sender_renderer,
        &mut observer,
        &mut observer_renderer,
        &root,
        Duration::from_secs(90),
        |sender, observer| {
            sender.scene_state == 2
                && observer.scene_state == 2
                && remote_player(sender, observer_hash).is_some()
                && remote_player(observer, sender_hash).is_some()
        },
    );
    for client in [&sender, &observer] {
        assert_eq!(
            client.chat_public_mode, 0,
            "prepare public-chat-on accounts"
        );
        assert_eq!(
            client.main_modal_id, -1,
            "close prepared account interfaces"
        );
        assert_eq!(client.chat_modal_id, -1, "close prepared account dialogues");
        assert!(!client.dialog_input_open && !client.social_input_open);
    }
    let mut deliveries = Vec::new();
    for (index, (input, text, colour, effect)) in [
        ("ReD:WaVe:Protocol one", "Protocol one", 1, 1),
        ("red:wave2:Protocol two", "Protocol two", 1, 2),
        ("green:shake:Protocol three", "Protocol three", 2, 3),
        ("cyan:scroll:Protocol four", "Protocol four", 3, 4),
        ("white:slide:Protocol five", "Protocol five", 5, 5),
        ("yellow:Protocol six", "Protocol six", 0, 0),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(sender.chat_input.is_empty());
        assert!(
            !received(&observer, sender_hash, text, colour, effect),
            "stale matching bubble cannot prove delivery"
        );
        for character in input.bytes() {
            sender.shell.apply_key(true, 0, i32::from(character));
        }
        pump_pair(
            &mut sender,
            &mut sender_renderer,
            &mut observer,
            &mut observer_renderer,
            &root,
            Duration::from_secs(5),
            |sender, _| sender.chat_input == input,
        );
        let generation = observer.gens.player_info;
        let packets = outbound.public_count();
        sender.shell.apply_key(true, 0, 13);
        pump_pair(
            &mut sender,
            &mut sender_renderer,
            &mut observer,
            &mut observer_renderer,
            &root,
            Duration::from_secs(10),
            |_, observer| {
                observer.gens.player_info > generation
                    && received(observer, sender_hash, text, colour, effect)
            },
        );
        assert_eq!(
            outbound.public_count(),
            packets + 1,
            "one actual public-chat packet per Enter"
        );
        deliveries.push(json!({"input":input,"expected_text":text,"colour":colour,"effect":effect,
            "observer_generation_before":generation,"observer":state(&observer),
            "outbound_packets":*outbound.0.lock().unwrap(),
            "proof":"other account remote-player state from a later server player-info update; not sender echo"}));
        std::fs::write(
            root.join("deliveries.json"),
            serde_json::to_vec_pretty(&deliveries).unwrap(),
        )
        .unwrap();
        screenshot(
            &root.join(format!("observer-{index}.jpg")),
            observer_renderer.mainredraw(&mut observer),
        );
        let sender_generation = sender.gens.player_info;
        let observer_generation = observer.gens.player_info;
        pump_pair(
            &mut sender,
            &mut sender_renderer,
            &mut observer,
            &mut observer_renderer,
            &root,
            Duration::from_secs(5),
            |sender, observer| {
                sender.gens.player_info > sender_generation
                    && observer.gens.player_info > observer_generation
            },
        );
    }
    assert_eq!(deliveries.len(), 6);
}
