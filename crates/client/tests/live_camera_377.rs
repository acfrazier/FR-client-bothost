//! Opt-in Giant Dwarf intro: normal NPC/dialogue actions trigger server cameras.
//! Native camera reads are required to attest packet application independently
//! of gameplay. No camera packet is sent and no quest outcome is seeded.
use client::client::{CheatAdmission, Client, ClientConfig, ClientRevision, MiniMenuAction};
use client::render::backend::FrameOutput;
use client::render::Renderer;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

#[path = "support/live_engine_377.rs"]
mod live_engine_377;

fn tile(c: &Client) -> [i32; 2] {
    let p = c.local_player.as_ref().unwrap();
    [
        c.map_build_base_x + p.route_x[0],
        c.map_build_base_z + p.route_z[0],
    ]
}
fn state(c: &Client) -> Value {
    json!({"cycle":c.loop_cycle,"player_info":c.gens.player_info,"camera_generation":c.gens.camera,
        "scene":c.scene_state,"tile":tile(c),"plane":c.minusedlevel,
        "base":[c.map_build_base_x,c.map_build_base_z],"cinematic":c.cinema_cam,
        "eye":[c.cam_x,c.cam_y,c.cam_z,c.cam_pitch,c.cam_yaw],
        "move":[c.cam_move_to_lx,c.cam_move_to_lz,c.cam_move_to_hei,c.cam_move_to_rate,c.cam_move_to_rate2],
        "look":[c.cam_look_at_lx,c.cam_look_at_lz,c.cam_look_at_hei,c.cam_look_at_rate,c.cam_look_at_rate2],
        "shake":c.cam_shake,"shake_jitter":c.cam_shake_axis,"shake_amplitude":c.cam_shake_ran,
        "shake_speed":c.cam_shake_amp,"shake_cycle":c.cam_shake_cycle,
        "chat_modal":c.chat_modal_id,"overlay":c.main_overlay_id,
        "chat":c.chat_text.iter().take(3).collect::<Vec<_>>()})
}
fn step(c: &mut Client, r: &mut Renderer) {
    c.mainloop();
    assert!(
        c.ingame,
        "session ended: {:?}",
        c.take_session_exit_reason()
    );
    drop(r.mainredraw(c));
    std::thread::sleep(Duration::from_millis(20));
}
fn until(c: &mut Client, r: &mut Renderer, label: &str, ready: impl Fn(&Client) -> bool) {
    let end = Instant::now() + Duration::from_secs(45);
    loop {
        step(c, r);
        if ready(c) {
            return;
        }
        assert!(Instant::now() < end, "{label}: {}", state(c));
    }
}
fn is_intro_choice(button_type: i32, text: &str) -> bool {
    button_type == 1 && (text == "That's a deal!" || text.starts_with("Yes, I'm ready"))
}

#[test]
fn spoken_confirmation_is_not_a_choice_button() {
    for text in ["That's a deal!", "Yes, I'm ready and don't mind waiting."] {
        assert!(is_intro_choice(1, text));
        assert!(!is_intro_choice(0, text));
        assert!(!is_intro_choice(6, text));
    }
}

