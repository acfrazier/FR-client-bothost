//! Opt-in end-to-end production proof for the revision-377 client.
//! Requires the orchestrator's already-running engine; it never starts or
//! changes the server. Inputs are admitted only when PREPARE=1; all outputs,
//! experience, levels, interfaces, animations and screenshots are server-driven.
#![expect(
    clippy::too_many_arguments,
    reason = "live proof helpers pass session and evidence dependencies explicitly"
)]
use client::client::{CheatAdmission, Client, ClientConfig, ClientRevision, MiniMenuAction};
use client::io::{ClientProt377, PacketObserver};
use client::render::backend::FrameOutput;
use client::render::Renderer;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "support/live_engine_377.rs"]
mod live_engine_377;

const COPPER_ORE: i32 = 436;
const TIN_ORE: i32 = 438;
const BRONZE_BAR: i32 = 2349;
const BRONZE_DAGGER: i32 = 1205;
const HAMMER: i32 = 2347;
const RAW_SHRIMP: i32 = 317;
const COOKED_SHRIMP: i32 = 315;
const BURNT_FISH: i32 = 323;
const KNIFE: i32 = 946;
const LOGS: i32 = 1511;
const WOOL: i32 = 1737;
const BALL_OF_WOOL: i32 = 1759;
const SPINNING_WHEEL: i32 = 2644;
const STAIRS_UP: i32 = 1738;
const STAIRS_DOWN: i32 = 1739;
const INVENTORY: usize = 3214;
const COOKING: usize = 7;
const CRAFTING: usize = 12;
const SMITHING: usize = 13;
const BRONZE_BAR_XP: i32 = 62;
const BRONZE_DAGGER_XP: i32 = 125;
const SHRIMP_XP: i32 = 300;
const WOOL_XP: i32 = 25;

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

#[derive(Clone, Copy)]
struct LoadedLocation {
    id: i32,
    code: i32,
    x: i32,
    z: i32,
    layer: usize,
    option_index: Option<usize>,
}

fn tile(c: &Client) -> (i32, i32) {
    let player = c
        .local_player
        .as_ref()
        .expect("server-populated local player");
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
    let Some(inventory) = c.if_(INVENTORY) else {
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

fn inventory_free_slots(c: &Client) -> usize {
    c.if_(INVENTORY)
        .and_then(|inventory| inventory.link_obj_type.as_ref())
        .map(|objects| {
            objects
                .iter()
                .filter(|object_plus_one| **object_plus_one == 0)
                .count()
        })
        .unwrap_or(0)
}

fn inventory_slot(c: &Client, object: i32) -> i32 {
    let inventory = c.if_(INVENTORY).expect("server-populated player inventory");
    inventory
        .link_obj_type
        .as_ref()
        .expect("player inventory object slots")
        .iter()
        .position(|id| *id == object + 1)
        .unwrap_or_else(|| panic!("object {object} must occupy an actual server inventory slot"))
        as i32
}

fn named_inventory(c: &Client) -> Vec<Value> {
    let Some(inventory) = c.if_(INVENTORY) else {
        return Vec::new();
    };
    let (Some(objects), Some(counts)) = (
        inventory.link_obj_type.as_ref(),
        inventory.link_obj_number.as_ref(),
    ) else {
        return Vec::new();
    };
    objects
        .iter()
        .zip(counts)
        .enumerate()
        .filter_map(|(slot, (&object_plus_one, &count))| {
            let object = object_plus_one - 1;
            (object >= 0 && count > 0).then(|| {
                json!({"slot":slot,"object":object,
                    "name":c.cache.objs.get(object as usize).map(|obj|obj.name.as_str()),
                    "count":count})
            })
        })
        .collect()
}

fn generations(c: &Client) -> Value {
    json!({
        "npc":c.gens.npc,"player":c.gens.player,"inv":c.gens.inv,
        "varp":c.gens.varp,"stat":c.gens.stat,"chat":c.gens.chat,
        "scene":c.gens.scene,"iface":c.gens.iface,"camera":c.gens.camera,
        "map_flag":c.gens.map_flag,"world":c.gens.world,
        "player_info":c.gens.player_info,"npc_info":c.gens.npc_info,
        "session":c.gens.session,"invalidations":c.gens.invalidations
    })
}

fn find_location(
    c: &Client,
    location_id: Option<i32>,
    exact_name: Option<&str>,
    option: Option<&str>,
    near: Option<(i32, i32, i32)>,
) -> Option<LoadedLocation> {
    let player = tile(c);
    let mut found = Vec::new();
    for x in 0..104 {
        for z in 0..104 {
            let world_x = x + c.map_build_base_x;
            let world_z = z + c.map_build_base_z;
            if near.is_some_and(|(center_x, center_z, radius)| {
                (world_x - center_x).abs() + (world_z - center_z).abs() > radius
            }) {
                continue;
            }
            let codes = [
                c.world.wall_type(c.minusedlevel, x, z),
                c.world.decor_type(c.minusedlevel, x, z),
                c.world.scene_type(c.minusedlevel, x, z),
                c.world.gd_type(c.minusedlevel, x, z),
            ];
            for (layer, code) in codes.into_iter().enumerate() {
                if code == 0 {
                    continue;
                }
                let id = (code >> 14) & 0x7fff;
                if location_id.is_some_and(|expected| expected != id) {
                    continue;
                }
                let Some(loc) = c.cache.resolve_loc(id as usize, &c.var) else {
                    continue;
                };
                if exact_name.is_some_and(|name| !loc.name.eq_ignore_ascii_case(name)) {
                    continue;
                }
                let option_index = match option {
                    Some(option) => loc
                        .op
                        .iter()
                        .position(|candidate| candidate.as_deref() == Some(option)),
                    None => None,
                };
                if option.is_some() && option_index.is_none() {
                    continue;
                }
                found.push((
                    LoadedLocation {
                        id,
                        code,
                        x: world_x,
                        z: world_z,
                        layer,
                        option_index,
                    },
                    loc.name.clone(),
                ));
            }
        }
    }
    found
        .into_iter()
        .min_by_key(|(location, _)| (location.x - player.0).abs() + (location.z - player.1).abs())
        .map(|(location, _)| location)
}

fn location_animation(c: &Client) -> Value {
    let Some(location) = find_location(c, Some(SPINNING_WHEEL), None, None, None) else {
        return Value::Null;
    };
    let local_x = location.x - c.map_build_base_x;
    let local_z = location.z - c.map_build_base_z;
    let animation = match location.layer {
        0 => c
            .world
            .get_wall(c.minusedlevel, local_x, local_z)
            .map(|object| (object.anim_seq, object.anim_shape, object.anim_angle)),
        1 => c
            .world
            .get_decor(c.minusedlevel, local_x, local_z)
            .map(|object| (object.anim_seq, object.anim_shape, object.anim_angle)),
        2 => c
            .world
            .get_scene(c.minusedlevel, local_x, local_z)
            .map(|object| (object.anim_seq, object.anim_shape, object.anim_angle)),
        3 => c
            .world
            .get_gd(c.minusedlevel, local_x, local_z)
            .map(|object| (object.anim_seq, object.anim_shape, object.anim_angle)),
        _ => None,
    };
    let Some((anim_seq, anim_shape, anim_angle)) = animation else {
        return Value::Null;
    };
    json!({"location":location.id,"typecode":location.code,
        "tile":[location.x,location.z],"plane":c.minusedlevel,
        "layer":location.layer,"anim_seq":anim_seq,
        "anim_shape":anim_shape,"anim_angle":anim_angle})
}

fn full_state(c: &Client, packets: &[Value]) -> Value {
    let player = c
        .local_player
        .as_ref()
        .expect("server-populated local player");
    let components = c
        .ifaces_merged()
        .map(|component| {
            json!({
                "id":component.id,"layer":component.layer_id,
                "text":component.text,"button_text":component.button_text,
                "button_type":component.button_type,"colour":component.colour,
                "objects":component.link_obj_type,"counts":component.link_obj_number,
                "hide":component.hide,"scroll_pos":component.scroll_pos,
                "scroll_height":component.scroll_height,"x":component.x,"y":component.y,
                "width":component.width,"height":component.height
            })
        })
        .collect::<Vec<_>>();
    let inventories = c
        .ifaces_merged()
        .filter(|component| {
            component.link_obj_type.is_some() && component.link_obj_number.is_some()
        })
        .map(|component| {
            json!({"component":component.id,"layer":component.layer_id,
                "objects":component.link_obj_type,"counts":component.link_obj_number})
        })
        .collect::<Vec<_>>();
    json!({
        "connected":c.ingame,"scene_state":c.scene_state,
        "main_modal":c.main_modal_id,"side_modal":c.side_modal_id,
        "chat_modal":c.chat_modal_id,"main_interface":interface_text(c,c.main_modal_id),
        "side_interface":interface_text(c,c.side_modal_id),
        "chat_interface":interface_text(c,c.chat_modal_id),
        "map_base":[c.map_build_base_x,c.map_build_base_z],
        "tile":tile(c),"plane":c.minusedlevel,"cycle":c.loop_cycle,
        "varps":c.var,"server_varps":c.var_serv,
        "inventories":inventories,"components":components,
        "inventory_named":named_inventory(c),
        "stat_xp":c.stat_xp,"stat_levels":c.stat_base_level,
        "generations":generations(c),"outbound_packets":packets,
        "player":{
            "fine_position":[player.x,player.z],
            "route_length":player.route_length,"route_x":player.route_x,
            "route_z":player.route_z,"sequence":player.entity.primary_anim,
            "sequence_frame":player.entity.primary_anim_frame,
            "sequence_cycle":player.entity.primary_anim_cycle,
            "sequence_delay":player.entity.primary_anim_delay,
            "spotanim_id":player.entity.spotanim_id,
            "spotanim_frame":player.entity.spotanim_frame,
            "spotanim_height":player.entity.spotanim_height
        },
        "scene_animation":location_animation(c),
        "dialog_input_open":c.dialog_input_open,"dialog_input":c.dialog_input,
        "resumed_pause_button":c.resumed_pause_button
    })
}

fn action_state(c: &Client) -> Value {
    json!({
        "tile":tile(c),"plane":c.minusedlevel,"cycle":c.loop_cycle,
        "main_modal":c.main_modal_id,"side_modal":c.side_modal_id,
        "chat_modal":c.chat_modal_id,
        "items":{
            "copper_ore":inventory_count(c,COPPER_ORE),
            "tin_ore":inventory_count(c,TIN_ORE),
            "bronze_bar":inventory_count(c,BRONZE_BAR),
            "bronze_dagger":inventory_count(c,BRONZE_DAGGER),
            "hammer":inventory_count(c,HAMMER),"raw_shrimp":inventory_count(c,RAW_SHRIMP),
            "cooked_shrimp":inventory_count(c,COOKED_SHRIMP),
            "burnt_fish":inventory_count(c,BURNT_FISH),"knife":inventory_count(c,KNIFE),
            "logs":inventory_count(c,LOGS),"wool":inventory_count(c,WOOL),
            "ball_of_wool":inventory_count(c,BALL_OF_WOOL)
        },
        "inventory_named":named_inventory(c),
        "stat_xp":c.stat_xp,"stat_levels":c.stat_base_level,
        "generations":generations(c)
    })
}

fn packet_count(outbound: &Outbound) -> usize {
    outbound.0.lock().unwrap().len()
}

fn packets_since(outbound: &Outbound, start: usize) -> Vec<Value> {
    outbound
        .0
        .lock()
        .unwrap()
        .iter()
        .skip(start)
        .cloned()
        .collect()
}

fn write_json(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn persist_array(path: &Path, values: &[Value]) {
    write_json(path, &Value::Array(values.to_vec()));
}

fn pump_until(
    c: &mut Client,
    r: &mut Renderer,
    seconds: u64,
    context: &str,
    mut ready: impl FnMut(&Client) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        c.shell.latch_click();
        c.mainloop();
        assert!(
            c.ingame,
            "live production 377 session ended: {:?}",
            c.take_session_exit_reason()
        );
        drop(r.mainredraw(c));
        if ready(c) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "live production 377 deadline ({context}): {}",
        action_state(c)
    );
}

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xedb8_8320
            };
        }
    }
    !crc
}

