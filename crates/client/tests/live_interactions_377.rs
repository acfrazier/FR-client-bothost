//! Explicit live UI proof against the already-running Fairy Ring 377 engine.
//! Movement comes from rendered terrain/minimap clicks, never an assigned route.
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

fn tutorial_text(c: &Client) -> String {
    c.ifaces_merged()
        .filter(|i| i.layer_id == c.tut_com_id && !i.text.is_empty())
        .map(|i| i.text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn state(c: &Client) -> Value {
    let p = c.local_player.as_ref().unwrap();
    let mut projectiles = Vec::new();
    c.projectiles.for_each(|p| {
        projectiles.push(json!({"spotanim":p.spotanim,
        "source":[p.src_x,p.h1,p.src_z],"position":[p.x,p.y,p.z],
        "start_cycle":p.t1,"end_cycle":p.t2,"target":p.target}))
    });
    json!({"cycle":c.loop_cycle,"tile":tile(c),"fine_position":[p.x,p.z],
        "player_info_generation":c.gens.player_info,"varp_generation":c.gens.varp,
        "main_modal":c.main_modal_id,"chat_modal":c.chat_modal_id,"overlay":c.main_overlay_id,
        "body":body(c),"progress_varp_406":c.var.get(406),"progress_server_406":c.var_serv.get(406),
        "hint":{"kind":c.hint_type,"npc":c.hint_npc,"tile":[c.hint_tile_x,c.hint_tile_z],"flash_on":c.loop_cycle % 20 < 10},
        "active_tab":c.active_icon,"flashing_tab":c.tut_flash_icon,
        "map_flag_generation":c.gens.map_flag,"minimap_flag":[c.minimap_flag_x,c.minimap_flag_z],
        "instructors":c.npc_ids.iter().take(c.npc_count as usize).filter_map(|id| {
            let npc = c.npc[*id as usize].as_ref()?;
            matches!(npc.r#type, Some(942 | 943 | 952)).then(|| json!({"slot":id,"type":npc.r#type,
                "tile":[npc.route_x[0]+c.map_build_base_x,npc.route_z[0]+c.map_build_base_z]}))
        }).collect::<Vec<_>>(),
        "bronze_hatchets":inventory_count(c,1351),"tinderboxes":inventory_count(c,590),"tutorial_logs":inventory_count(c,2511),
        "tutorial_text":tutorial_text(c),"progress_segments":progress_segments(c),
        "inventory":c.if_(3214).map(|i|json!({"objects":i.link_obj_type,"counts":i.link_obj_number})),
        "side_modal":c.side_modal_id,
        "inventories":c.ifaces_merged().filter(|i|i.link_obj_type.is_some()).map(|i|
            json!({"component":i.id,"layer":i.layer_id,"objects":i.link_obj_type,"counts":i.link_obj_number})).collect::<Vec<_>>(),
        "npcs":c.npc_ids.iter().take(c.npc_count as usize).filter_map(|id| {
            let npc=c.npc[*id as usize].as_ref()?;
            Some(json!({"slot":id,"type":npc.r#type,"tile":[npc.route_x[0]+c.map_build_base_x,npc.route_z[0]+c.map_build_base_z],
                "health":npc.health,"total_health":npc.total_health,"combat_cycle":npc.combat_cycle}))
        }).collect::<Vec<_>>(),
        "projectiles":projectiles,
        "chat":c.chat_text.iter().zip(&c.chat_type).take(8).map(|(text,kind)|json!({"text":text,"kind":kind})).collect::<Vec<_>>(),
        "raw_shrimps":inventory_count(c,2514),"burnt_shrimps":inventory_count(c,323),
        "cooked_shrimps":inventory_count(c,315),"flour":inventory_count(c,2516),
        "water":inventory_count(c,1929),"dough":inventory_count(c,2307),"bread":inventory_count(c,2309),
        "stat_xp":c.stat_xp,"stat_levels":c.stat_base_level})
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
    // The packet pump is intentionally bounded per frame. A text predicate
    // can match before the following TUT_OPEN/IF_SET* messages are consumed.
    // Capture and take the next action only after two complete server epochs.
    let generation = c.gens.player_info;
    pump_until(c, r, 10, |c| c.gens.player_info >= generation + 2);
    capture_stage(c, r, root, name, stages);
}

