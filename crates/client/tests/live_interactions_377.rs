//! Explicit live UI proof against the already-running Fairy Ring 377 engine.
//! Movement comes from a rendered terrain click, never from an assigned route.
use client::client::{Client, ClientConfig, ClientRevision, MiniMenuAction};
use client::io::{ClientProt377, PacketObserver};
use client::render::backend::FrameOutput;
use client::render::Renderer;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[path = "support/live_engine_377.rs"]
mod live_engine_377;

#[derive(Default)]
struct Outbound(Mutex<Vec<Value>>);
impl PacketObserver for Outbound {
    fn packet_body_length(&self, opcode: u8) -> i16 {
        ClientProt377::ALL
            .iter()
            .find(|p| p.id == i32::from(opcode))
            .expect("only declared Java 377 packets may be emitted")
            .length as i16
    }
    fn packet_committed(&self, opcode: u8, encoded_length: usize) {
        self.0
            .lock()
            .unwrap()
            .push(json!({"opcode":opcode,"encoded_length":encoded_length}));
    }
    fn observed_tick(&self, tick: u64, session: u64) {
        self.0
            .lock()
            .unwrap()
            .push(json!({"player_info":tick,"session":session}));
    }
}

fn screenshot(path: &Path, output: FrameOutput) {
    let FrameOutput::PixMap(map) = output else {
        panic!("CPU proof required")
    };
    let rgb: Vec<u8> = map
        .pixels
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    image::codecs::jpeg::JpegEncoder::new_with_quality(std::fs::File::create(path).unwrap(), 95)
        .encode(
            &rgb,
            map.width as u32,
            map.height as u32,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    eprintln!("377 interaction screenshot {}", path.display());
}

fn tile(c: &Client) -> (i32, i32) {
    let p = c.local_player.as_ref().unwrap();
    (
        c.map_build_base_x + p.route_x[0],
        c.map_build_base_z + p.route_z[0],
    )
}

fn body(c: &Client) -> String {
    c.ifaces_merged()
        .filter(|i| i.layer_id == c.chat_modal_id && i.button_type == 0 && !i.text.is_empty())
        .map(|i| i.text.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn state(c: &Client) -> Value {
    let p = c.local_player.as_ref().unwrap();
    json!({"cycle":c.loop_cycle,"tile":tile(c),"fine_position":[p.x,p.z],
        "player_info_generation":c.gens.player_info,"varp_generation":c.gens.varp,
        "main_modal":c.main_modal_id,"chat_modal":c.chat_modal_id,"overlay":c.main_overlay_id,
        "body":body(c),"progress_varp_406":c.var.get(406),"progress_server_406":c.var_serv.get(406)})
}

fn pump_until(
    c: &mut Client,
    r: &mut Renderer,
    seconds: u64,
    mut ready: impl FnMut(&Client) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        c.shell.latch_click();
        c.mainloop();
        assert!(
            c.ingame,
            "live 377 session ended: {:?}",
            c.take_session_exit_reason()
        );
        drop(r.mainredraw(c));
        if ready(c) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("live interaction deadline: {}", state(c));
}

fn click(c: &mut Client, r: &mut Renderer, x: i32, y: i32, button: i32) {
    c.shell.apply_mouse_move(x, y);
    drop(r.mainredraw(c));
    c.shell.apply_mouse_down(button, x, y);
    // Match Client::run: promote window events before the machine pass.
    c.shell.latch_click();
    c.mainloop();
    assert!(
        c.ingame,
        "click ended session: {:?}",
        c.take_session_exit_reason()
    );
    drop(r.mainredraw(c));
    c.shell.apply_mouse_up();
    c.shell.latch_click();
    c.mainloop();
    assert!(
        c.ingame,
        "click release ended session: {:?}",
        c.take_session_exit_reason()
    );
    drop(r.mainredraw(c));
}

fn component_rect(
    c: &Client,
    root: i32,
    target: i32,
    x: i32,
    y: i32,
) -> Option<(i32, i32, i32, i32)> {
    let com = c.if_(root as usize)?;
    if root == target {
        return Some((x, y, com.width, com.height));
    }
    let children = com.children.as_ref()?;
    let child_x = com.child_x.as_ref()?;
    let child_y = com.child_y.as_ref()?;
    for (index, &id) in children.iter().enumerate() {
        let child = c.if_(id as usize)?;
        if let Some(rect) = component_rect(
            c,
            id,
            target,
            x + child_x[index] + child.x,
            y + child_y[index] + child.y - com.scroll_pos,
        ) {
            return Some(rect);
        }
    }
    None
}

fn click_component(c: &mut Client, r: &mut Renderer, root: i32, target: i32, origin: (i32, i32)) {
    let (x, y, w, h) = component_rect(c, root, target, origin.0, origin.1)
        .expect("live button belongs to its open interface");
    assert!(w > 0 && h > 0);
    eprintln!(
        "377 click component={target} pixel={},{} rect={x},{y},{w},{h}",
        x + w / 2,
        y + h / 2
    );
    c.shell.apply_mouse_move(x + w / 2, y + h / 2);
    drop(r.mainredraw(c));
    assert!(
        (0..c.menu_num_entries as usize).any(|i| c.menu_param_c[i] == target),
        "rendered button must enter real minimenu"
    );
    click(c, r, x + w / 2, y + h / 2, 1);
}

fn save_stage(c: &mut Client, r: &mut Renderer, root: &Path, name: &str, stages: &mut Vec<Value>) {
    let path = root.join(format!("rust-{name}.jpg"));
    screenshot(&path, r.mainredraw(c));
    let receipt = json!({"stage":name,"screenshot":path,"state":state(c)});
    eprintln!("377 interaction {receipt}");
    stages.push(receipt);
}

#[test]
#[ignore = "requires orchestrator-started Fairy Ring 377 engine on localhost:81/43595"]
fn terrain_click_guide_dialogue_and_progress() {
    let root = PathBuf::from(
        std::env::var("CLIENT_377_LIVE_DIR").expect("explicit evidence directory required"),
    );
    let cache =
        std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("matched running cache required");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("explicit t2a account required");
    assert!(user.starts_with("t2a"));
    assert!(!Path::new("/Volumes/dev-scratch/274bot-evidence/MAC-QUIET.lock").exists());
    let engine = live_engine_377::require_engine(&root, &user, &cache);
    Renderer::set_prefer_gpu(false);
    let mut c = Client::new_with_revision_and_http_port(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir: cache,
            members: true,
            lowmem: true,
        },
        ClientRevision::R377,
        81,
    );
    c.draw = true;
    let mut r = Renderer::new(true);
    c.maininit_with_progress(Some(&mut |c, message, percent| {
        r.draw_progress(c, message, percent);
    }));
    assert!(!c.error_loading);
    let outbound = Arc::new(Outbound::default());
    c.out.set_observer(Some(outbound.clone()));
    c.set_login_deadline(Instant::now() + Duration::from_secs(10));
    c.login(&user, "t2apass", false).unwrap();
    pump_until(&mut c, &mut r, 90, |c| {
        c.scene_state == 2
            && c.npc_count > 0
            && c.main_modal_id == 3559
            && c.var.get(406) == Some(&1)
    });
    let mut stages = Vec::new();
    save_stage(&mut c, &mut r, &root, "design", &mut stages);
    let design = c.main_modal_id;
    click_component(&mut c, &mut r, design, 3651, (4, 4));
    pump_until(&mut c, &mut r, 10, |c| c.main_modal_id == -1);
    let before = tile(&c);
    let tick = c.gens.player_info;
    let local = c.local_player.as_ref().unwrap();
    let (target_x, target_z) = (local.route_x[0] - 3, local.route_z[0]);
    let fine_target = (target_x * 128 + 64, target_z * 128 + 64);
    r.get_overlay_pos(&mut c, fine_target.0, fine_target.1, 0);
    let (x, y) = (r.project_x + 4, r.project_y + 4);
    assert!(
        (4..516).contains(&x) && (4..338).contains(&y),
        "terrain target projects inside the game viewport: {x},{y}"
    );
    c.shell.apply_mouse_move(x, y);
    drop(r.mainredraw(&mut c));
    let walk = (0..c.menu_num_entries as usize)
        .find(|&i| c.menu_action[i] == MiniMenuAction::WALK)
        .expect("real ground menu entry");
    if walk + 1 == c.menu_num_entries as usize {
        click(&mut c, &mut r, x, y, 1);
    } else {
        click(&mut c, &mut r, x, y, 2);
        assert!(c.is_menu_open);
        assert_eq!(c.menu_area, 0);
        let row_y = c.menu_y + (c.menu_num_entries - 1 - walk as i32) * 15 + 31 + 4;
        let row_x = c.menu_x + 9;
        click(&mut c, &mut r, row_x, row_y, 1);
    }
    pump_until(&mut c, &mut r, 10, |c| {
        tile(c) == (before.0 - 3, before.1)
            && c.gens.player_info > tick
            && c.local_player
                .as_ref()
                .is_some_and(|p| (p.x, p.z) == fine_target)
    });
    let after = tile(&c);
    assert_eq!(after, (before.0 - 3, before.1));
    save_stage(&mut c, &mut r, &root, "walk", &mut stages);
    let guide = c
        .npc_ids
        .iter()
        .take(c.npc_count as usize)
        .copied()
        .find(|&slot| {
            c.npc
                .get(slot as usize)
                .and_then(|n| n.as_ref())
                .is_some_and(|n| n.r#type == Some(945))
        })
        .expect("server-populated RuneScape Guide");
    c.menu_action[0] = MiniMenuAction::OP_NPC1;
    c.menu_param_a[0] = guide;
    c.menu_param_b[0] = 0;
    c.menu_param_c[0] = 0;
    c.doAction(0);
    pump_until(&mut c, &mut r, 10, |c| {
        c.chat_modal_id >= 0 && !body(c).is_empty() && !c.resumed_pause_button
    });
    let mut pages = 0;
    loop {
        let page = body(&c);
        if c.chat_modal_id == 2459 {
            save_stage(&mut c, &mut r, &root, "dialogue-no-skip", &mut stages);
            let root_id = c.chat_modal_id;
            click_component(&mut c, &mut r, root_id, 2462, (17, 357));
            pump_until(&mut c, &mut r, 10, |c| {
                c.chat_modal_id != 2459
                    && c.chat_modal_id >= 0
                    && !c.resumed_pause_button
                    && !body(c).is_empty()
            });
            continue;
        }
        assert!(!page.is_empty());
        save_stage(
            &mut c,
            &mut r,
            &root,
            &format!("dialogue-{pages}"),
            &mut stages,
        );
        pages += 1;
        let final_page = page.contains("To continue the tutorial go");
        let root_id = c.chat_modal_id;
        let resume = c
            .ifaces_merged()
            .find(|i| i.layer_id == root_id && i.button_type == 6)
            .map(|i| i.id)
            .expect("real continue button");
        click_component(&mut c, &mut r, root_id, resume, (17, 357));
        if final_page {
            pump_until(&mut c, &mut r, 10, |c| c.chat_modal_id == -1);
            break;
        }
        pump_until(&mut c, &mut r, 10, |c| {
            c.chat_modal_id == 2459
                || c.chat_modal_id >= 0
                    && !c.resumed_pause_button
                    && !body(c).is_empty()
                    && body(c) != page
        });
        assert!(pages <= 8, "dialogue failed to advance");
    }
    assert!(pages >= 6, "skip prompt plus five welcome pages required");
    assert_eq!(c.var.get(406), Some(&1));
    assert_eq!(c.var_serv.get(406), Some(&1));
    assert_eq!(c.main_overlay_id, 8680);
    let segments:Vec<Value>=c.ifaces_merged().filter(|i|i.layer_id==8680 && i.scripts.as_ref().is_some_and(|ss|ss.iter().any(|s|s.as_slice()==[5,406,0])))
        .map(|i|json!({"component":i.id,"script_value":Renderer::get_if_var(&c,i.id,0),"threshold":i.script_operand.as_ref().and_then(|v|v.first())})).collect();
    assert_eq!(
        segments.len(),
        20,
        "all twenty real progress segments read varp406"
    );
    assert!(segments.iter().all(|s| s["script_value"] == 1));
    assert!(body(&c).is_empty());
    save_stage(&mut c, &mut r, &root, "progress", &mut stages);
    c.menu_action[0] = MiniMenuAction::IF_BUTTON;
    c.menu_param_c[0] = 2458;
    c.doAction(0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while c.ingame && Instant::now() < deadline {
        c.shell.latch_click();
        c.mainloop();
        drop(r.mainredraw(&mut c));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!c.ingame, "engine must acknowledge logout");
    let exit = format!("{}", c.take_session_exit_reason().unwrap());
    assert!(exit.contains("server_logout"));
    let packets = outbound.0.lock().unwrap();
    for opcode in [6, 19, 28, 79, 112, 163, 226, 247, 248] {
        assert!(
            packets.iter().any(|p| p["opcode"] == opcode),
            "missing required opcode{opcode}: {packets:?}"
        );
    }
    assert!(packets.iter().filter(|p| p["opcode"] == 226).count() >= 6);
    let receipt = json!({"engine_dir":engine,"user":user,"revision":377,"movement":{"before":before,"after":after,"click":[x,y],"initial_player_info":tick,"fine_destination":fine_target},
        "guide_slot":guide,"dialogue_pages":pages,"progress":{"varp":406,"value":1,"overlay":8680,"segments":segments,
        "meaning":"1 + floor(tutorial * 20 / 670); stage 4 still has no filled segments; tutorial varp281 is not transmitted"},
        "stages":stages,"outbound":*packets,"logout":exit});
    std::fs::write(
        root.join("rust-interactions.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("PASS: real terrain click, authoritative movement, guide, {pages} dialogue pages, varp406 progress, server logout");
}