fn action(c: &mut Client, kind: i32, component: i32) {
    c.menu_action[0] = kind;
    c.menu_param_c[0] = component;
    c.doAction(0);
}
fn seed(
    c: &mut Client,
    r: &mut Renderer,
    log: &mut std::fs::File,
    command: &str,
    ready: impl Fn(&Client) -> bool,
) {
    writeln!(log, "{}", json!({"step":"SeedOwnAccount","why":"tutorial/position prerequisite only","command":command,"before":state(c)})).unwrap();
    c.set_cheat_admission(CheatAdmission::Granted);
    let sent = c.send_cheat(command).is_sent();
    c.set_cheat_admission(CheatAdmission::Denied);
    assert!(sent);
    until(c, r, command, ready);
    writeln!(
        log,
        "{}",
        json!({"step":"SeedObserved","command":command,"after":state(c)})
    )
    .unwrap();
}
fn capture(c: &mut Client, r: &mut Renderer, path: &Path) {
    let FrameOutput::PixMap(map) = r.mainredraw(c) else {
        panic!("CPU frame required")
    };
    let rgb: Vec<u8> = map
        .pixels
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
        .collect();
    image::codecs::jpeg::JpegEncoder::new_with_quality(std::fs::File::create(path).unwrap(), 95)
        .encode(
            &rgb,
            map.width as u32,
            map.height as u32,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
}
fn height(c: &Client, x: i32, z: i32) -> i32 {
    let (x, z) = (x as usize, z as usize);
    let mut level = c.minusedlevel as usize;
    if level < 3 && c.mapl[1][x][z] & 2 != 0 {
        level += 1;
    }
    // Java getHeightmapY's two-stage integer bilerp at the tile centre.
    let h = &c.groundh[level];
    (((h[x][z] + h[x + 1][z]) >> 1) + ((h[x][z + 1] + h[x + 1][z + 1]) >> 1)) >> 1
}
fn eye(c: &Client) -> [i32; 5] {
    [c.cam_x, c.cam_y, c.cam_z, c.cam_pitch, c.cam_yaw]
}

fn java_frame(
    previous: [i32; 5],
    source: [i32; 3],
    target: [i32; 3],
    movement: [i32; 2],
    rotation: [i32; 2],
) -> [i32; 5] {
    // Independent Java Client.java:4701-4792 calculation. Readiness and
    // unchanged packet targets are checked by the caller, not assumed.
    let approach = |value: i32, target: i32, rate: [i32; 2]| {
        if value < target {
            (value + rate[0] + (target - value) * rate[1] / 1000).min(target)
        } else if value > target {
            (value - rate[0] - (value - target) * rate[1] / 1000).max(target)
        } else {
            value
        }
    };
    let [x, y, z] = std::array::from_fn(|i| approach(previous[i], source[i], movement));
    let [dx, dy, dz] = [target[0] - x, target[1] - y, target[2] - z];
    let distance = ((dx * dx + dz * dz) as f64).sqrt() as i32;
    let pitch = (((dy as f64).atan2(distance as f64) * 325.949) as i32 & 0x7ff).clamp(128, 383);
    let target_yaw = ((dx as f64).atan2(dz as f64) * -325.949) as i32 & 0x7ff;
    let delta = |yaw| {
        let mut value = target_yaw - yaw;
        if value > 1024 {
            value -= 2048;
        }
        if value < -1024 {
            value += 2048;
        }
        value
    };
    let before = delta(previous[4]);
    let mut yaw = previous[4];
    if before > 0 {
        yaw = (yaw + rotation[0] + before * rotation[1] / 1000) & 0x7ff;
    } else if before < 0 {
        yaw = (yaw - rotation[0] - -before * rotation[1] / 1000) & 0x7ff;
    }
    let after = delta(yaw);
    if (before > 0 && after < 0) || (before < 0 && after > 0) {
        yaw = target_yaw;
    }
    [x, y, z, approach(previous[3], pitch, rotation), yaw]
}

#[test]
fn live_frame_oracle_matches_a_fixed_java_row() {
    assert_eq!(
        java_frame(
            [900, 100, 1000, 300, 2030],
            [576, -200, 704],
            [320, -50, 960],
            [10, 50],
            [5, 75],
        ),
        [874, 75, 976, 311, 27]
    );
}

fn expected_camera_frame(c: &Client, previous: [i32; 5]) -> [i32; 5] {
    assert_eq!([c.cam_move_to_rate2, c.cam_look_at_rate2], [100, 100]);
    java_frame(
        previous,
        [
            c.cam_move_to_lx * 128 + 64,
            height(c, c.cam_move_to_lx, c.cam_move_to_lz) - c.cam_move_to_hei,
            c.cam_move_to_lz * 128 + 64,
        ],
        [
            c.cam_look_at_lx * 128 + 64,
            height(c, c.cam_look_at_lx, c.cam_look_at_lz) - c.cam_look_at_hei,
            c.cam_look_at_lz * 128 + 64,
        ],
        [c.cam_move_to_rate, c.cam_move_to_rate2],
        [c.cam_look_at_rate, c.cam_look_at_rate2],
    )
}

#[test]
#[ignore = "requires the already-running 377 engine and a fresh frcm account"]
fn giant_dwarf_server_move_look_shake_and_reset() {
    let root = std::env::var("CLIENT_377_LIVE_DIR").expect("evidence directory required");
    let root = Path::new(&root);
    let cache =
        std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("runtime JAG directory required");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("fresh account required");
    assert!(user.starts_with("frcm") && user.len() <= 8 && user.len() > 4);
    let engine = live_engine_377::require_engine(root, &user, &cache);
    let password = "frcmtest";
    let mut c = Client::new_with_revision_and_http_port(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir: cache,
            members: true,
            lowmem: false,
        },
        ClientRevision::R377,
        81,
    );
    Renderer::set_prefer_gpu(false);
    c.draw = true;
    let mut r = Renderer::new(true);
    c.maininit_with_progress(Some(&mut |c, message, percent| {
        r.draw_progress(c, message, percent);
    }));
    assert!(!c.error_loading);
    c.set_login_deadline(Instant::now() + Duration::from_secs(15));
    c.login(&user, password, false).unwrap();
    until(&mut c, &mut r, "fresh tutorial scene", |c| {
        c.scene_state == 2 && c.local_player.as_ref().is_some_and(|p| p.ready)
    });
    let mut log = std::fs::File::create(root.join("live.jsonl")).unwrap();
    writeln!(
        log,
        "{}",
        json!({"step":"FreshLogin","user":user,"engine":engine,"state":state(&c)})
    )
    .unwrap();
    seed(&mut c, &mut r, &mut log, "tele 0,50,50,20,20", |c| {
        tile(c) == [3220, 3220] && c.scene_state == 2
    });
    seed(&mut c, &mut r, &mut log, "setvar tutorial 1000", |c| {
        c.chat_text
            .iter()
            .any(|text| text.as_str() == "set tutorial: to 1000")
    });
    action(&mut c, MiniMenuAction::IF_BUTTON, 2458);
    let end = Instant::now() + Duration::from_secs(15);
    while c.ingame && Instant::now() < end {
        c.mainloop();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!c.ingame, "prerequisite clean logout");
    c.set_login_deadline(Instant::now() + Duration::from_secs(15));
    c.login(&user, password, false).unwrap();
    until(&mut c, &mut r, "mainland login", |c| c.scene_state == 2);
    seed(&mut c, &mut r, &mut log, "tele 0,44,158,26,16", |c| {
        tile(c) == [2842, 10128] && c.scene_state == 2
    });
    until(&mut c, &mut r, "natural boatman spawn", |c| {
        c.npc_ids.iter().take(c.npc_count as usize).any(|&id| {
            c.npc[id as usize]
                .as_ref()
                .is_some_and(|n| n.r#type == Some(2205))
        })
    });
    let boatman = *c
        .npc_ids
        .iter()
        .take(c.npc_count as usize)
        .find(|&&id| {
            c.npc[id as usize]
                .as_ref()
                .is_some_and(|n| n.r#type == Some(2205))
        })
        .unwrap();
    writeln!(log,"{}",json!({"step":"TalkToBoatman","why":"normal content starts the intro from quest stage zero","npc_slot":boatman,"state":state(&c)})).unwrap();
    c.menu_action[0] = MiniMenuAction::OP_NPC1;
    c.menu_param_a[0] = boatman;
    c.menu_param_b[0] = 0;
    c.menu_param_c[0] = 0;
    c.doAction(0);
    let end = Instant::now() + Duration::from_secs(180);
    let mut next_dialogue = 0;
    let mut first_camera = false;
    let mut shake = false;
    let mut last_generation = c.gens.camera;
    let mut last_dialogue = None;
    let mut verified_frames = 0;
    loop {
        let before = (c.scene_state, c.cinema_cam, c.gens.camera, eye(&c));
        step(&mut c, &mut r);
        let mut row = state(&c);
        row["elapsed_ms"] =
            json!(180000 - end.saturating_duration_since(Instant::now()).as_millis());
        // Java:3328 runs interpolation only after scene readiness. Incoming
        // camera packets can precede the plane update and new terrain build.
        let comparable = c.cinema_cam
            && c.scene_state == 2
            && before.0 == 2
            && before.1
            && before.2 == c.gens.camera;
        let expected = comparable.then(|| expected_camera_frame(&c, before.3));
        row["java_expected_eye"] = json!(expected);
        writeln!(log, "{row}").unwrap();
        if let Some(expected) = expected {
            assert_eq!(eye(&c), expected, "Java ready-scene camera frame");
            verified_frames += 1;
            if !first_camera {
                first_camera = true;
                capture(&mut c, &mut r, &root.join("move-look.jpg"));
            }
        }
        if c.cam_shake[3] && !shake {
            assert_eq!(
                [c.cam_shake_axis[3], c.cam_shake_ran[3], c.cam_shake_amp[3]],
                [0, 20, 2]
            );
            assert!(c.cam_shake_cycle[3] > 0);
            shake = true;
            capture(&mut c, &mut r, &root.join("shake.jpg"));
        }
        if first_camera && shake && !c.cinema_cam && c.gens.camera > last_generation {
            assert_eq!(c.cam_shake, [false; 5]);
            capture(&mut c, &mut r, &root.join("reset.jpg"));
            break;
        }
        last_generation = c.gens.camera;
        if !c.cinema_cam && c.chat_modal_id >= 0 && c.loop_cycle >= next_dialogue {
            let fingerprint = (c.chat_modal_id, c.gens.iface);
            if last_dialogue != Some(fingerprint) && !c.resumed_pause_button {
                let components: Vec<_> = c
                    .ifaces_merged()
                    .filter(|i| i.layer_id == c.chat_modal_id)
                    .collect();
                let choice = components
                    .iter()
                    .find(|i| is_intro_choice(i.button_type, i.text));
                let button = choice
                    .map(|i| (MiniMenuAction::IF_BUTTON, i.id))
                    .or_else(|| {
                        components
                            .iter()
                            .find(|i| i.button_type == 6)
                            .map(|i| (MiniMenuAction::PAUSE_BUTTON, i.id))
                    });
                if let Some((kind, id)) = button {
                    writeln!(log,"{}",json!({"step":"DialogueButton","component":id,"action":kind,"text":components.iter().map(|i|i.text).collect::<Vec<_>>()})).unwrap();
                    action(&mut c, kind, id);
                    next_dialogue = c.loop_cycle + 25;
                    last_dialogue = Some(fingerprint);
                }
            }
        }
        assert!(
            Instant::now() < end,
            "camera content deadline: {}",
            state(&c)
        );
    }
    let receipt = json!({"verdict":"PASS","user":user,"content":"Giant Dwarf intro","move_look":first_camera,"java_frames_verified":verified_frames,"shake":shake,"reset":!c.cinema_cam,"final":state(&c),"limits":"runtime/state and CPU frames only; no blanket Java pixel equivalence"});
    std::fs::write(
        root.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    writeln!(log, "{receipt}").unwrap();
    eprintln!("{receipt}");
    c.logout();
}