fn capture_stage(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    name: &str,
    stages: &mut Vec<Value>,
) {
    let path = root.join(format!("rust-{name}.jpg"));
    screenshot(&path, r.mainredraw(c));
    let receipt = json!({"stage":name,"screenshot":path,"state":state(c)});
    eprintln!("377 interaction {receipt}");
    stages.push(receipt);
    std::fs::write(
        root.join("stages.json"),
        serde_json::to_vec_pretty(stages).unwrap(),
    )
    .unwrap();
}

fn inventory_count(c: &Client, object: i32) -> i32 {
    let Some(inv) = c.if_(3214) else {
        return 0;
    };
    match (&inv.link_obj_type, &inv.link_obj_number) {
        (Some(types), Some(counts)) => types
            .iter()
            .zip(counts)
            .filter(|(id, _)| **id == object + 1)
            .map(|(_, count)| *count)
            .sum(),
        _ => 0,
    }
}

fn progress_segments(c: &Client) -> Vec<Value> {
    c.ifaces_merged()
        .filter(|i| i.layer_id == 8680 && i.scripts.as_ref().is_some_and(|scripts|
            scripts.iter().any(|script| script.as_slice() == [5, 406, 0])))
        .map(|i| {
            assert_eq!(i.script_comparator.as_deref(), Some(&[3][..]));
            let value = Renderer::get_if_var(c, i.id, 0).expect("actual varp406 script evaluates");
            let threshold = i.script_operand.as_ref().unwrap()[0];
            json!({"component":i.id,"script_value":value,"threshold":threshold,"filled":value > threshold})
        }).collect()
}

fn continue_chat(c: &mut Client, r: &mut Renderer) {
    let root = c.chat_modal_id;
    let button = c
        .ifaces_merged()
        .find(|i| i.layer_id == root && i.button_type == 6)
        .expect("server-opened dialogue has a real continue button")
        .id;
    click_component(c, r, root, button, (17, 357));
}

