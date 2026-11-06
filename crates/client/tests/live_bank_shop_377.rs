//! Opt-in live proof of Java 377 bank, shop, amount-dialog, and inventory-drag dispatch.
//! It requires the orchestrator-started Fairy Ring engine and a prepared account.
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

const COINS: i32 = 995;
const BUCKET: i32 = 1925;
const SHOP_OBJECT: i32 = 2347; // Hammer: the real general-store stock has five.
const WITHDRAW_X: i32 = 7;
const DEPOSIT_X: i32 = 3;
const MIN_SHOP_COINS: i32 = 100;

#[derive(Default)]
struct Outbound(Mutex<Vec<Value>>);

impl PacketObserver for Outbound {
    fn packet_body_length(&self, opcode: u8) -> i16 {
        ClientProt377::ALL
            .iter()
            .find(|packet| packet.id == i32::from(opcode))
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

fn tile(c: &Client) -> (i32, i32) {
    let player = c
        .local_player
        .as_ref()
        .expect("local player is server populated");
    (
        c.map_build_base_x + player.route_x[0],
        c.map_build_base_z + player.route_z[0],
    )
}

fn interface_text(c: &Client, layer: i32) -> String {
    if layer < 0 {
        return String::new();
    }
    c.ifaces_merged()
        .filter(|component| component.layer_id == layer)
        .flat_map(|component| [component.text.to_string(), component.button_text.clone()])
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn inventory_count(c: &Client, object: i32) -> i32 {
    let Some(inventory) = c.if_(3214) else {
        return 0;
    };
    match (&inventory.link_obj_type, &inventory.link_obj_number) {
        (Some(objects), Some(counts)) => objects
            .iter()
            .zip(counts)
            .filter(|(id, _)| **id == object + 1)
            .map(|(_, count)| *count)
            .sum(),
        _ => 0,
    }
}

fn inventory_component(c: &Client, layer: i32) -> i32 {
    c.ifaces_merged()
        .find(|component| {
            component.layer_id == layer
                && component.link_obj_type.is_some()
                && component.link_obj_number.is_some()
        })
        .map(|component| component.id)
        .unwrap_or_else(|| panic!("server-populated inventory component is open in layer {layer}"))
}

fn side_player_inventory(c: &Client) -> i32 {
    let player = c.if_(3214).expect("server-populated player inventory");
    let types = player
        .link_obj_type
        .as_deref()
        .expect("player inventory object slots");
    let counts = player
        .link_obj_number
        .as_deref()
        .expect("player inventory count slots");
    c.ifaces_merged()
        .find(|component| {
            component.layer_id == c.side_modal_id
                && component.link_obj_type.as_deref() == Some(types)
                && component.link_obj_number.as_deref() == Some(counts)
        })
        .map(|component| component.id)
        .expect("bank or shop transmits the live player inventory to its side component")
}

fn count_component(c: &Client, component: i32, object: i32) -> i32 {
    let inventory = c
        .if_(component as usize)
        .expect("server-populated item component");
    match (&inventory.link_obj_type, &inventory.link_obj_number) {
        (Some(objects), Some(counts)) => objects
            .iter()
            .zip(counts)
            .filter(|(id, _)| **id == object + 1)
            .map(|(_, count)| *count)
            .sum(),
        _ => 0,
    }
}

fn slot_for(c: &Client, component: i32, object: i32) -> i32 {
    let inventory = c
        .if_(component as usize)
        .expect("server-populated item component");
    let objects = inventory
        .link_obj_type
        .as_ref()
        .expect("item component object slots");
    let counts = inventory
        .link_obj_number
        .as_ref()
        .expect("item component count slots");
    objects
        .iter()
        .zip(counts)
        .position(|(id, count)| *id == object + 1 && *count > 0)
        .unwrap_or_else(|| {
            panic!("object {object} occupies a server-populated slot in {component}")
        }) as i32
}

fn component_items(c: &Client, component: i32) -> Vec<Value> {
    let Some(inventory) = c.if_(component as usize) else {
        return Vec::new();
    };
    let (Some(objects), Some(counts)) = (&inventory.link_obj_type, &inventory.link_obj_number)
    else {
        return Vec::new();
    };
    objects
        .iter()
        .zip(counts)
        .enumerate()
        .filter(|(_, (object_plus_one, count))| **object_plus_one > 0 && **count > 0)
        .map(|(slot, (&object_plus_one, &count))| {
            let object = object_plus_one - 1;
            json!({"slot":slot,"object":object,"count":count,
                "name":c.cache.objs.get(object as usize).map(|item|item.name.as_str())})
        })
        .collect()
}

fn state(c: &Client) -> Value {
    let main_items = (c.main_modal_id >= 0)
        .then(|| inventory_component(c, c.main_modal_id))
        .map(|component| json!({"component":component,"items":component_items(c,component)}));
    let side_items = (c.side_modal_id >= 0)
        .then(|| inventory_component(c, c.side_modal_id))
        .map(|component| json!({"component":component,"items":component_items(c,component)}));
    json!({
        "connected":c.ingame,
        "scene_state":c.scene_state,
        "tile":tile(c),
        "player_info_generation":c.gens.player_info,
        "main_modal":c.main_modal_id,
        "main_interface":interface_text(c,c.main_modal_id),
        "main_inventory":main_items,
        "side_modal":c.side_modal_id,
        "side_interface":interface_text(c,c.side_modal_id),
        "side_inventory":side_items,
        "inventory_coins":inventory_count(c,COINS),
        "inventory_buckets":inventory_count(c,BUCKET),
        "dialog_input_open":c.dialog_input_open,
        "dialog_input":c.dialog_input
    })
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
    panic!("live bank/shop deadline: {}", state(c));
}

fn component_rect(
    c: &Client,
    root: i32,
    target: i32,
    x: i32,
    y: i32,
) -> Option<(i32, i32, i32, i32)> {
    let component = c.if_(root as usize)?;
    if root == target {
        return Some((x, y, component.width, component.height));
    }
    let children = component.children.as_ref()?;
    let child_x = component.child_x.as_ref()?;
    let child_y = component.child_y.as_ref()?;
    for ((&child_id, &offset_x), &offset_y) in children.iter().zip(child_x).zip(child_y) {
        let Some(child) = c.if_(child_id as usize) else {
            continue;
        };
        if let Some(rect) = component_rect(
            c,
            child_id,
            target,
            x + offset_x + child.x,
            y + offset_y + child.y - component.scroll_pos,
        ) {
            return Some(rect);
        }
    }
    None
}

fn component_origin(c: &Client, root: i32) -> (i32, i32) {
    if root == c.main_modal_id {
        (4, 4)
    } else if root == c.side_modal_id {
        (553, 205)
    } else if root == c.chat_modal_id {
        (17, 357)
    } else {
        panic!("open component root {root} has a known rendered origin")
    }
}

fn slot_center(c: &Client, root: i32, component: i32, slot: i32) -> (i32, i32) {
    let inventory = c
        .if_(component as usize)
        .expect("actual rendered inventory component");
    assert!(inventory.width > 0 && inventory.height > 0);
    assert!(slot >= 0 && slot < inventory.width * inventory.height);
    let origin = component_origin(c, root);
    let (x, y, _, _) = component_rect(c, root, component, origin.0, origin.1)
        .expect("inventory slot belongs to the open interface");
    let column = slot % inventory.width;
    let row = slot / inventory.width;
    let background_x = if slot < 20 {
        inventory
            .inv_background_x
            .as_ref()
            .and_then(|offsets| offsets.get(slot as usize))
            .copied()
            .unwrap_or(0)
    } else {
        0
    };
    let background_y = if slot < 20 {
        inventory
            .inv_background_y
            .as_ref()
            .and_then(|offsets| offsets.get(slot as usize))
            .copied()
            .unwrap_or(0)
    } else {
        0
    };
    (
        x + column * (inventory.margin_x + 32) + background_x + 16,
        y + row * (inventory.margin_y + 32) + background_y + 16,
    )
}

fn packet_for_inventory_action(action: i32) -> Option<i32> {
    match action {
        MiniMenuAction::INV_BUTTON1 => Some(ClientProt377::INV_BUTTON1.id),
        MiniMenuAction::INV_BUTTON2 => Some(ClientProt377::INV_BUTTON2.id),
        MiniMenuAction::INV_BUTTON3 => Some(ClientProt377::INV_BUTTON3.id),
        MiniMenuAction::INV_BUTTON4 => Some(ClientProt377::INV_BUTTON4.id),
        MiniMenuAction::INV_BUTTON5 => Some(ClientProt377::INV_BUTTON5.id),
        _ => None,
    }
}

fn invoke_inventory_option(
    c: &mut Client,
    r: &mut Renderer,
    root: i32,
    component: i32,
    slot: i32,
    object: i32,
    label: &str,
) -> (i32, Value) {
    let (x, y) = slot_center(c, root, component, slot);
    c.shell.apply_mouse_move(x, y);
    drop(r.mainredraw(c));
    let prefix = format!("{label} @lre@");
    let matches = (0..c.menu_num_entries.max(0) as usize)
        .filter(|&index| {
            packet_for_inventory_action(c.menu_action[index]).is_some()
                && c.menu_param_a[index] == object
                && c.menu_param_b[index] == slot
                && c.menu_param_c[index] == component
                && c.menu_option[index].starts_with(&prefix)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "one production menu entry for {label} on object {object}: {:?}",
        (0..c.menu_num_entries.max(0) as usize)
            .map(|index| json!({"action":c.menu_action[index],"option":c.menu_option[index],
                "object":c.menu_param_a[index],"slot":c.menu_param_b[index],"component":c.menu_param_c[index]}))
            .collect::<Vec<_>>()
    );
    let entry = matches[0];
    let action = c.menu_action[entry];
    let opcode = packet_for_inventory_action(action)
        .expect("selected production inventory option maps to a Java 377 packet");
    let receipt = json!({
        "object":object,
        "name":c.cache.objs[object as usize].name.as_str(),
        "slot":slot,
        "component":component,
        "option":label,
        "menu_option":c.menu_option[entry].as_str(),
        "menu_action":action,
        "packet_opcode":opcode,
        "input":"rendered production inventory option selected through Client::doAction"
    });
    c.doAction(entry as i32);
    eprintln!("377 bank/shop action {receipt}");
    (opcode, receipt)
}

fn packet_count(outbound: &Outbound) -> usize {
    outbound.0.lock().unwrap().len()
}

fn assert_packet_since(outbound: &Outbound, start: usize, opcode: i32) {
    let packets = outbound.0.lock().unwrap();
    assert!(
        packets[start..]
            .iter()
            .any(|packet| packet["opcode"].as_i64() == Some(i64::from(opcode))),
        "packet opcode {opcode} committed after outbound record {start}: {packets:?}"
    );
}

fn outbound_snapshot(outbound: &Outbound) -> Vec<Value> {
    outbound.0.lock().unwrap().clone()
}

fn click(c: &mut Client, r: &mut Renderer, x: i32, y: i32) {
    c.shell.apply_mouse_move(x, y);
    drop(r.mainredraw(c));
    c.shell.apply_mouse_down(1, x, y);
    c.shell.latch_click();
    c.mainloop();
    assert!(c.ingame, "live interface click kept the session connected");
    drop(r.mainredraw(c));
    c.shell.apply_mouse_up();
    c.shell.latch_click();
    c.mainloop();
    assert!(
        c.ingame,
        "live interface release kept the session connected"
    );
    drop(r.mainredraw(c));
}

fn click_component(c: &mut Client, r: &mut Renderer, root: i32, target: i32) {
    let (x, y, width, height) = component_rect(
        c,
        root,
        target,
        component_origin(c, root).0,
        component_origin(c, root).1,
    )
    .expect("button belongs to the open interface");
    assert!(width > 0 && height > 0);
    click(c, r, x + width / 2, y + height / 2);
}

fn close_main(c: &mut Client, r: &mut Renderer) {
    let root = c.main_modal_id;
    let close = c
        .ifaces_merged()
        .find(|component| component.layer_id == root && component.button_type == 3)
        .expect("open interface exposes its real close button")
        .id;
    click_component(c, r, root, close);
    pump_until(c, r, 10, |c| c.main_modal_id < 0 && c.side_modal_id < 0);
    let generation = c.gens.player_info;
    pump_until(c, r, 10, |c| c.gens.player_info >= generation + 2);
}

fn walk_waypoint(
    c: &mut Client,
    r: &mut Renderer,
    requested: (i32, i32),
    actions: &mut Vec<Value>,
) {
    pump_until(c, r, 30, |c| c.scene_state == 2);
    let before = tile(c);
    let player = c.local_player.as_ref().unwrap();
    let (x, z) = (player.route_x[0], player.route_z[0]);
    assert!(c.tryMove(
        x,
        z,
        requested.0 - c.map_build_base_x,
        requested.1 - c.map_build_base_z,
        true,
        0,
        0,
        0,
        0,
        0,
        0,
    ));
    let endpoint = *c
        .try_move_path
        .last()
        .expect("production path has an endpoint");
    let target = (
        endpoint.0 + c.map_build_base_x,
        endpoint.1 + c.map_build_base_z,
    );
    assert!(
        (target.0 - requested.0)
            .abs()
            .max((target.1 - requested.1).abs())
            <= 1
    );
    pump_until(c, r, 45, |c| tile(c) == target);
    actions.push(
        json!({"walk_from":before,"requested":requested,"arrived":tile(c),
        "input":"production Client::tryMove game-click pathfinder; received player position"}),
    );
}

const SHOP_ROUTE: &[(i32, i32)] = &[
    (3095, 3250),
    (3110, 3250),
    (3125, 3250),
    (3140, 3250),
    (3155, 3250),
    (3170, 3250),
    (3185, 3250),
    (3200, 3250),
    (3210, 3247),
];

fn npc_slot(c: &Client, name: &str, option: &str) -> i32 {
    let player = tile(c);
    c.npc_ids
        .iter()
        .take(c.npc_count as usize)
        .copied()
        .filter(|&slot| {
            c.npc[slot as usize]
                .as_ref()
                .and_then(|npc| npc.r#type)
                .and_then(|kind| c.cache.npcs.get(kind))
                .is_some_and(|npc| {
                    npc.name.eq_ignore_ascii_case(name)
                        && npc.op.iter().flatten().any(|candidate| candidate == option)
                })
        })
        .min_by_key(|&slot| {
            let npc = c.npc[slot as usize].as_ref().unwrap();
            let dx = (npc.route_x[0] + c.map_build_base_x - player.0).abs();
            let dz = (npc.route_z[0] + c.map_build_base_z - player.1).abs();
            (dx.max(dz), dx + dz)
        })
        .unwrap_or_else(|| {
            panic!("server-populated {name} with {option} option is visible near {player:?}")
        })
}

fn operate_npc(c: &mut Client, slot: i32, action: i32, option: &str) -> Value {
    let npc = c.npc[slot as usize]
        .as_ref()
        .expect("server-populated NPC slot");
    let kind = npc.r#type.expect("server-populated NPC type");
    let receipt = json!({
        "slot":slot,
        "type":kind,
        "name":c.cache.npcs[kind].name.as_str(),
        "option":option,
        "player":tile(c),
        "npc_tile":[npc.route_x[0]+c.map_build_base_x,npc.route_z[0]+c.map_build_base_z],
        "input":"production Client::doAction on the received NPC slot"
    });
    c.menu_action[0] = action;
    c.menu_param_a[0] = slot;
    c.menu_param_b[0] = 0;
    c.menu_param_c[0] = 0;
    c.doAction(0);
    eprintln!("377 bank/shop NPC action {receipt}");
    receipt
}

fn enter_amount(c: &mut Client, r: &mut Renderer, amount: i32) -> Value {
    pump_until(c, r, 15, |c| c.dialog_input_open);
    assert!(
        c.dialog_input.is_empty(),
        "amount prompt starts without prefilled text"
    );
    let digits = amount.to_string();
    for digit in digits.bytes() {
        c.shell.apply_key(true, 0, i32::from(digit));
        c.shell.latch_click();
        c.mainloop();
        assert!(c.ingame, "typing an amount kept the session connected");
        drop(r.mainredraw(c));
    }
    assert_eq!(
        c.dialog_input, digits,
        "amount was entered through queued UI keys"
    );
    c.shell.apply_key(true, 0, 13);
    c.shell.latch_click();
    c.mainloop();
    assert!(c.ingame, "submitting an amount kept the session connected");
    drop(r.mainredraw(c));
    assert!(
        !c.dialog_input_open,
        "Enter submitted the actual amount prompt"
    );
    json!({
        "prompt":"server-opened P_COUNTDIALOG",
        "input":"GameShell digit key events followed by Enter through Client::handle_chat_input",
        "amount":amount
    })
}

fn capture_stage(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    name: &str,
    stages: &mut Vec<Value>,
) {
    let FrameOutput::PixMap(map) = r.mainredraw(c) else {
        panic!("CPU live proof required")
    };
    let path = root.join(format!("{name}.jpg"));
    let rgb: Vec<u8> = map
        .pixels
        .iter()
        .flat_map(|&pixel| [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8])
        .collect();
    image::codecs::jpeg::JpegEncoder::new_with_quality(std::fs::File::create(&path).unwrap(), 90)
        .encode(
            &rgb,
            map.width as u32,
            map.height as u32,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    let record = json!({"stage":name,"screenshot":path,"state":state(c)});
    stages.push(record.clone());
    std::fs::write(
        root.join("stages.json"),
        serde_json::to_vec_pretty(stages).unwrap(),
    )
    .unwrap();
    eprintln!("377 bank/shop stage {record}");
}

fn fixture_interventions() -> Vec<Value> {
    let encoded = std::env::var("CLIENT_377_BANK_SHOP_INTERVENTIONS")
        .expect("explicit JSON fixture-intervention labels required; use [] when none were made");
    let interventions: Vec<Value> = serde_json::from_str(&encoded)
        .expect("fixture interventions must be a JSON array of labeled objects");
    assert!(
        interventions.iter().all(|item| {
            item.get("label")
                .and_then(Value::as_str)
                .is_some_and(|label| !label.trim().is_empty())
        }),
        "each fixture intervention needs a nonempty label"
    );
    interventions
}

#[test]
#[ignore = "requires orchestrator-started Fairy Ring 377 engine and prepared account near Draynor bank"]
fn bank_shop_amount_drag_live_377() {
    let root = PathBuf::from(
        std::env::var("CLIENT_377_LIVE_DIR").expect("explicit evidence directory required"),
    );
    let cache =
        std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("matched running cache required");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("explicit account required");
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists(), "quiet lock blocks live runs");
    }
    let mut interventions = fixture_interventions();
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
            && c.local_player.is_some()
            && c.side_icon[0] >= 0
            && inventory_count(c, BUCKET) > 0
    });
    let start_tile = tile(&c);
    let mut actions = Vec::new();
    if (start_tile.0 - 3092).abs().max((start_tile.1 - 3243).abs()) > 15 {
        assert!((start_tile.0 - 3150).abs() <= 80 && (start_tile.1 - 3250).abs() <= 20);
        for &waypoint in SHOP_ROUTE.iter().rev().skip(1) {
            walk_waypoint(&mut c, &mut r, waypoint, &mut actions);
        }
        walk_waypoint(&mut c, &mut r, (3092, 3243), &mut actions);
    }
    assert!(
        (tile(&c).0 - 3092).abs().max((tile(&c).1 - 3243).abs()) <= 15,
        "coordinated account arrives near Draynor bank: {:?}",
        tile(&c)
    );
    if std::env::var("CLIENT_377_BANK_SHOP_PREPARE").as_deref() == Ok("1") {
        c.set_cheat_admission(client::client::CheatAdmission::Granted);
        for (object, name, target) in [(COINS, "coins", 1000), (BUCKET, "bucket_empty", 5)] {
            let before = inventory_count(&c, object);
            if before < target {
                let command = format!("give {name} {}", target - before);
                assert!(c.send_cheat(&command).is_sent());
                pump_until(&mut c, &mut r, 10, |c| inventory_count(c, object) == target);
                interventions.push(json!({
                    "label":"Seed only the coordinated account's inventory before the tested actions",
                    "method":"admitted CLIENT_CHEAT to the selected Fairy Ring engine",
                    "command":command,"object":object,"before":before,
                    "after":inventory_count(&c,object)
                }));
            }
        }
        c.set_cheat_admission(client::client::CheatAdmission::Denied);
        std::fs::write(
            root.join("fixture-interventions.json"),
            serde_json::to_vec_pretty(&interventions).unwrap(),
        )
        .unwrap();
    }
    assert!(
        inventory_count(&c, COINS) >= 10,
        "fixture has at least 10 coins in the player's inventory"
    );
    assert!(
        inventory_count(&c, BUCKET) >= 5,
        "fixture has empty buckets for the inventory drag"
    );
    assert!(
        inventory_count(&c, COINS) >= MIN_SHOP_COINS,
        "fixture has enough of its own coins for the shop Buy-5"
    );
    let mut stages = Vec::new();
    capture_stage(&mut c, &mut r, &root, "connected-before-bank", &mut stages);
    let bank_npc = npc_slot(&c, "Banker", "Bank");
    actions.push(operate_npc(
        &mut c,
        bank_npc,
        MiniMenuAction::OP_NPC3,
        "Bank",
    ));
    pump_until(&mut c, &mut r, 30, |c| {
        c.main_modal_id >= 0
            && c.side_modal_id >= 0
            && interface_text(c, c.main_modal_id).contains("Bank of RuneScape")
    });
    let bank_component = inventory_component(&c, c.main_modal_id);
    let player_component = side_player_inventory(&c);
    let inventory_coins_start = inventory_count(&c, COINS);
    let bank_coins_start = count_component(&c, bank_component, COINS);
    assert!(bank_coins_start >= 10, "fixture bank has at least 10 coins");
    capture_stage(&mut c, &mut r, &root, "bank-open", &mut stages);

    let start = packet_count(&outbound);
    let bank_root = c.main_modal_id;
    let bank_slot = slot_for(&c, bank_component, COINS);
    let (opcode, action) = invoke_inventory_option(
        &mut c,
        &mut r,
        bank_root,
        bank_component,
        bank_slot,
        COINS,
        "Withdraw 10",
    );
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, COINS) == inventory_coins_start + 10
            && count_component(c, bank_component, COINS) == bank_coins_start - 10
    });
    assert_packet_since(&outbound, start, opcode);
    actions.push(json!({"operation":"bank Withdraw-10","menu":action,
        "inventory_coins":[inventory_coins_start,inventory_count(&c,COINS)],
        "bank_coins":[bank_coins_start,count_component(&c,bank_component,COINS)]}));
    capture_stage(&mut c, &mut r, &root, "bank-withdraw-10", &mut stages);

    let start = packet_count(&outbound);
    let side_root = c.side_modal_id;
    let player_coins_slot = slot_for(&c, player_component, COINS);
    let (opcode, action) = invoke_inventory_option(
        &mut c,
        &mut r,
        side_root,
        player_component,
        player_coins_slot,
        COINS,
        "Deposit 10",
    );
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, COINS) == inventory_coins_start
            && count_component(c, bank_component, COINS) == bank_coins_start
    });
    assert_packet_since(&outbound, start, opcode);
    actions.push(json!({"operation":"bank Deposit-10","menu":action,
        "inventory_coins":[inventory_coins_start+10,inventory_count(&c,COINS)],
        "bank_coins":[bank_coins_start-10,count_component(&c,bank_component,COINS)]}));
    capture_stage(&mut c, &mut r, &root, "bank-deposit-10", &mut stages);

    let start = packet_count(&outbound);
    let bank_root = c.main_modal_id;
    let bank_slot = slot_for(&c, bank_component, COINS);
    let (opcode, action) = invoke_inventory_option(
        &mut c,
        &mut r,
        bank_root,
        bank_component,
        bank_slot,
        COINS,
        "Withdraw X",
    );
    pump_until(&mut c, &mut r, 15, |c| c.dialog_input_open);
    capture_stage(&mut c, &mut r, &root, "bank-withdraw-x-prompt", &mut stages);
    assert_packet_since(&outbound, start, opcode);
    let prompt = enter_amount(&mut c, &mut r, WITHDRAW_X);
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, COINS) == inventory_coins_start + WITHDRAW_X
            && count_component(c, bank_component, COINS) == bank_coins_start - WITHDRAW_X
    });
    assert_packet_since(&outbound, start, ClientProt377::RESUME_P_COUNTDIALOG.id);
    actions.push(
        json!({"operation":"bank Withdraw-X","menu":action,"prompt":prompt,
        "inventory_coins":[inventory_coins_start,inventory_count(&c,COINS)],
        "bank_coins":[bank_coins_start,count_component(&c,bank_component,COINS)]}),
    );
    capture_stage(&mut c, &mut r, &root, "bank-withdraw-x", &mut stages);

    let start = packet_count(&outbound);
    let side_root = c.side_modal_id;
    let player_coins_slot = slot_for(&c, player_component, COINS);
    let (opcode, action) = invoke_inventory_option(
        &mut c,
        &mut r,
        side_root,
        player_component,
        player_coins_slot,
        COINS,
        "Deposit X",
    );
    pump_until(&mut c, &mut r, 15, |c| c.dialog_input_open);
    capture_stage(&mut c, &mut r, &root, "bank-deposit-x-prompt", &mut stages);
    assert_packet_since(&outbound, start, opcode);
    let prompt = enter_amount(&mut c, &mut r, DEPOSIT_X);
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, COINS) == inventory_coins_start + WITHDRAW_X - DEPOSIT_X
            && count_component(c, bank_component, COINS)
                == bank_coins_start - WITHDRAW_X + DEPOSIT_X
    });
    assert_packet_since(&outbound, start, ClientProt377::RESUME_P_COUNTDIALOG.id);
    actions.push(
        json!({"operation":"bank Deposit-X","menu":action,"prompt":prompt,
        "inventory_coins":[inventory_coins_start+WITHDRAW_X,inventory_count(&c,COINS)],
        "bank_coins":[bank_coins_start-WITHDRAW_X,count_component(&c,bank_component,COINS)]}),
    );
    capture_stage(&mut c, &mut r, &root, "bank-deposit-x", &mut stages);
    let inventory_coins_after_bank = inventory_count(&c, COINS);
    let bank_coins_after = count_component(&c, bank_component, COINS);
    assert_eq!(
        inventory_coins_after_bank,
        inventory_coins_start + WITHDRAW_X - DEPOSIT_X
    );

    let buckets_before_drag = inventory_count(&c, BUCKET);
    let coins_before_drag = inventory_count(&c, COINS);
    let source_slot = slot_for(&c, player_component, BUCKET);
    let destination_slot = slot_for(&c, player_component, COINS);
    assert_ne!(source_slot, destination_slot);
    let side_root = c.side_modal_id;
    let (source_x, source_y) = slot_center(&c, side_root, player_component, source_slot);
    let (destination_x, destination_y) =
        slot_center(&c, side_root, player_component, destination_slot);
    let before_drag_counts = c
        .if_(player_component as usize)
        .unwrap()
        .link_obj_number
        .as_ref()
        .unwrap()
        .clone();
    let start = packet_count(&outbound);
    let generation = c.gens.player_info;
    c.shell.apply_mouse_move(source_x, source_y);
    drop(r.mainredraw(&mut c));
    c.shell.apply_mouse_down(1, source_x, source_y);
    c.shell.latch_click();
    c.mainloop();
    assert!(c.ingame, "drag start kept the session connected");
    assert_eq!(
        c.obj_drag_area, 2,
        "drag began in the bank's player inventory"
    );
    assert_eq!(c.obj_drag_com_id, player_component);
    assert_eq!(c.obj_drag_slot, source_slot);
    c.shell.apply_mouse_move(destination_x, destination_y);
    for _ in 0..5 {
        std::thread::sleep(Duration::from_millis(20));
        c.shell.latch_click();
        c.mainloop();
        assert!(c.ingame, "held inventory drag kept the session connected");
        drop(r.mainredraw(&mut c));
    }
    c.shell.apply_mouse_up();
    c.shell.latch_click();
    c.mainloop();
    assert!(
        c.ingame,
        "inventory drag release kept the session connected"
    );
    drop(r.mainredraw(&mut c));
    assert_eq!(c.obj_drag_area, 0, "completed drag left no active drag");
    let (source_after, destination_after) = {
        let after_drag = c
            .if_(player_component as usize)
            .expect("bank player inventory remains open");
        let after_drag_types = after_drag.link_obj_type.as_ref().unwrap();
        let after_drag_counts = after_drag.link_obj_number.as_ref().unwrap();
        assert_eq!(after_drag_types[source_slot as usize], COINS + 1);
        assert_eq!(after_drag_types[destination_slot as usize], BUCKET + 1);
        assert_eq!(
            after_drag_counts[source_slot as usize],
            before_drag_counts[destination_slot as usize]
        );
        assert_eq!(
            after_drag_counts[destination_slot as usize],
            before_drag_counts[source_slot as usize]
        );
        (
            after_drag_types[source_slot as usize] - 1,
            after_drag_types[destination_slot as usize] - 1,
        )
    };
    assert_eq!(
        inventory_count(&c, COINS),
        inventory_coins_start + WITHDRAW_X - DEPOSIT_X
    );
    assert_eq!(inventory_count(&c, COINS), coins_before_drag);
    assert_eq!(inventory_count(&c, BUCKET), buckets_before_drag);
    assert_packet_since(&outbound, start, ClientProt377::INV_BUTTOND.id);
    pump_until(&mut c, &mut r, 10, |c| c.gens.player_info > generation);
    let drag = json!({"source_slot":source_slot,"destination_slot":destination_slot,
        "component":player_component,"from_object":BUCKET,"to_object":COINS,
        "input":"real mouse-down, held pointer movement, five Client::mainloop drag cycles, mouse-up through Client::handle_obj_drag",
        "packet_opcode":ClientProt377::INV_BUTTOND.id,
        "source_after":source_after,"destination_after":destination_after});
    actions.push(json!({"operation":"inventory item drag","result":drag}));
    capture_stage(&mut c, &mut r, &root, "bank-inventory-drag", &mut stages);
    close_main(&mut c, &mut r);
    for &waypoint in SHOP_ROUTE {
        walk_waypoint(&mut c, &mut r, waypoint, &mut actions);
    }

    let shopkeeper = npc_slot(&c, "Shop keeper", "Trade");
    actions.push(operate_npc(
        &mut c,
        shopkeeper,
        MiniMenuAction::OP_NPC3,
        "Trade",
    ));
    pump_until(&mut c, &mut r, 30, |c| {
        c.main_modal_id >= 0
            && c.side_modal_id >= 0
            && interface_text(c, c.main_modal_id).contains("Lumbridge General Store")
    });
    assert!(interface_text(&c, c.main_modal_id).contains("Lumbridge General Store"));
    let stock_component = inventory_component(&c, c.main_modal_id);
    let shop_player_component = side_player_inventory(&c);
    let stock_before_buy = count_component(&c, stock_component, SHOP_OBJECT);
    assert!(
        stock_before_buy >= 5,
        "actual Lumbridge shop stock offers Buy-5"
    );
    let items_before_buy = inventory_count(&c, SHOP_OBJECT);
    let coins_before_buy = inventory_count(&c, COINS);
    assert!(coins_before_buy >= MIN_SHOP_COINS);
    let free_slots = c
        .if_(shop_player_component as usize)
        .and_then(|inventory| inventory.link_obj_type.as_ref())
        .map(|objects| objects.iter().filter(|object| **object == 0).count())
        .unwrap_or(0);
    assert!(
        free_slots >= 5,
        "fixture leaves five inventory slots for Buy-5"
    );
    capture_stage(&mut c, &mut r, &root, "lumbridge-shop-open", &mut stages);

    let start = packet_count(&outbound);
    let shop_root = c.main_modal_id;
    let shop_stock_slot = slot_for(&c, stock_component, SHOP_OBJECT);
    let (opcode, action) = invoke_inventory_option(
        &mut c,
        &mut r,
        shop_root,
        stock_component,
        shop_stock_slot,
        SHOP_OBJECT,
        "Buy 5",
    );
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, SHOP_OBJECT) == items_before_buy + 5
            && inventory_count(c, COINS) < coins_before_buy
            && count_component(c, stock_component, SHOP_OBJECT) == stock_before_buy - 5
    });
    assert_packet_since(&outbound, start, opcode);
    actions.push(json!({"operation":"shop Buy-5","menu":action,
        "items":[items_before_buy,inventory_count(&c,SHOP_OBJECT)],
        "coins":[coins_before_buy,inventory_count(&c,COINS)],
        "stock":[stock_before_buy,count_component(&c,stock_component,SHOP_OBJECT)]}));
    capture_stage(&mut c, &mut r, &root, "lumbridge-shop-buy-5", &mut stages);

    let items_before_sell = inventory_count(&c, SHOP_OBJECT);
    let coins_before_sell = inventory_count(&c, COINS);
    let stock_before_sell = count_component(&c, stock_component, SHOP_OBJECT);
    let start = packet_count(&outbound);
    let side_root = c.side_modal_id;
    let shop_inventory_slot = slot_for(&c, shop_player_component, SHOP_OBJECT);
    let (opcode, action) = invoke_inventory_option(
        &mut c,
        &mut r,
        side_root,
        shop_player_component,
        shop_inventory_slot,
        SHOP_OBJECT,
        "Sell 5",
    );
    // The engine shop formula floors a one-coin hammer's resale value to zero.
    // Require that exact balance as well as both received item-count changes.
    pump_until(&mut c, &mut r, 20, |c| {
        inventory_count(c, SHOP_OBJECT) == items_before_sell - 5
            && inventory_count(c, COINS) == coins_before_sell
            && count_component(c, stock_component, SHOP_OBJECT) == stock_before_sell + 5
    });
    assert_packet_since(&outbound, start, opcode);
    actions.push(json!({"operation":"shop Sell-5","menu":action,
        "items":[items_before_sell,inventory_count(&c,SHOP_OBJECT)],
        "coins":[coins_before_sell,inventory_count(&c,COINS)],
        "stock":[stock_before_sell,count_component(&c,stock_component,SHOP_OBJECT)]}));
    capture_stage(&mut c, &mut r, &root, "lumbridge-shop-sell-5", &mut stages);
    close_main(&mut c, &mut r);
    for &waypoint in SHOP_ROUTE.iter().rev().skip(1) {
        walk_waypoint(&mut c, &mut r, waypoint, &mut actions);
    }
    walk_waypoint(&mut c, &mut r, (3092, 3243), &mut actions);
    capture_stage(&mut c, &mut r, &root, "returned-draynor", &mut stages);
    assert!(
        c.ingame,
        "account remains connected after bank and shop proof"
    );

    let receipt = json!({
        "engine_dir":engine,
        "user":user,
        "revision":377,
        "starting_tile":start_tile,
        "connected_at_completion":c.ingame,
        "bank":{"object":COINS,"inventory_before":inventory_coins_start,
            "bank_after":bank_coins_after,"inventory_after_bank":inventory_coins_after_bank,
            "withdraw10":10,"withdraw_x":WITHDRAW_X,"deposit10":10,"deposit_x":DEPOSIT_X,
            "inventory_net":WITHDRAW_X-DEPOSIT_X,"bank_net":DEPOSIT_X-WITHDRAW_X},
        "shop":{"npc":"Lumbridge General Store Shop keeper","object":SHOP_OBJECT,
            "buy5":5,"sell5":5,"coins_after":inventory_count(&c,COINS),
            "items_after":inventory_count(&c,SHOP_OBJECT)},
        "actions":actions,
        "stages":stages,
        "interventions":interventions,
        "outbound":outbound_snapshot(&outbound)
    });
    std::fs::write(
        root.join("bank-shop-377.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("PASS: live 377 bank Withdraw/Deposit 10/X, amount prompts, inventory drag, shop Buy/Sell-5; {receipt}");
}