fn png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    png.extend_from_slice(&png_crc32(&png[start..]).to_be_bytes());
}

fn lossless_png(pixels: &[i32], width: usize, height: usize) -> Vec<u8> {
    use std::io::Write;
    assert_eq!(pixels.len(), width * height);
    let mut scanlines = Vec::with_capacity(height * (width * 3 + 1));
    for row in pixels.chunks_exact(width) {
        scanlines.push(0);
        for &pixel in row {
            scanlines.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&scanlines).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    png_chunk(&mut png, b"IHDR", &header);
    png_chunk(&mut png, b"IDAT", &compressed);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

fn viewport_pixels(pixels: &[i32], width: usize, height: usize) -> Vec<i32> {
    assert!(
        width >= 516 && height >= 338,
        "full frame contains the 512x334 viewport"
    );
    let mut viewport = Vec::with_capacity(512 * 334);
    for y in 4..338 {
        viewport.extend_from_slice(&pixels[y * width + 4..y * width + 516]);
    }
    viewport
}

fn capture_stage(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    outbound: &Outbound,
    stages: &mut Vec<Value>,
    name: &str,
) {
    let FrameOutput::PixMap(map) = r.mainredraw(c) else {
        panic!("CPU proof required");
    };
    let full_path = root.join(format!("rust-{name}.png"));
    std::fs::write(
        &full_path,
        lossless_png(&map.pixels, map.width as usize, map.height as usize),
    )
    .unwrap();
    let raw_path = root.join(format!("rust-{name}-raw512x334.png"));
    let viewport = viewport_pixels(&map.pixels, map.width as usize, map.height as usize);
    std::fs::write(&raw_path, lossless_png(&viewport, 512, 334)).unwrap();
    let record = json!({
        "stage":name,"screenshot":full_path,"raw512x334":raw_path,
        "state":full_state(c,&packets_since(outbound,0))
    });
    write_json(&root.join(format!("{name}.json")), &record);
    stages.push(record.clone());
    persist_array(&root.join("stages.json"), stages);
    eprintln!("377 production stage {record}");
}

fn push_action(
    root: &Path,
    actions: &mut Vec<Value>,
    outbound: &Outbound,
    name: &str,
    before: Value,
    after: Value,
    packet_start: usize,
    details: Value,
) {
    let receipt = json!({
        "action":name,"details":details,"before":before,"after":after,
        "packets":packets_since(outbound,packet_start)
    });
    actions.push(receipt.clone());
    persist_array(&root.join("actions.json"), actions);
    eprintln!("377 production action {receipt}");
}

fn record_intervention(root: &Path, interventions: &mut Vec<Value>, intervention: Value) {
    interventions.push(intervention);
    persist_array(&root.join("interventions.json"), interventions);
}

fn admit_cheat(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    interventions: &mut Vec<Value>,
    prepare: bool,
    command: String,
    label: &str,
    mut ready: impl FnMut(&Client) -> bool,
) {
    assert!(
        prepare,
        "inventory/stat seeding requires CLIENT_377_PRODUCTION_PREPARE=1"
    );
    c.set_cheat_admission(CheatAdmission::Granted);
    let sent = c.send_cheat(&command).is_sent();
    c.set_cheat_admission(CheatAdmission::Denied);
    assert!(sent, "admitted test-account cheat was sent: {label}");
    pump_until(c, r, 15, label, |client| ready(client));
    record_intervention(
        root,
        interventions,
        json!({"label":label,"method":"admitted CLIENT_CHEAT to selected test account",
            "command":command,"result":"server-applied state observed"}),
    );
}

fn ensure_item(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    interventions: &mut Vec<Value>,
    prepare: bool,
    object: i32,
    cheat_name: &str,
    needed: i32,
) {
    let before = inventory_count(c, object);
    if before >= needed {
        return;
    }
    let quantity = needed - before;
    assert!(
        inventory_free_slots(c) >= quantity as usize,
        "inventory has room for explicit {cheat_name} inputs; free slots={}, requested={quantity}",
        inventory_free_slots(c)
    );
    let command = format!("give {cheat_name} {quantity}");
    admit_cheat(
        c,
        r,
        root,
        interventions,
        prepare,
        command,
        "seed only input inventory items",
        |client| inventory_count(client, object) >= needed,
    );
    interventions
        .last_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("object".into(), json!(object));
    interventions
        .last_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("before_count".into(), json!(before));
    interventions
        .last_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("after_count".into(), json!(inventory_count(c, object)));
    persist_array(&root.join("interventions.json"), interventions);
}

fn prepare_smithing_baseline(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    interventions: &mut Vec<Value>,
    prepare: bool,
) {
    let level = c.stat_base_level[SMITHING];
    let xp = c.stat_xp[SMITHING];
    if prepare && (level != 1 || xp != 0) {
        admit_cheat(
            c,
            r,
            root,
            interventions,
            true,
            "setstat smithing 1".to_owned(),
            "reset only Smithing to a level-one, zero-XP baseline",
            |client| client.stat_base_level[SMITHING] == 1 && client.stat_xp[SMITHING] == 0,
        );
        let intervention = interventions.last_mut().unwrap().as_object_mut().unwrap();
        intervention.insert("before_level".into(), json!(level));
        intervention.insert("before_xp".into(), json!(xp));
        intervention.insert("after_level".into(), json!(c.stat_base_level[SMITHING]));
        intervention.insert("after_xp".into(), json!(c.stat_xp[SMITHING]));
        persist_array(&root.join("interventions.json"), interventions);
    }
    assert_eq!(
        c.stat_base_level[SMITHING], 1,
        "level-up proof requires Smithing level 1; use PREPARE=1 only for the admitted baseline reset"
    );
    if !prepare {
        assert_eq!(
            c.stat_xp[SMITHING],
            0,
            "all-phase Smithing proof needs 0 starting XP; use PREPARE=1 for the recorded baseline reset"
        );
    }
}
fn component_origin(c: &Client, root: i32) -> (i32, i32) {
    if root == c.main_modal_id {
        (4, 4)
    } else if root == c.side_modal_id {
        (553, 205)
    } else if root == c.chat_modal_id {
        (17, 357)
    } else {
        panic!("open interface {root} has a known rendered origin")
    }
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

fn click(c: &mut Client, r: &mut Renderer, x: i32, y: i32) {
    c.shell.apply_mouse_move(x, y);
    drop(r.mainredraw(c));
    c.shell.apply_mouse_down(1, x, y);
    c.shell.latch_click();
    c.mainloop();
    assert!(
        c.ingame,
        "production component click kept the session connected"
    );
    drop(r.mainredraw(c));
    c.shell.apply_mouse_up();
    c.shell.latch_click();
    c.mainloop();
    assert!(
        c.ingame,
        "production component release kept the session connected"
    );
    drop(r.mainredraw(c));
}

fn click_component(c: &mut Client, r: &mut Renderer, root: i32, target: i32) {
    let origin = component_origin(c, root);
    let (x, y, width, height) = component_rect(c, root, target, origin.0, origin.1)
        .expect("button belongs to the rendered interface");
    assert!(width > 0 && height > 0);
    click(c, r, x + width / 2, y + height / 2);
}

fn click_button(c: &mut Client, r: &mut Renderer, root: i32, label: &str) -> i32 {
    let matches = c
        .ifaces_merged()
        .filter(|component| {
            component.layer_id == root
                && (component.button_text == label || component.text == label)
                && component.button_type != 0
        })
        .map(|component| component.id)
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "one rendered {label:?} button in layer {root}"
    );
    click_component(c, r, root, matches[0]);
    matches[0]
}

fn has_continue_button(c: &Client) -> bool {
    c.chat_modal_id >= 0
        && c.ifaces_merged().any(|component| {
            component.layer_id == c.chat_modal_id
                && (component.button_text == "Click here to continue"
                    || component.text == "Click here to continue")
        })
}

fn click_pause_continue(c: &mut Client, r: &mut Renderer) -> i32 {
    assert!(
        has_continue_button(c),
        "server pause exposes its real Continue button"
    );
    click_button(c, r, c.chat_modal_id, "Click here to continue")
}

fn handle_levelup_pause(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    outbound: &Outbound,
    stages: &mut Vec<Value>,
    stage_name: &str,
    previous_level: i32,
) -> bool {
    if c.stat_base_level[SMITHING] <= previous_level || !has_continue_button(c) {
        return false;
    }
    let player = c.local_player.as_ref().unwrap();
    assert_ne!(
        player.entity.spotanim_id, -1,
        "level-up firework was received"
    );
    assert_eq!(
        player.entity.spotanim_height, 124,
        "level-up firework height"
    );
    capture_stage(c, r, root, outbound, stages, stage_name);
    let button = click_pause_continue(c, r);
    pump_until(c, r, 15, "resume server level-up pause", |client| {
        client.chat_modal_id < 0 && client.resumed_pause_button
    });
    eprintln!("377 production resumed level-up through component {button}");
    true
}

fn walk_waypoint(
    c: &mut Client,
    r: &mut Renderer,
    requested: (i32, i32),
    actions: &mut Vec<Value>,
) {
    pump_until(c, r, 30, "scene loaded for movement", |client| {
        client.scene_state == 2
    });
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
        .expect("production path has endpoint");
    let target = (
        endpoint.0 + c.map_build_base_x,
        endpoint.1 + c.map_build_base_z,
    );
    assert!(
        (target.0 - requested.0)
            .abs()
            .max((target.1 - requested.1).abs())
            <= 1,
        "production path approaches requested tile: requested={requested:?} target={target:?}"
    );
    pump_until(c, r, 45, "server-applied player route", |client| {
        tile(client) == target
    });
    actions.push(
        json!({"walk_from":before,"requested":requested,"arrived":tile(c),
        "input":"production Client::tryMove game-click pathfinder; received player position"}),
    );
}

fn walk_route(c: &mut Client, r: &mut Renderer, target: (i32, i32), actions: &mut Vec<Value>) {
    for _ in 0..100 {
        let current = tile(c);
        let dx = target.0 - current.0;
        let dz = target.1 - current.1;
        if dx == 0 && dz == 0 {
            return;
        }
        let distance = dx.abs().max(dz.abs());
        let step = distance.min(6);
        let next = (
            current.0 + dx * step / distance,
            current.1 + dz * step / distance,
        );
        walk_waypoint(c, r, next, actions);
    }
    assert_eq!(
        tile(c),
        target,
        "production route reached its content waypoint"
    );
}

fn walk_near(c: &mut Client, r: &mut Renderer, target: (i32, i32), actions: &mut Vec<Value>) {
    for _ in 0..100 {
        let current = tile(c);
        if (current.0 - target.0)
            .abs()
            .max((current.1 - target.1).abs())
            <= 1
        {
            return;
        }
        let dx = target.0 - current.0;
        let dz = target.1 - current.1;
        let distance = dx.abs().max(dz.abs());
        let step = distance.min(6);
        let next = (
            current.0 + dx * step / distance,
            current.1 + dz * step / distance,
        );
        walk_waypoint(c, r, next, actions);
    }
    panic!(
        "production path approached location tile {target:?} from {:?}",
        tile(c)
    );
}

const LUMBRIDGE_ROUTE: &[(i32, i32)] = &[
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

fn ensure_lumbridge(c: &mut Client, r: &mut Renderer, actions: &mut Vec<Value>) {
    assert_eq!(
        c.minusedlevel, 0,
        "mainland production route starts on plane 0"
    );
    let current = tile(c);
    if (3200..=3211).contains(&current.0) && (3205..=3215).contains(&current.1) {
        walk_route(c, r, (3208, 3213), actions);
        leave_kitchen(c, r, actions);
        walk_route(c, r, (3210, 3247), actions);
        return;
    }
    if (current.0 - 3210).abs().max((current.1 - 3247).abs()) <= 35 {
        walk_route(c, r, (3210, 3247), actions);
        return;
    }
    let (route_index, route_distance) = LUMBRIDGE_ROUTE
        .iter()
        .enumerate()
        .map(|(index, waypoint)| {
            (
                index,
                (waypoint.0 - current.0).abs() + (waypoint.1 - current.1).abs(),
            )
        })
        .min_by_key(|(_, distance)| *distance)
        .unwrap();
    assert!(
        route_distance <= 45,
        "selected mainland account is on the Draynor-Lumbridge route: {current:?}"
    );
    for &waypoint in LUMBRIDGE_ROUTE.iter().skip(route_index + 1) {
        walk_waypoint(c, r, waypoint, actions);
    }
    walk_route(c, r, (3210, 3247), actions);
}

fn operate_location(c: &mut Client, location: LoadedLocation, option_index: usize) -> Value {
    let loc = c
        .cache
        .resolve_loc(location.id as usize, &c.var)
        .expect("actual server-loaded location type");
    let option = loc
        .op
        .get(option_index)
        .and_then(Option::as_deref)
        .expect("selected location action exists");
    let action = [
        MiniMenuAction::OP_LOC1,
        MiniMenuAction::OP_LOC2,
        MiniMenuAction::OP_LOC3,
        MiniMenuAction::OP_LOC4,
        MiniMenuAction::OP_LOC5,
    ][option_index];
    let expected_option = format!("{option} @cya@{}", loc.name);
    let receipt = json!({"location":location.id,"typecode":location.code,
        "tile":[location.x,location.z],"option":option,"option_index":option_index,
        "input":"production scene minimenu and doAction"});
    c.menu_num_entries = 0;
    c.pick_count = 1;
    c.pick_typecodes[0] = location.code;
    c.add_world_options();
    let entry = (0..c.menu_num_entries.max(0) as usize)
        .find(|&entry| {
            c.menu_action[entry] == action
                && c.menu_param_a[entry] == location.code
                && c.menu_option[entry] == expected_option
        })
        .unwrap_or_else(|| panic!("actual location minimenu exposes {expected_option:?}"));
    c.doAction(entry as i32);
    receipt
}

fn open_passage(
    c: &mut Client,
    r: &mut Renderer,
    name: &str,
    position: (i32, i32),
    actions: &mut Vec<Value>,
) {
    if let Some(closed) = find_location(
        c,
        None,
        Some(name),
        Some("Open"),
        Some((position.0, position.1, 0)),
    ) {
        actions.push(operate_location(c, closed, closed.option_index.unwrap()));
        pump_until(c, r, 30, "door opened by server", |client| {
            find_location(
                client,
                None,
                Some(name),
                Some("Open"),
                Some((position.0, position.1, 0)),
            )
            .is_none()
                && find_location(
                    client,
                    None,
                    Some(name),
                    Some("Close"),
                    Some((position.0, position.1, 3)),
                )
                .is_some()
        });
    } else {
        assert!(
            find_location(
                c,
                None,
                Some(name),
                Some("Close"),
                Some((position.0, position.1, 3)),
            )
            .is_some(),
            "actual {name} at {position:?} is already open"
        );
    }
}

fn approach_cook(c: &mut Client, r: &mut Renderer, actions: &mut Vec<Value>) {
    if tile(c).1 > 3222 {
        walk_route(c, r, (3220, 3244), actions);
    }
    walk_route(c, r, (3220, 3222), actions);
    walk_route(c, r, (3220, 3218), actions);
    open_passage(c, r, "Large door", (3217, 3218), actions);
    walk_route(c, r, (3215, 3218), actions);
    walk_route(c, r, (3215, 3212), actions);
    open_passage(c, r, "Door", (3215, 3211), actions);
    walk_route(c, r, (3215, 3210), actions);
    walk_route(c, r, (3208, 3210), actions);
    open_passage(c, r, "Door", (3208, 3211), actions);
    walk_route(c, r, (3208, 3213), actions);
}

fn leave_kitchen(c: &mut Client, r: &mut Renderer, actions: &mut Vec<Value>) {
    open_passage(c, r, "Door", (3208, 3211), actions);
    walk_route(c, r, (3208, 3210), actions);
    walk_route(c, r, (3215, 3210), actions);
    open_passage(c, r, "Door", (3215, 3211), actions);
    walk_route(c, r, (3215, 3218), actions);
    open_passage(c, r, "Large door", (3217, 3218), actions);
    walk_route(c, r, (3220, 3218), actions);
    walk_route(c, r, (3220, 3244), actions);
}

fn select_item(c: &mut Client, object: i32) -> Value {
    let slot = inventory_slot(c, object);
    c.menu_action[0] = MiniMenuAction::USEHELD_START;
    c.menu_param_a[0] = object;
    c.menu_param_b[0] = slot;
    c.menu_param_c[0] = INVENTORY as i32;
    c.doAction(0);
    assert_eq!(c.use_mode, 1, "production Use-item selection armed");
    json!({"object":object,"slot":slot,"component":INVENTORY,
        "name":c.cache.objs[object as usize].name,
        "input":"production doAction(USEHELD_START)"})
}

fn use_item_on_location(c: &mut Client, object: i32, location: LoadedLocation) -> Value {
    let source = select_item(c, object);
    let receipt = json!({"source":source,"location":location.id,
        "typecode":location.code,"tile":[location.x,location.z],
        "name":c.cache.resolve_loc(location.id as usize,&c.var).unwrap().name,
        "input":"production doAction(USEHELD_START), doAction(USEHELD_ONLOC) on actual scene typecode"});
    c.menu_action[0] = MiniMenuAction::USEHELD_ONLOC;
    c.menu_param_a[0] = location.code;
    c.menu_param_b[0] = location.x - c.map_build_base_x;
    c.menu_param_c[0] = location.z - c.map_build_base_z;
    c.doAction(0);
    assert_eq!(c.use_mode, 0, "production item-on-location action disarmed");
    receipt
}

fn use_item_on_item(c: &mut Client, source_object: i32, target_object: i32) -> Value {
    let source = select_item(c, source_object);
    let target_slot = inventory_slot(c, target_object);
    c.menu_action[0] = MiniMenuAction::USEHELD_ONHELD;
    c.menu_param_a[0] = target_object;
    c.menu_param_b[0] = target_slot;
    c.menu_param_c[0] = INVENTORY as i32;
    c.doAction(0);
    assert_eq!(c.use_mode, 0, "production item-on-item action disarmed");
    json!({"source":source,"target":{"object":target_object,"slot":target_slot,
        "component":INVENTORY},
        "input":"production doAction(USEHELD_START), doAction(USEHELD_ONHELD)"})
}

fn click_smithing_product(c: &mut Client, r: &mut Renderer, root: i32, object: i32) -> Value {
    let slots = c
        .ifaces_merged()
        .filter(|component| {
            component.layer_id == root
                && component
                    .link_obj_type
                    .as_ref()
                    .is_some_and(|objects| objects.contains(&(object + 1)))
        })
        .filter_map(|component| {
            let slot = component
                .link_obj_type
                .as_ref()?
                .iter()
                .position(|id| *id == object + 1)?;
            Some((component.id, slot as i32))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        slots.len(),
        1,
        "one smithing product slot for item {object}"
    );
    let (component, slot) = slots[0];
    let item_component = c.if_(component as usize).unwrap();
    assert!(
        item_component.width > 0,
        "smithing product grid has a positive column count"
    );
    let origin = component_origin(c, root);
    let (x, y, _, _) = component_rect(c, root, component, origin.0, origin.1)
        .expect("product slot belongs to smithing interface");
    let column = slot % item_component.width;
    let row = slot / item_component.width;
    let background_x = if slot < 20 {
        item_component
            .inv_background_x
            .as_ref()
            .and_then(|offsets| offsets.get(slot as usize))
            .copied()
            .unwrap_or(0)
    } else {
        0
    };
    let background_y = if slot < 20 {
        item_component
            .inv_background_y
            .as_ref()
            .and_then(|offsets| offsets.get(slot as usize))
            .copied()
            .unwrap_or(0)
    } else {
        0
    };
    let (screen_x, screen_y) = (
        x + column * (item_component.margin_x + 32) + background_x + 16,
        y + row * (item_component.margin_y + 32) + background_y + 16,
    );
    c.shell.apply_mouse_move(screen_x, screen_y);
    drop(r.mainredraw(c));
    let entries = (0..c.menu_num_entries.max(0) as usize)
        .filter(|&entry| {
            c.menu_action[entry] == MiniMenuAction::INV_BUTTON1
                && c.menu_param_a[entry] == object
                && c.menu_param_b[entry] == slot
                && c.menu_param_c[entry] == component
                && c.menu_option[entry]
                    .to_ascii_lowercase()
                    .contains(&c.cache.objs[object as usize].name.to_ascii_lowercase())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        1,
        "production menu offers the actual one-item product"
    );
    let entry = entries[0];
    let receipt = json!({"component":component,"object":object,"slot":slot,
        "menu_option":c.menu_option[entry],"menu_action":c.menu_action[entry],
        "input":"rendered smithing product slot and production doAction(INV_BUTTON1)"});
    c.doAction(entry as i32);
    receipt
}

fn enter_amount(c: &mut Client, r: &mut Renderer, amount: i32) -> Value {
    pump_until(
        c,
        r,
        15,
        "server-opened production count prompt",
        |client| client.dialog_input_open,
    );
    assert!(c.dialog_input.is_empty(), "count prompt starts empty");
    let digits = amount.to_string();
    for digit in digits.bytes() {
        c.shell.apply_key(true, 0, i32::from(digit));
        c.shell.latch_click();
        c.mainloop();
        assert!(c.ingame, "count input kept live session connected");
        drop(r.mainredraw(c));
    }
    assert_eq!(
        c.dialog_input, digits,
        "typed amount reaches actual input field"
    );
    c.shell.apply_key(true, 0, 13);
    c.shell.latch_click();
    c.mainloop();
    assert!(c.ingame, "submitting amount kept live session connected");
    drop(r.mainredraw(c));
    assert!(
        !c.dialog_input_open,
        "Enter submitted the actual P_COUNTDIALOG"
    );
    json!({"prompt":"server-opened P_COUNTDIALOG",
        "input":"GameShell digit key events followed by Enter through Client::handle_chat_input",
        "amount":amount})
}

fn cook_range(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    outbound: &Outbound,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
    range: LoadedLocation,
) {
    let raw_start = inventory_count(c, RAW_SHRIMP);
    assert!(
        (3..=27).contains(&raw_start),
        "Cook X plus Cook All uses 3..=27 raw shrimps"
    );
    let cooking_level_before = c.stat_base_level[COOKING];
    let before = action_state(c);
    let packet_start = packet_count(outbound);
    let use_receipt = use_item_on_location(c, RAW_SHRIMP, range);
    pump_until(
        c,
        r,
        15,
        "server Cook X/Cook All quantity interface",
        |client| {
            client.chat_modal_id >= 0
                && interface_text(client, client.chat_modal_id)
                    .contains("How many would you like to cook?")
        },
    );
    capture_stage(c, r, root, outbound, stages, "cook-x-and-all-menu");
    let x_button = click_button(c, r, c.chat_modal_id, "Cook X");
    pump_until(c, r, 15, "Cook X opens server count dialog", |client| {
        client.dialog_input_open
    });
    capture_stage(c, r, root, outbound, stages, "cook-x-count-prompt");
    let prompt = enter_amount(c, r, 1);
    let cooked_before_x = inventory_count(c, COOKED_SHRIMP);
    let burnt_before_x = inventory_count(c, BURNT_FISH);
    let xp_before_x = c.stat_xp[COOKING];
    pump_until(
        c,
        r,
        45,
        "Cook X produced exactly one cooked or burnt item",
        |client| {
            inventory_count(client, RAW_SHRIMP) == raw_start - 1
                && inventory_count(client, COOKED_SHRIMP) + inventory_count(client, BURNT_FISH)
                    == cooked_before_x + burnt_before_x + 1
        },
    );
    let cooked_delta_x = inventory_count(c, COOKED_SHRIMP) - cooked_before_x;
    assert!(matches!(cooked_delta_x, 0 | 1));
    assert_eq!(
        c.stat_xp[COOKING] - xp_before_x,
        cooked_delta_x * SHRIMP_XP,
        "cooking XP matches the actual product script success"
    );
    let cooked_levelup =
        if c.stat_base_level[COOKING] > cooking_level_before && has_continue_button(c) {
            capture_stage(c, r, root, outbound, stages, "cook-levelup-after-x");
            let button = click_pause_continue(c, r);
            pump_until(c, r, 15, "resume Cook X level-up", |client| {
                client.chat_modal_id < 0 && client.resumed_pause_button
            });
            eprintln!("377 production resumed Cook X level-up through component {button}");
            true
        } else {
            false
        };
    let x_after = action_state(c);
    push_action(
        root,
        actions,
        outbound,
        "cook-x-one",
        before,
        x_after,
        packet_start,
        json!({"input":use_receipt,"button_component":x_button,
            "amount_prompt":prompt,"requested_amount":1,
            "cooked_delta":cooked_delta_x,
            "burnt_delta":1-cooked_delta_x,"xp_per_success":SHRIMP_XP,
            "levelup":cooked_levelup}),
    );
    capture_stage(c, r, root, outbound, stages, "cook-x-one-complete");

    pump_until(
        c,
        r,
        15,
        "Cook X pause closed before second production action",
        |client| client.chat_modal_id < 0 && !client.dialog_input_open,
    );
    let remaining = inventory_count(c, RAW_SHRIMP);
    assert!(
        remaining >= 2,
        "Cook All runs while multiple raw shrimps remain"
    );
    let before_all = action_state(c);
    let packet_start_all = packet_count(outbound);
    let all_use = use_item_on_location(c, RAW_SHRIMP, range);
    pump_until(c, r, 15, "server Cook All quantity interface", |client| {
        client.chat_modal_id >= 0
            && interface_text(client, client.chat_modal_id)
                .contains("How many would you like to cook?")
    });
    capture_stage(c, r, root, outbound, stages, "cook-all-menu");
    let all_button = click_button(c, r, c.chat_modal_id, "Cook All");
    let cooked_before_all = inventory_count(c, COOKED_SHRIMP);
    let burnt_before_all = inventory_count(c, BURNT_FISH);
    let xp_before_all = c.stat_xp[COOKING];
    let mut levelup_number = 0;
    loop {
        if c.stat_base_level[COOKING] > cooking_level_before && has_continue_button(c) {
            let stage_name = format!("cook-levelup-{}", levelup_number + 1);
            capture_stage(c, r, root, outbound, stages, &stage_name);
            let button = click_pause_continue(c, r);
            pump_until(c, r, 15, "resume cooking level-up", |client| {
                client.chat_modal_id < 0 && client.resumed_pause_button
            });
            levelup_number += 1;
            eprintln!("377 production resumed cooking level-up through component {button}");
            continue;
        }
        if inventory_count(c, RAW_SHRIMP) == 0 {
            break;
        }
        pump_until(c, r, 30, "Cook All product or level-up pause", |client| {
            inventory_count(client, RAW_SHRIMP) == 0
                || (client.stat_base_level[COOKING] > cooking_level_before
                    && has_continue_button(client))
        });
    }
    assert_eq!(inventory_count(c, RAW_SHRIMP), 0);
    let cooked_delta_all = inventory_count(c, COOKED_SHRIMP) - cooked_before_all;
    let burnt_delta_all = inventory_count(c, BURNT_FISH) - burnt_before_all;
    assert_eq!(cooked_delta_all + burnt_delta_all, remaining);
    assert_eq!(
        c.stat_xp[COOKING] - xp_before_all,
        cooked_delta_all * SHRIMP_XP,
        "Cook All XP matches exactly the received successful products"
    );
    let all_after = action_state(c);
    push_action(
        root,
        actions,
        outbound,
        "cook-all-remaining",
        before_all,
        all_after,
        packet_start_all,
        json!({"input":all_use,"button_component":all_button,
            "requested_amount":remaining,"cooked_delta":cooked_delta_all,
            "burnt_delta":burnt_delta_all,"xp_per_success":SHRIMP_XP,
            "cooking_levelups":levelup_number}),
    );
    capture_stage(c, r, root, outbound, stages, "cook-all-complete");
}

fn smelt_once(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    outbound: &Outbound,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
    interventions: &mut Vec<Value>,
    prepare: bool,
    furnace: LoadedLocation,
    action_name: &str,
    levelup_stage_name: &str,
) -> bool {
    ensure_item(
        c,
        r,
        root,
        interventions,
        prepare,
        COPPER_ORE,
        "copper_ore",
        1,
    );
    ensure_item(c, r, root, interventions, prepare, TIN_ORE, "tin_ore", 1);
    let before = action_state(c);
    let packet_start = packet_count(outbound);
    let copper_before = inventory_count(c, COPPER_ORE);
    let tin_before = inventory_count(c, TIN_ORE);
    let bar_before = inventory_count(c, BRONZE_BAR);
    let xp_before = c.stat_xp[SMITHING];
    let level_before = c.stat_base_level[SMITHING];
    let input = use_item_on_location(c, COPPER_ORE, furnace);
    pump_until(
        c,
        r,
        45,
        "bronze smelting XP and ore consumption",
        |client| {
            inventory_count(client, COPPER_ORE) == copper_before - 1
                && inventory_count(client, TIN_ORE) == tin_before - 1
                && client.stat_xp[SMITHING] == xp_before + BRONZE_BAR_XP
                && (inventory_count(client, BRONZE_BAR) == bar_before + 1
                    || (client.stat_base_level[SMITHING] > level_before
                        && has_continue_button(client)))
        },
    );
    let leveled = c.stat_base_level[SMITHING] > level_before;
    if leveled {
        let player = c.local_player.as_ref().unwrap();
        assert_ne!(
            player.entity.spotanim_id, -1,
            "smelting level-up firework received"
        );
        assert_eq!(
            player.entity.spotanim_height, 124,
            "level-up firework height"
        );
        capture_stage(c, r, root, outbound, stages, levelup_stage_name);
        let button = click_pause_continue(c, r);
        pump_until(c, r, 15, "resume server smelting level-up", |client| {
            client.chat_modal_id < 0 && client.resumed_pause_button
        });
        eprintln!("377 production resumed Smithing level-up through component {button}");
        pump_until(
            c,
            r,
            45,
            "smelting product added after level-up resume",
            |client| inventory_count(client, BRONZE_BAR) == bar_before + 1,
        );
    } else {
        pump_until(c, r, 45, "bronze bar product received", |client| {
            inventory_count(client, BRONZE_BAR) == bar_before + 1
        });
    }
    assert_eq!(c.stat_xp[SMITHING] - xp_before, BRONZE_BAR_XP);
    assert_eq!(inventory_count(c, COPPER_ORE), copper_before - 1);
    assert_eq!(inventory_count(c, TIN_ORE), tin_before - 1);
    assert_eq!(inventory_count(c, BRONZE_BAR), bar_before + 1);
    let after = action_state(c);
    push_action(
        root,
        actions,
        outbound,
        action_name,
        before,
        after,
        packet_start,
        json!({"input":input,"expected_product":BRONZE_BAR,
            "copper_delta":-1,"tin_delta":-1,"bar_delta":1,
            "smithing_xp_delta":BRONZE_BAR_XP,"levelup":leveled,
            "source":"content/scripts/skill_smithing/scripts/smelting/smelting.rs2:28-102"}),
    );
    leveled
}

fn smith_bronze_dagger(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    outbound: &Outbound,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
    interventions: &mut Vec<Value>,
    prepare: bool,
    anvil: LoadedLocation,
) {
    ensure_item(c, r, root, interventions, prepare, HAMMER, "hammer", 1);
    let packet_start = packet_count(outbound);
    let use_receipt = use_item_on_location(c, BRONZE_BAR, anvil);
    pump_until(c, r, 20, "server smithing product interface", |client| {
        client.main_modal_id >= 0
            && client.ifaces_merged().any(|component| {
                component.layer_id == client.main_modal_id
                    && component
                        .link_obj_type
                        .as_ref()
                        .is_some_and(|objects| objects.contains(&(BRONZE_DAGGER + 1)))
            })
    });
    capture_stage(
        c,
        r,
        root,
        outbound,
        stages,
        "anvil-bronze-product-interface",
    );
    let before = action_state(c);
    let action = click_smithing_product(c, r, c.main_modal_id, BRONZE_DAGGER);
    let bar_before = inventory_count(c, BRONZE_BAR);
    let dagger_before = inventory_count(c, BRONZE_DAGGER);
    let xp_before = c.stat_xp[SMITHING];
    let level_before = c.stat_base_level[SMITHING];
    pump_until(
        c,
        r,
        45,
        "bronze dagger and Smithing XP received",
        |client| {
            inventory_count(client, BRONZE_BAR) == bar_before - 1
                && inventory_count(client, BRONZE_DAGGER) == dagger_before + 1
                && client.stat_xp[SMITHING] == xp_before + BRONZE_DAGGER_XP
        },
    );
    assert_eq!(c.stat_xp[SMITHING] - xp_before, BRONZE_DAGGER_XP);
    let leveled = handle_levelup_pause(
        c,
        r,
        root,
        outbound,
        stages,
        "levelup-from-anvil-dagger",
        level_before,
    );
    let after = action_state(c);
    push_action(
        root,
        actions,
        outbound,
        "smith-bronze-dagger",
        before,
        after,
        packet_start,
        json!({"anvil_use":use_receipt,"product_action":action,
            "expected_product":BRONZE_DAGGER,"bar_delta":-1,"dagger_delta":1,
            "smithing_xp_delta":BRONZE_DAGGER_XP,"levelup":leveled,
            "source":"content/scripts/skill_smithing/scripts/smithing/smithing.rs2:200-273; content/scripts/skill_smithing/configs/smithing/smithing.dbrow:1-10"}),
    );
    capture_stage(c, r, root, outbound, stages, "anvil-bronze-dagger-complete");
}

fn cut_logs_gap(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    outbound: &Outbound,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
    interventions: &mut Vec<Value>,
    prepare: bool,
) {
    ensure_item(c, r, root, interventions, prepare, KNIFE, "knife", 1);
    ensure_item(c, r, root, interventions, prepare, LOGS, "logs", 1);
    let before = action_state(c);
    let packet_start = packet_count(outbound);
    let knife_before = inventory_count(c, KNIFE);
    let logs_before = inventory_count(c, LOGS);
    let xp_before = c.stat_xp[9];
    let input = use_item_on_item(c, KNIFE, LOGS);
    let generation = c.gens.player_info;
    pump_until(
        c,
        r,
        20,
        "settle actual knife-on-logs product-script attempt",
        |client| client.gens.player_info >= generation + 2,
    );
    assert_eq!(inventory_count(c, KNIFE), knife_before);
    assert_eq!(inventory_count(c, LOGS), logs_before);
    assert_eq!(c.stat_xp[9], xp_before);
    let after = action_state(c);
    push_action(
        root,
        actions,
        outbound,
        "knife-on-logs-content-gap",
        before,
        after,
        packet_start,
        json!({"input":input,"result":"no output, inventory consumption, XP, or Make-X prompt observed",
            "content_gap":"multiobj3_close has no body; the cut-logs script returns before consuming wood",
            "source":"content/scripts/skill_fletching/scripts/cut_logs.rs2:53-63"}),
    );
    capture_stage(
        c,
        r,
        root,
        outbound,
        stages,
        "fletching-multiobj3-close-gap",
    );
}

fn spin_one_wool(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    outbound: &Outbound,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
    interventions: &mut Vec<Value>,
    prepare: bool,
    wheel: LoadedLocation,
) {
    ensure_item(c, r, root, interventions, prepare, WOOL, "wool", 1);
    let before = action_state(c);
    let packet_start = packet_count(outbound);
    let wool_before = inventory_count(c, WOOL);
    let ball_before = inventory_count(c, BALL_OF_WOOL);
    let xp_before = c.stat_xp[CRAFTING];
    let level_before = c.stat_base_level[CRAFTING];
    let wheel_before = location_animation(c);
    let scene_generation = c.gens.scene;
    let player_generation = c.gens.player_info;
    let player_sequence_before = c.local_player.as_ref().unwrap().entity.primary_anim;
    capture_stage(c, r, root, outbound, stages, "spinning-wheel-before");
    let input = use_item_on_location(c, WOOL, wheel);
    pump_until(
        c,
        r,
        45,
        "spun wool product, XP, player and scene animations",
        |client| {
            inventory_count(client, WOOL) == wool_before - 1
                && inventory_count(client, BALL_OF_WOOL) == ball_before + 1
                && client.stat_xp[CRAFTING] == xp_before + WOOL_XP
                && client.gens.scene > scene_generation
                && client.gens.player_info > player_generation
                && location_animation(client)
                    .get("anim_seq")
                    .and_then(Value::as_i64)
                    .is_some_and(|sequence| sequence >= 0)
                && client
                    .local_player
                    .as_ref()
                    .is_some_and(|player| player.entity.primary_anim >= 0)
        },
    );
    assert_eq!(c.stat_base_level[CRAFTING], level_before);
    let after_animation = location_animation(c);
    let player_sequence = c.local_player.as_ref().unwrap().entity.primary_anim;
    assert!(
        player_sequence >= 0,
        "server sent the spinning player sequence"
    );
    assert_ne!(after_animation, Value::Null);
    assert!(
        after_animation["anim_seq"].as_i64().unwrap() >= 0,
        "server LOC_ANIM sequence is stored on the actual scene slot"
    );
    let after = action_state(c);
    push_action(
        root,
        actions,
        outbound,
        "spin-wool-on-wheel",
        before,
        after,
        packet_start,
        json!({"input":input,"scene_before":wheel_before,
            "scene_after":after_animation,"player_sequence_before":player_sequence_before,
            "player_sequence":player_sequence,"wool_delta":-1,
            "ball_of_wool_delta":1,"crafting_xp_delta":WOOL_XP,
            "source":"content/scripts/skill_crafting/scripts/spinning/spinning.rs2:43-64; content/scripts/skill_crafting/configs/spinning/spinning.struct:8-12"}),
    );
    let leveled = handle_levelup_pause(
        c,
        r,
        root,
        outbound,
        stages,
        "levelup-from-spinning",
        level_before,
    );
    capture_stage(
        c,
        r,
        root,
        outbound,
        stages,
        "spinning-wheel-product-and-animation",
    );
    assert!(
        !leveled,
        "one wool product does not itself level a fresh Crafting 1 account"
    );
}

fn climb_up_to_spinner(
    c: &mut Client,
    r: &mut Renderer,
    actions: &mut Vec<Value>,
) -> LoadedLocation {
    let stair = find_location(c, Some(STAIRS_UP), None, Some("Climb-up"), None)
        .expect("actual Lumbridge south staircase on plane 0");
    walk_near(c, r, (stair.x, stair.z), actions);
    let receipt = operate_location(c, stair, stair.option_index.unwrap());
    actions.push(receipt);
    pump_until(c, r, 30, "Climb-up server destination", |client| {
        client.minusedlevel == 1 && tile(client) == (3205, 3209)
    });
    let wheel = find_location(c, Some(SPINNING_WHEEL), None, None, None)
        .expect("Lumbridge plane-one spinning wheel loaded from scene");
    assert_eq!((wheel.x, wheel.z), (3209, 3212));
    walk_near(c, r, (wheel.x, wheel.z), actions);
    wheel
}

fn descend_to_furnace(c: &mut Client, r: &mut Renderer, actions: &mut Vec<Value>) {
    if c.minusedlevel == 0 {
        return;
    }
    assert_eq!(c.minusedlevel, 1, "expected the first Lumbridge floor");
    let stair = find_location(c, Some(STAIRS_DOWN), None, Some("Climb-down"), None)
        .expect("actual Lumbridge staircase on plane 1");
    walk_near(c, r, (stair.x, stair.z), actions);
    actions.push(operate_location(c, stair, stair.option_index.unwrap()));
    pump_until(c, r, 30, "Climb-down server destination", |client| {
        client.minusedlevel == 0 && tile(client) == (3205, 3209)
    });
}

fn run_phase(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    outbound: &Outbound,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
    interventions: &mut Vec<Value>,
    phase: &str,
    prepare: bool,
) {
    let all = phase == "all";
    if all
        || phase == "furnace"
        || phase == "anvil"
        || phase == "levelup"
        || phase == "cook"
        || phase == "craft"
    {
        descend_to_furnace(c, r, actions);
        ensure_lumbridge(c, r, actions);
    }

    if all || phase == "furnace" {
        let furnace = find_location(c, None, Some("Furnace"), None, None)
            .expect("loaded Lumbridge smithing furnace");
        walk_near(c, r, (furnace.x, furnace.z), actions);
        ensure_item(
            c,
            r,
            root,
            interventions,
            prepare,
            COPPER_ORE,
            "copper_ore",
            1,
        );
        ensure_item(c, r, root, interventions, prepare, TIN_ORE, "tin_ore", 1);
        capture_stage(c, r, root, outbound, stages, "furnace-before-bronze-smelt");
        smelt_once(
            c,
            r,
            root,
            outbound,
            stages,
            actions,
            interventions,
            prepare,
            furnace,
            "smelt-one-bronze-bar",
            "levelup-from-first-smelt",
        );
        capture_stage(c, r, root, outbound, stages, "furnace-bronze-bar-complete");
    }

    if all || phase == "anvil" {
        let anvil = find_location(c, None, Some("Anvil"), None, None)
            .expect("loaded Lumbridge smithing anvil");
        walk_near(c, r, (anvil.x, anvil.z), actions);
        if phase == "anvil" {
            ensure_item(
                c,
                r,
                root,
                interventions,
                prepare,
                BRONZE_BAR,
                "bronze_bar",
                1,
            );
        }
        smith_bronze_dagger(
            c,
            r,
            root,
            outbound,
            stages,
            actions,
            interventions,
            prepare,
            anvil,
        );
    }

    if all || phase == "cook" {
        approach_cook(c, r, actions);
        ensure_item(
            c,
            r,
            root,
            interventions,
            prepare,
            RAW_SHRIMP,
            "raw_shrimp",
            6,
        );
        let raw_count = inventory_count(c, RAW_SHRIMP);
        assert!(
            (3..=27).contains(&raw_count),
            "preserve room for the All batch and later inputs"
        );
        let range = find_location(c, None, Some("Cooking range"), None, None)
            .expect("server-loaded Lumbridge cooking range");
        walk_near(c, r, (range.x, range.z), actions);
        cook_range(c, r, root, outbound, stages, actions, range);
    }

    if all || phase == "fletch" {
        cut_logs_gap(
            c,
            r,
            root,
            outbound,
            stages,
            actions,
            interventions,
            prepare,
        );
    }

    if all || phase == "craft" {
        if tile(c) != (3208, 3213) {
            approach_cook(c, r, actions);
        }
        leave_kitchen(c, r, actions);
        let wheel = climb_up_to_spinner(c, r, actions);
        spin_one_wool(
            c,
            r,
            root,
            outbound,
            stages,
            actions,
            interventions,
            prepare,
            wheel,
        );
    }

    if all || phase == "levelup" {
        descend_to_furnace(c, r, actions);
        ensure_lumbridge(c, r, actions);
        let furnace = find_location(c, None, Some("Furnace"), None, None)
            .expect("loaded Lumbridge furnace for server-driven level-up");
        walk_near(c, r, (furnace.x, furnace.z), actions);
        if phase == "levelup" {
            prepare_smithing_baseline(c, r, root, interventions, prepare);
        }
        assert_eq!(c.stat_base_level[SMITHING], 1);
        let mut leveled = false;
        for index in 0..14 {
            let did_level = smelt_once(
                c,
                r,
                root,
                outbound,
                stages,
                actions,
                interventions,
                prepare,
                furnace,
                &format!("levelup-bronze-smelt-{}", index + 1),
                "smithing-levelup-dialog-firework",
            );
            if did_level || c.stat_base_level[SMITHING] > 1 {
                leveled = true;
                break;
            }
        }
        assert!(
            leveled,
            "real bronze-smelting products advance Smithing through level 2"
        );
        assert_eq!(c.stat_base_level[SMITHING], 2);
        assert!(
            c.resumed_pause_button,
            "level-up pause was resumed through its real button"
        );
        capture_stage(
            c,
            r,
            root,
            outbound,
            stages,
            "smithing-levelup-after-resume",
        );
    }
}

#[test]
#[ignore = "requires orchestrator-started Fairy Ring 377 engine on localhost:81/43595"]
fn live_production_377_smelt_smith_cook_fletch_craft_and_levelup() {
    let evidence_base = PathBuf::from(
        std::env::var("CLIENT_377_LIVE_DIR").expect("explicit evidence directory required"),
    );
    let cache = std::env::var("CLIENT_377_RUNTIME_CACHE_DIR")
        .expect("matched running runtime cache required");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("explicit t2a account required");
    let password = std::env::var("CLIENT_377_LIVE_PASSWORD")
        .expect("explicit account password required; never hardcode credentials");
    assert!(
        user.starts_with("t2a"),
        "account choice comes from explicit t2a env"
    );
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists(), "quiet lock blocks live runs");
    }
    let phase = std::env::var("CLIENT_377_PRODUCTION_PHASE").unwrap_or_else(|_| "all".into());
    assert!(
        matches!(
            phase.as_str(),
            "all" | "furnace" | "anvil" | "cook" | "fletch" | "craft" | "levelup"
        ),
        "production phase is all|furnace|anvil|cook|fletch|craft|levelup"
    );
    let prepare = match std::env::var("CLIENT_377_PRODUCTION_PREPARE") {
        Ok(value) if value == "1" => true,
        Ok(value) if value == "0" => false,
        Ok(_) => panic!("CLIENT_377_PRODUCTION_PREPARE must be 0 or 1"),
        Err(_) => false,
    };
    let manual_interventions: Value = std::env::var("CLIENT_377_PRODUCTION_INTERVENTIONS")
        .map(|value| serde_json::from_str(&value).expect("interventions must be valid JSON"))
        .unwrap_or_else(|_| json!([]));
    assert!(
        manual_interventions.is_array(),
        "interventions is a JSON array"
    );

    std::fs::create_dir_all(&evidence_base).unwrap();
    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let root = evidence_base.join(format!("production377-{run_id}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let run_config = json!({
        "account":user,"phase":phase,"prepare":prepare,
        "manual_interventions":manual_interventions,
        "sources":{
            "smelting":"content/scripts/skill_smithing/scripts/smelting/smelting.rs2:28-102",
            "smithing":"content/scripts/skill_smithing/scripts/smithing/smithing.rs2:200-273",
            "cooking":"content/scripts/skill_cooking/scripts/cooking.rs2:10-193",
            "fletching":"content/scripts/skill_fletching/scripts/cut_logs.rs2:29-63",
            "spinning":"content/scripts/skill_crafting/scripts/spinning/spinning.rs2:43-64",
            "levelup":"content/scripts/levelup.rs2"
        },
        "expected_products":{"bronze_bar":BRONZE_BAR,"bronze_dagger":BRONZE_DAGGER,
            "shrimp":COOKED_SHRIMP,"burnt_fish":BURNT_FISH,"ball_of_wool":BALL_OF_WOOL},
        "content_gap":"cut_logs.rs2 calls multiobj3_close; its procedure body is commented out"
    });
    write_json(&root.join("run.json"), &run_config);
    let mut interventions = Vec::new();
    if !manual_interventions.as_array().unwrap().is_empty() {
        record_intervention(
            &root,
            &mut interventions,
            json!({"label":"operator-declared interventions supplied through CLIENT_377_PRODUCTION_INTERVENTIONS",
                "entries":manual_interventions,
                "note":"no additional state mutation performed by this record"}),
        );
    }
    persist_array(&root.join("interventions.json"), &interventions);
    let mut actions = Vec::new();
    let mut stages = Vec::new();
    persist_array(&root.join("actions.json"), &actions);
    persist_array(&root.join("stages.json"), &stages);

    let _engine = live_engine_377::require_engine(&root, &user, &cache);
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
    c.maininit_with_progress(Some(&mut |client, message, percent| {
        r.draw_progress(client, message, percent);
    }));
    assert!(
        !c.error_loading,
        "matched revision-377 runtime cache loaded"
    );
    let outbound = Arc::new(Outbound::default());
    c.out.set_observer(Some(outbound.clone()));
    c.set_login_deadline(Instant::now() + Duration::from_secs(10));
    c.login(&user, &password, false).unwrap();
    pump_until(
        &mut c,
        &mut r,
        90,
        "selected account entered loaded mainland",
        |client| client.scene_state == 2 && client.local_player.is_some(),
    );
    assert_eq!(
        c.minusedlevel, 0,
        "selected mainland account starts on ground floor"
    );
    if phase == "all" || phase == "levelup" {
        prepare_smithing_baseline(&mut c, &mut r, &root, &mut interventions, prepare);
    }
    run_phase(
        &mut c,
        &mut r,
        &root,
        &outbound,
        &mut stages,
        &mut actions,
        &mut interventions,
        &phase,
        prepare,
    );
    write_json(
        &root.join("result.json"),
        &json!({"phase":phase,"stages":stages.len(),"actions":actions.len(),
            "interventions":interventions.len(),"account":user,
            "final_state":full_state(&c,&packets_since(&outbound,0))}),
    );
    eprintln!(
        "PASS: production 377 phase={phase}, stages={}, actions={}, evidence={}",
        stages.len(),
        actions.len(),
        root.display()
    );
}