fn talk_to(c: &mut Client, npc_type: usize) -> i32 {
    let slot = c
        .npc_ids
        .iter()
        .take(c.npc_count as usize)
        .copied()
        .find(|slot| {
            c.npc[*slot as usize]
                .as_ref()
                .is_some_and(|npc| npc.r#type == Some(npc_type))
        })
        .expect("server-populated tutorial instructor");
    let npc = c.npc[slot as usize].as_ref().unwrap();
    eprintln!(
        "377 npc action {}",
        json!({"slot":slot,"type":npc_type,"player":tile(c),
        "npc_tile":[npc.route_x[0]+c.map_build_base_x,npc.route_z[0]+c.map_build_base_z],
        "input":"production doAction(OP_NPC1) on the actual server-populated NPC slot"})
    );
    c.menu_action[0] = MiniMenuAction::OP_NPC1;
    c.menu_param_a[0] = slot;
    c.menu_param_b[0] = 0;
    c.menu_param_c[0] = 0;
    c.doAction(0);
    slot
}

fn location_typecode(c: &Client, x: i32, z: i32, first_option: &str) -> Option<i32> {
    let (x, z) = (x - c.map_build_base_x, z - c.map_build_base_z);
    if !(0..104).contains(&x) || !(0..104).contains(&z) {
        return None;
    }
    [
        c.world.wall_type(c.minusedlevel, x, z),
        c.world.decor_type(c.minusedlevel, x, z),
        c.world.scene_type(c.minusedlevel, x, z),
        c.world.gd_type(c.minusedlevel, x, z),
    ]
    .into_iter()
    .find(|code| {
        *code != 0
            && c.cache
                .locs
                .get(((code >> 14) & 0x7fff) as usize)
                .is_some_and(|loc| loc.op.first().and_then(Option::as_deref) == Some(first_option))
    })
    .or_else(|| {
        // The tutorial's 2x2 trees can be hinted on a non-anchor tile.
        // Only select a real scene location whose footprint contains it.
        for (dx, dz) in [(1, 0), (0, 1), (1, 1)] {
            let (anchor_x, anchor_z) = (x - dx, z - dz);
            if anchor_x < 0 || anchor_z < 0 {
                continue;
            }
            let Some(scene) = c.world.get_scene(c.minusedlevel, anchor_x, anchor_z) else {
                continue;
            };
            if scene.max_tile_x >= x
                && scene.max_tile_z >= z
                && c.cache.locs[((scene.typecode >> 14) & 0x7fff) as usize]
                    .op
                    .first()
                    .and_then(Option::as_deref)
                    == Some(first_option)
            {
                return Some(scene.typecode);
            }
        }
        None
    })
}

fn operate_location(c: &mut Client, x: i32, z: i32, first_option: &str) -> Value {
    let code = location_typecode(c, x, z, first_option)
        .expect("hint targets an actual server-loaded location with the requested first option");
    let location = (code >> 14) & 0x7fff;
    let target_x = c.map_build_base_x + (code & 0x7f);
    let target_z = c.map_build_base_z + ((code >> 7) & 0x7f);
    let receipt = json!({"hint_tile":[x,z],"tile":[target_x,target_z],"location":location,"typecode":code,
        "name":c.cache.locs[location as usize].name,"option":first_option,
        "input":"production doAction(OP_LOC1) on actual scene typecode"});
    c.menu_action[0] = MiniMenuAction::OP_LOC1;
    c.menu_param_a[0] = code;
    c.menu_param_b[0] = target_x - c.map_build_base_x;
    c.menu_param_c[0] = target_z - c.map_build_base_z;
    c.doAction(0);
    eprintln!("377 location action {receipt}");
    receipt
}

fn inventory_slot(c: &Client, object: i32) -> i32 {
    c.if_(3214)
        .and_then(|i| i.link_obj_type.as_ref())
        .and_then(|objects| objects.iter().position(|id| *id == object + 1))
        .expect("item must exist in the server-applied inventory") as i32
}

fn select_item(c: &mut Client, object: i32) -> Value {
    let slot = inventory_slot(c, object);
    c.menu_action[0] = MiniMenuAction::USEHELD_START;
    c.menu_param_a[0] = object;
    c.menu_param_b[0] = slot;
    c.menu_param_c[0] = 3214;
    c.doAction(0);
    assert_eq!(c.use_mode, 1);
    json!({"object":object,"slot":slot,"component":3214,"name":c.cache.objs[object as usize].name})
}

fn use_items(c: &mut Client, source: i32, target: i32) -> Value {
    let source = select_item(c, source);
    let slot = inventory_slot(c, target);
    let receipt = json!({"source":source,"target":{"object":target,"slot":slot,"component":3214},
        "input":"production doAction(USEHELD_START), doAction(USEHELD_ONHELD)"});
    c.menu_action[0] = MiniMenuAction::USEHELD_ONHELD;
    c.menu_param_a[0] = target;
    c.menu_param_b[0] = slot;
    c.menu_param_c[0] = 3214;
    c.doAction(0);
    assert_eq!(c.use_mode, 0);
    eprintln!("377 item action {receipt}");
    receipt
}

fn scene_location(c: &Client, location: i32) -> Option<(i32, i32, i32)> {
    let player = tile(c);
    (0..104)
        .flat_map(|x| (0..104).map(move |z| (x, z)))
        .filter_map(|(x, z)| {
            [
                c.world.wall_type(c.minusedlevel, x, z),
                c.world.decor_type(c.minusedlevel, x, z),
                c.world.scene_type(c.minusedlevel, x, z),
                c.world.gd_type(c.minusedlevel, x, z),
            ]
            .into_iter()
            .find(|code| *code != 0 && ((*code >> 14) & 0x7fff) == location)
            .map(|code| (code, x + c.map_build_base_x, z + c.map_build_base_z))
        })
        .min_by_key(|(_, x, z)| (x - player.0).abs() + (z - player.1).abs())
}

fn use_item_on_location(c: &mut Client, object: i32, location: i32) -> Value {
    let (code, x, z) = scene_location(c, location).expect("actual server-loaded scene location");
    let source = select_item(c, object);
    let receipt = json!({"source":source,"location":location,"typecode":code,"tile":[x,z],
        "name":c.cache.locs[location as usize].name,
        "input":"production doAction(USEHELD_START), doAction(USEHELD_ONLOC) on actual scene typecode"});
    c.menu_action[0] = MiniMenuAction::USEHELD_ONLOC;
    c.menu_param_a[0] = code;
    c.menu_param_b[0] = x - c.map_build_base_x;
    c.menu_param_c[0] = z - c.map_build_base_z;
    c.doAction(0);
    assert_eq!(c.use_mode, 0);
    eprintln!("377 item-location action {receipt}");
    receipt
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
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists(), "quiet lock blocks live runs");
    }
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
    pump_until(&mut c, &mut r, 10, |c| {
        c.hint_type == 1 && c.loop_cycle % 20 < 10
    });
    save_stage(&mut c, &mut r, &root, "guide-hint", &mut stages);
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
    let segments = progress_segments(&c);
    assert_eq!(
        segments.len(),
        20,
        "all twenty real progress segments read varp406"
    );
    assert!(segments.iter().all(|s| s["script_value"] == 1));
    assert!(body(&c).is_empty());
    save_stage(&mut c, &mut r, &root, "progress", &mut stages);
    assert_eq!(segments.iter().filter(|s| s["filled"] == true).count(), 0);
    let progress_before = state(&c);
    let progress_generation = c.gens.varp;
    assert_eq!(c.hint_type, 2, "Guide directs the player to the door");
    let (door_x, door_z) = (c.hint_tile_x, c.hint_tile_z);
    let door = operate_location(&mut c, door_x, door_z, "Open");
    pump_until(&mut c, &mut r, 20, |c| {
        c.hint_type == 1
            && c.npc
                .get(c.hint_npc as usize)
                .and_then(Option::as_ref)
                .is_some_and(|npc| npc.r#type == Some(943))
    });
    save_stage(&mut c, &mut r, &root, "door", &mut stages);
    let survival_slot = talk_to(&mut c, 943);
    pump_until(&mut c, &mut r, 20, |c| {
        c.chat_modal_id >= 0 && body(c).contains("Brynna")
    });
    save_stage(&mut c, &mut r, &root, "survival-introduction", &mut stages);
    continue_chat(&mut c, &mut r);
    pump_until(&mut c, &mut r, 10, |c| {
        !c.resumed_pause_button && body(c).contains("Bronze Hatchet")
    });
    save_stage(&mut c, &mut r, &root, "survival-tools", &mut stages);
    continue_chat(&mut c, &mut r);
    pump_until(&mut c, &mut r, 10, |c| {
        c.chat_modal_id == -1 && c.tut_flash_icon == 3
    });
    save_stage(&mut c, &mut r, &root, "inventory-request", &mut stages);
    // Java 377's real backpack tab hitbox; redraw sends TUT_CLICKSIDE 119.
    click(&mut c, &mut r, 660, 187, 1);
    pump_until(&mut c, &mut r, 10, |c| {
        c.active_icon == 3
            && c.tut_flash_icon == -1
            && inventory_count(c, 1351) == 1
            && inventory_count(c, 590) == 1
            && c.hint_type == 2
    });
    save_stage(&mut c, &mut r, &root, "inventory-open", &mut stages);
    let (tree_x, tree_z) = (c.hint_tile_x, c.hint_tile_z);
    let first_tree = operate_location(&mut c, tree_x, tree_z, "Chop down");
    pump_until(&mut c, &mut r, 30, |c| {
        inventory_count(c, 2511) == 1
            && c.chat_modal_id >= 0
            && body(c).contains("You get some logs")
    });
    save_stage(&mut c, &mut r, &root, "first-logs", &mut stages);
    continue_chat(&mut c, &mut r);
    // This content branch sets stage 40 but refreshes progress only on the
    // next successful chop. Wait for the real temporary stump to regrow.
    pump_until(&mut c, &mut r, 30, |c| {
        c.chat_modal_id == -1 && location_typecode(c, tree_x, tree_z, "Chop down").is_some()
    });
    let second_tree = operate_location(&mut c, tree_x, tree_z, "Chop down");
    pump_until(&mut c, &mut r, 30, |c| {
        inventory_count(c, 2511) == 2
            && c.var.get(406) == Some(&2)
            && c.var_serv.get(406) == Some(&2)
            && body(c).contains("You get some logs")
    });
    assert!(c.gens.varp > progress_generation);
    let progress_after = state(&c);
    let after_segments = progress_segments(&c);
    assert_eq!(after_segments.len(), 20);
    assert_eq!(
        after_segments
            .iter()
            .filter(|s| s["filled"] == true)
            .count(),
        1
    );
    save_stage(&mut c, &mut r, &root, "progress-transition", &mut stages);
    continue_chat(&mut c, &mut r);
    pump_until(&mut c, &mut r, 10, |c| c.chat_modal_id == -1);

    let mut item_actions = Vec::new();
    let firemaking_xp = c.stat_xp[11];
    item_actions.push(use_items(&mut c, 590, 2511));
    pump_until(&mut c, &mut r, 30, |c| {
        inventory_count(c, 2511) == 1
            && c.stat_xp[11] > firemaking_xp
            && c.tut_flash_icon == 1
            && scene_location(c, 3038).is_some()
    });
    save_stage(&mut c, &mut r, &root, "fire-lit", &mut stages);
    click(&mut c, &mut r, 584, 187, 1);
    pump_until(&mut c, &mut r, 10, |c| {
        c.active_icon == 1
            && c.tut_flash_icon == -1
            && tutorial_text(c).contains("These are your stats")
            && c.var.get(406) == Some(&2)
    });
    save_stage(&mut c, &mut r, &root, "skills-open", &mut stages);
    talk_to(&mut c, 943);
    pump_until(&mut c, &mut r, 20, |c| {
        body(c).contains("There's nothing like a good fire")
    });
    save_stage(&mut c, &mut r, &root, "fishing-instruction", &mut stages);
    continue_chat(&mut c, &mut r);
    pump_until(&mut c, &mut r, 10, |c| {
        !c.resumed_pause_button && body(c).contains("Net") && inventory_count(c, 303) == 1
    });
    save_stage(&mut c, &mut r, &root, "fishing-net", &mut stages);
    continue_chat(&mut c, &mut r);
    pump_until(&mut c, &mut r, 10, |c| c.chat_modal_id == -1);
    click(&mut c, &mut r, 660, 187, 1);
    let fishing_slot = talk_to(&mut c, 952);
    let fishing_xp = c.stat_xp[10];
    pump_until(&mut c, &mut r, 30, |c| {
        inventory_count(c, 2514) == 1
            && c.stat_xp[10] > fishing_xp
            && tutorial_text(c).contains("Cooking your shrimp")
    });
    save_stage(&mut c, &mut r, &root, "first-raw-shrimp", &mut stages);
    // The lesson explicitly asks for a new fire after fishing. Use the
    // remaining naturally chopped log at the current pond-side position.
    item_actions.push(use_items(&mut c, 590, 2511));
    pump_until(&mut c, &mut r, 30, |c| {
        inventory_count(c, 2511) == 0
            && scene_location(c, 3038).is_some()
            && c.var.get(406) == Some(&3)
    });
    save_stage(&mut c, &mut r, &root, "cooking-fire", &mut stages);
    item_actions.push(use_item_on_location(&mut c, 2514, 3038));
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, 2514) == 0
            && inventory_count(c, 323) == 1
            && tutorial_text(c).contains("Burning your shrimp")
    });
    save_stage(&mut c, &mut r, &root, "burnt-shrimp", &mut stages);
    talk_to(&mut c, 952);
    pump_until(&mut c, &mut r, 30, |c| {
        inventory_count(c, 2514) == 1 && tutorial_text(c).contains("Burning your shrimp")
    });
    save_stage(&mut c, &mut r, &root, "second-raw-shrimp", &mut stages);
    let cooking_xp = c.stat_xp[7];
    item_actions.push(use_item_on_location(&mut c, 2514, 3038));
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, 2514) == 0
            && inventory_count(c, 315) == 1
            && c.stat_xp[7] > cooking_xp
            && tutorial_text(c).contains("Well done, you've just cooked")
            && c.hint_type == 2
    });
    save_stage(&mut c, &mut r, &root, "cooked-shrimp", &mut stages);
    let (gate_x, gate_z) = (c.hint_tile_x, c.hint_tile_z);
    let gate = operate_location(&mut c, gate_x, gate_z, "Open");
    pump_until(&mut c, &mut r, 20, |c| {
        c.hint_type == 2
            && (c.hint_tile_x, c.hint_tile_z) != (gate_x, gate_z)
            && c.var.get(406) == Some(&4)
    });
    save_stage(&mut c, &mut r, &root, "survival-gate", &mut stages);
    let (chef_x, chef_z) = (c.hint_tile_x, c.hint_tile_z);
    // Content hints (6,12), immediately west of the actual (7,12) wall
    // placement in m48_48.jm2. Select the loaded door, not the hint tile.
    let (_, door_x, door_z) = scene_location(&c, 3017).expect("actual Master Chef door");
    assert!((door_x - chef_x).abs() <= 1 && (door_z - chef_z).abs() <= 1);
    let chef_door = json!({"server_hint":[chef_x,chef_z],
        "action":operate_location(&mut c, door_x, door_z, "Open")});
    pump_until(&mut c, &mut r, 30, |c| {
        c.hint_type == 1
            && c.npc
                .get(c.hint_npc as usize)
                .and_then(Option::as_ref)
                .is_some_and(|npc| npc.r#type == Some(942))
            && c.var.get(406) == Some(&5)
    });
    save_stage(&mut c, &mut r, &root, "chef-door", &mut stages);
    let chef_slot = talk_to(&mut c, 942);
    pump_until(&mut c, &mut r, 20, |c| body(c).contains("I am Lev"));
    for (index, expected) in [
        "I am Lev",
        "I already know how to cook",
        "HA HA HA",
        "no fine meal is complete",
    ]
    .into_iter()
    .enumerate()
    {
        assert!(body(&c).contains(expected), "chef page: {}", body(&c));
        save_stage(
            &mut c,
            &mut r,
            &root,
            &format!("chef-dialogue-{index}"),
            &mut stages,
        );
        let previous = body(&c);
        continue_chat(&mut c, &mut r);
        pump_until(&mut c, &mut r, 10, |c| {
            c.chat_modal_id >= 0 && !c.resumed_pause_button && body(c) != previous
        });
    }
    // The object-box text can precede the inventory update on the wire.
    pump_until(&mut c, &mut r, 10, |c| {
        body(c).contains("Pot of Flour")
            && inventory_count(c, 2516) == 1
            && inventory_count(c, 1929) == 1
    });
    assert!(body(&c).contains("Pot of Flour"));
    assert_eq!(inventory_count(&c, 2516), 1);
    assert_eq!(inventory_count(&c, 1929), 1);
    save_stage(&mut c, &mut r, &root, "chef-ingredients", &mut stages);
    continue_chat(&mut c, &mut r);
    pump_until(&mut c, &mut r, 10, |c| {
        c.chat_modal_id == -1 && tutorial_text(c).contains("Making dough")
    });
    item_actions.push(use_items(&mut c, 1929, 2516));
    pump_until(&mut c, &mut r, 10, |c| {
        inventory_count(c, 2516) == 0
            && inventory_count(c, 1929) == 0
            && inventory_count(c, 2307) == 1
            && tutorial_text(c).contains("Cooking dough")
            && c.var.get(406) == Some(&5)
    });
    save_stage(&mut c, &mut r, &root, "dough-mixed", &mut stages);
    item_actions.push(use_item_on_location(&mut c, 2307, 3039));
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, 2307) == 0
            && inventory_count(c, 2309) == 1
            && tutorial_text(c).contains("your first loaf of bread")
            && c.var.get(406) == Some(&6)
            && c.var_serv.get(406) == Some(&6)
    });
    save_stage(&mut c, &mut r, &root, "bread-baked", &mut stages);
    let final_state = state(&c);
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
    for opcode in [
        6,
        19,
        28,
        79,
        112,
        119,
        163,
        181,
        226,
        247,
        248,
        ClientProt377::OPHELDU.id,
        ClientProt377::OPLOCU.id,
    ] {
        assert!(
            packets.iter().any(|p| p["opcode"] == opcode),
            "missing required opcode{opcode}: {packets:?}"
        );
    }
    assert!(packets.iter().filter(|p| p["opcode"] == 226).count() >= 6);
    let receipt = json!({"engine_dir":engine,"user":user,"revision":377,"movement":{"before":before,"after":after,"click":[x,y],"initial_player_info":tick,"fine_destination":fine_target},
        "guide_slot":guide,"survival_slot":survival_slot,"dialogue_pages":pages,
        "fishing_slot":fishing_slot,"chef_slot":chef_slot,"item_actions":item_actions,"final_state":final_state,
        "locations":[door,first_tree,second_tree,gate,chef_door],"progress":{"varp":406,"before":progress_before,
        "after":progress_after,"before_segments":segments,"after_segments":after_segments,"overlay":8680,
        "meaning":"actual script writes stage 40, then refreshes overlay to 1 + floor(40 * 20 / 670) = 2; one filled segment; no varp seeding"},
        "stages":stages,"outbound":*packets,"logout":exit});
    std::fs::write(
        root.join("rust-interactions.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("PASS: natural Guide, firemaking, stats, fishing, burnt/cooked shrimp, chef ingredients, dough, bread, server varp406 1->6, server logout");
}

#[path = "support/tutorial_lesson_377.rs"]
mod tutorial_lesson_377;
