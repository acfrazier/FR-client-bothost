use super::*;

// Cook's quest is server-only. The content transmits its row colour, not varp 29.
// Java expands the IF_SETCOLOUR five-bit channels without filling the low bits.
const QUEST_UNSTARTED_COLOUR: i32 = 0xf80000;
const QUEST_ACTIVE_COLOUR: i32 = 0xf8f800;
const QUEST_COMPLETE_COLOUR: i32 = 0x00f800;
const COINS: i32 = 995;
const BRONZE_DAGGER: i32 = 1205;
const BRONZE_SWORD: i32 = 1277;
const BUCKET_EMPTY: i32 = 1925;
const BUCKET_MILK: i32 = 1927;
const POT_EMPTY: i32 = 1931;
const POT_FLOUR: i32 = 1933;
const EGG: i32 = 1944;
const GRAIN: i32 = 1947;
const WINDMILL_FLOOR: (i32, i32) = (3166, 3307);
const COW_ZONE: (i32, i32) = (3254, 3272);
const WEST_FARM_ROAD: [(i32, i32); 7] = [
    (3216, 3262),
    (3216, 3279),
    (3200, 3280),
    (3192, 3282),
    (3184, 3287),
    (3174, 3286),
    (3163, 3289),
];

struct OutboundFile {
    packets: Arc<Outbound>,
    path: PathBuf,
}

impl Drop for OutboundFile {
    fn drop(&mut self) {
        let packets = self.packets.0.lock().unwrap();
        let bytes = serde_json::to_vec_pretty(&*packets).unwrap();
        if let Err(error) = std::fs::write(&self.path, bytes) {
            eprintln!("cannot save mainland outbound receipt: {error}");
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct LoadedLocation {
    id: usize,
    code: i32,
    x: i32,
    z: i32,
    option_index: usize,
}

pub(super) fn interface_components(c: &Client, layer: i32) -> Vec<Value> {
    if layer < 0 {
        return Vec::new();
    }
    c.ifaces_merged()
        .filter(|i| {
            i.layer_id == layer
                && (!i.text.is_empty() || !i.button_text.is_empty() || i.link_obj_type.is_some())
        })
        .map(|i| {
            json!({"component":i.id,"type":i.r#type,"button_type":i.button_type,"client_code":i.client_code,
                "text":i.text,"button_text":i.button_text,"colour":i.colour,
                "objects":i.link_obj_type,"counts":i.link_obj_number})
        })
        .collect()
}

pub(super) fn interface_text(c: &Client, layer: i32) -> String {
    if layer < 0 {
        return String::new();
    }
    c.ifaces_merged()
        .filter(|i| i.layer_id == layer)
        .flat_map(|i| [i.text.to_string(), i.button_text.clone()])
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn ground_state(c: &Client) -> Vec<Value> {
    (0..104)
        .flat_map(|x| (0..104).map(move |z| (x, z)))
        .filter_map(|(x, z)| {
            let ground = c.world.ground_object_at(c.minusedlevel, x, z)?;
            let stack = [ground.top, ground.middle, ground.bottom]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            (!stack.is_empty()).then(|| {
                json!({"tile":[x+c.map_build_base_x,z+c.map_build_base_z],
                    "typecode":ground.typecode,"stack":stack})
            })
        })
        .collect()
}

fn evidence_state(c: &Client) -> Value {
    let mut snapshot = super::state(c);
    snapshot["windmill"] = json!({
        "millcheck_varp_695":c.var.get(695),
        "flour_bin_form":c.cache.resolve_loc(1781, &c.var)
            .map(|loc|json!({"id":loc.id,"name":loc.name,"options":loc.op})),
    });
    snapshot
}

fn save_mainland_stage(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    name: &str,
    stages: &mut Vec<Value>,
) {
    let generation = c.gens.player_info;
    pump_until(c, r, 10, |c| c.gens.player_info >= generation + 2);
    let path = root.join(format!("rust-mainland-{name}.jpg"));
    screenshot(&path, r.mainredraw(c));
    let stage = json!({"stage":name,"screenshot":path,"roof_top":r.roof_check(c),"state":evidence_state(c)});
    eprintln!("377 mainland stage {stage}");
    stages.push(stage);
    std::fs::write(
        root.join("mainland-stages.json"),
        serde_json::to_vec_pretty(stages).unwrap(),
    )
    .unwrap();
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
        tutorial_lesson_377::walk_to(c, r, next, actions);
    }
    assert_eq!(
        tile(c),
        target,
        "minimap route reached its content waypoint"
    );
}

fn loaded_location(
    c: &Client,
    name: &str,
    option: Option<&str>,
    near: Option<(i32, i32, i32)>,
) -> Option<LoadedLocation> {
    let name = name.to_ascii_lowercase();
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
            for code in [
                c.world.wall_type(c.minusedlevel, x, z),
                c.world.decor_type(c.minusedlevel, x, z),
                c.world.scene_type(c.minusedlevel, x, z),
                c.world.gd_type(c.minusedlevel, x, z),
            ] {
                if code == 0 {
                    continue;
                }
                let id = ((code >> 14) & 0x7fff) as usize;
                let Some(loc) = c.cache.resolve_loc(id, &c.var) else {
                    continue;
                };
                if loc.name.to_ascii_lowercase() != name {
                    continue;
                }
                let option_index = match option {
                    Some(option) => loc
                        .op
                        .iter()
                        .position(|candidate| candidate.as_deref() == Some(option)),
                    None => Some(0),
                };
                let Some(option_index) = option_index else {
                    continue;
                };
                found.push(LoadedLocation {
                    id: loc.id as usize,
                    code,
                    x: world_x,
                    z: world_z,
                    option_index,
                });
            }
        }
    }
    found
        .into_iter()
        .min_by_key(|location| (location.x - player.0).abs() + (location.z - player.1).abs())
}

fn operate_loaded_location(c: &mut Client, location: LoadedLocation) -> Value {
    let action = match location.option_index {
        0 => MiniMenuAction::OP_LOC1,
        1 => MiniMenuAction::OP_LOC2,
        _ => panic!("fixture uses only the first two real location options"),
    };
    let loc = &c.cache.locs[location.id];
    let option = loc.op[location.option_index].as_deref().unwrap();
    let expected_option = format!("{option} @cya@{}", loc.name);
    let receipt = json!({"location":location.id,"scene_location":(location.code >> 14) & 0x7fff,
        "typecode":location.code,"name":loc.name,"option":option,"option_index":location.option_index,
        "tile":[location.x,location.z],"input":"production scene minimenu and doAction"});
    c.menu_num_entries = 0;
    c.pick_count = 1;
    c.pick_typecodes[0] = location.code;
    c.add_world_options();
    let entry = (0..c.menu_num_entries as usize)
        .find(|&entry| {
            c.menu_action[entry] == action
                && c.menu_param_a[entry] == location.code
                && c.menu_option[entry] == expected_option
        })
        .expect("production menu exposes the selected location form's actual option");
    c.doAction(entry as i32);
    eprintln!("377 mainland location action {receipt}");
    receipt
}

fn open_passage(
    c: &mut Client,
    r: &mut Renderer,
    name: &str,
    position: (i32, i32),
    actions: &mut Vec<Value>,
) {
    if let Some(closed) = loaded_location(c, name, Some("Open"), Some((position.0, position.1, 0)))
    {
        actions.push(operate_loaded_location(c, closed));
        pump_until(c, r, 30, |c| {
            loaded_location(c, name, Some("Open"), Some((position.0, position.1, 0))).is_none()
                && loaded_location(c, name, Some("Close"), Some((position.0, position.1, 3)))
                    .is_some()
        });
    } else {
        assert!(
            loaded_location(c, name, Some("Close"), Some((position.0, position.1, 3))).is_some(),
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

fn npc_type(c: &Client, name: &str, option: &str) -> usize {
    let name = name.to_ascii_lowercase();
    c.cache
        .npcs
        .iter()
        .position(|npc| {
            npc.name.to_ascii_lowercase().contains(&name)
                && npc
                    .op
                    .iter()
                    .flatten()
                    .any(|candidate| candidate.as_str() == option)
        })
        .unwrap_or_else(|| panic!("cache NPC has the real {name} option {option}"))
}

fn npc_slot(c: &Client, npc_type: usize) -> i32 {
    c.npc_ids
        .iter()
        .take(c.npc_count as usize)
        .copied()
        .find(|slot| {
            c.npc[*slot as usize]
                .as_ref()
                .is_some_and(|npc| npc.r#type == Some(npc_type))
        })
        .unwrap_or_else(|| panic!("server-populated NPC type {npc_type} is in the scene"))
}

fn operate_npc(c: &mut Client, npc_type: usize, action: i32, option: &str) -> Value {
    let slot = npc_slot(c, npc_type);
    let npc = c.npc[slot as usize].as_ref().unwrap();
    let receipt = json!({"slot":slot,"type":npc_type,"name":c.cache.npcs[npc_type].name,
        "option":option,"player":tile(c),
        "npc_tile":[npc.route_x[0]+c.map_build_base_x,npc.route_z[0]+c.map_build_base_z],
        "input":"production doAction on server-populated NPC slot"});
    c.menu_action[0] = action;
    c.menu_param_a[0] = slot;
    c.menu_param_b[0] = 0;
    c.menu_param_c[0] = 0;
    c.doAction(0);
    eprintln!("377 mainland NPC action {receipt}");
    receipt
}

fn dialogue_contains(c: &Client, expected: &str) -> bool {
    interface_text(c, c.chat_modal_id)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .contains(expected)
}

fn journal_has_collected(text: &str, item: &str) -> bool {
    text.lines()
        .any(|line| line.contains("@str@") && line.contains(item))
}

fn advance_chat_to(c: &mut Client, r: &mut Renderer, expected: &str) {
    for _ in 0..12 {
        if dialogue_contains(c, expected) {
            return;
        }
        assert!(
            c.chat_modal_id >= 0
                && c.ifaces_merged()
                    .any(|i| i.layer_id == c.chat_modal_id && i.button_type == 6),
            "real continue button precedes {expected:?}: {}",
            body(c)
        );
        let previous = body(c);
        continue_chat(c, r);
        pump_until(c, r, 15, |c| {
            c.chat_modal_id >= 0 && !c.resumed_pause_button && body(c) != previous
        });
    }
    panic!("content dialogue did not reach {expected:?}: {}", body(c));
}

fn choose_chat_option(c: &mut Client, r: &mut Renderer, expected: &str) -> Value {
    advance_chat_to(c, r, expected);
    let root = c.chat_modal_id;
    assert!(root >= 0, "server dialogue choice is open");
    let (component, button_type, text, button_text) = c
        .ifaces_merged()
        .find(|i| {
            i.layer_id == root
                && i.button_type != 0
                && (i.button_text.contains(expected) || i.text.contains(expected))
        })
        .map(|i| {
            (
                i.id,
                i.button_type,
                i.text.to_string(),
                i.button_text.clone(),
            )
        })
        .unwrap_or_else(|| panic!("real dialogue option {expected:?} exists: {}", body(c)));
    let receipt = json!({"component":component,"button_type":button_type,
        "text":text,"button_text":button_text,"option":expected,
        "input":"rendered server dialogue component click"});
    let previous = interface_text(c, root);
    click_component(c, r, root, component, (17, 357));
    pump_until(c, r, 15, |c| {
        c.chat_modal_id != root || interface_text(c, root) != previous
    });
    receipt
}

fn dismiss_chat(c: &mut Client, r: &mut Renderer) {
    for _ in 0..12 {
        if c.chat_modal_id < 0 {
            return;
        }
        let previous = body(c);
        assert!(
            c.ifaces_merged()
                .any(|i| i.layer_id == c.chat_modal_id && i.button_type == 6),
            "only a real server continue button may dismiss chat: {previous}"
        );
        continue_chat(c, r);
        pump_until(c, r, 12, |c| c.chat_modal_id < 0 || body(c) != previous);
    }
    assert!(
        c.chat_modal_id < 0,
        "server dialogue completed before next action"
    );
}

fn shop_inventory_component(c: &Client, side: bool) -> (i32, Vec<i32>, Vec<i32>) {
    let layer = if side {
        c.side_modal_id
    } else {
        c.main_modal_id
    };
    c.ifaces_merged()
        .find(|i| i.layer_id == layer && i.link_obj_type.is_some() && i.link_obj_number.is_some())
        .map(|i| {
            (
                i.id,
                i.link_obj_type.as_ref().unwrap().to_vec(),
                i.link_obj_number.as_ref().unwrap().to_vec(),
            )
        })
        .unwrap_or_else(|| {
            panic!(
                "actual {} shop inventory component is open",
                if side { "player" } else { "stock" }
            )
        })
}

fn shop_side_player_inventory(c: &Client) -> i32 {
    let player = c.if_(3214).expect("player inventory exists");
    let types = player
        .link_obj_type
        .as_deref()
        .expect("player inventory slots");
    let counts = player
        .link_obj_number
        .as_deref()
        .expect("player inventory counts");
    c.ifaces_merged()
        .find(|i| {
            i.layer_id == c.side_modal_id
                && i.link_obj_type.as_deref() == Some(types)
                && i.link_obj_number.as_deref() == Some(counts)
        })
        .map(|i| i.id)
        .expect("shop transmits the live player inventory to its real side component")
}

fn shop_slot(types: &[i32], counts: &[i32], object: i32) -> i32 {
    types
        .iter()
        .enumerate()
        .find(|(slot, id)| **id == object + 1 && counts.get(*slot).is_some_and(|count| *count > 0))
        .map(|(slot, _)| slot as i32)
        .expect("actual shop or inventory item slot exists")
}

fn shop_inventory_button(
    c: &mut Client,
    component: i32,
    slot: i32,
    object: i32,
    option: &str,
) -> Value {
    let receipt = json!({"object":object,"name":c.cache.objs[object as usize].name,
        "slot":slot,"component":component,"option":option,
        "input":"production doAction(INV_BUTTON2) on received shop inventory slot"});
    c.menu_action[0] = MiniMenuAction::INV_BUTTON2;
    c.menu_param_a[0] = object;
    c.menu_param_b[0] = slot;
    c.menu_param_c[0] = component;
    c.doAction(0);
    eprintln!("377 mainland shop action {receipt}");
    receipt
}

fn ground_item(c: &Client, object: i32) -> Option<(i32, i32, i32)> {
    let player = tile(c);
    (0..104)
        .flat_map(|x| (0..104).map(move |z| (x, z)))
        .filter_map(|(x, z)| {
            let ground = c.world.ground_object_at(c.minusedlevel, x, z)?;
            [ground.top, ground.middle, ground.bottom]
                .into_iter()
                .flatten()
                .find(|(id, _)| *id == object)
                .map(|(_, count)| (x + c.map_build_base_x, z + c.map_build_base_z, count))
        })
        .min_by_key(|(x, z, _)| (x - player.0).abs() + (z - player.1).abs())
}

fn pickup_ground_item(c: &mut Client, object: i32, x: i32, z: i32) -> Value {
    let receipt = json!({"object":object,"name":c.cache.objs[object as usize].name,
        "tile":[x,z],"stack":ground_item(c,object),
        "input":"production doAction(OP_OBJ3) on actual ground-object stack"});
    c.menu_action[0] = MiniMenuAction::OP_OBJ3;
    c.menu_param_a[0] = object;
    c.menu_param_b[0] = x - c.map_build_base_x;
    c.menu_param_c[0] = z - c.map_build_base_z;
    c.doAction(0);
    eprintln!("377 mainland ground pickup {receipt}");
    receipt
}

fn drop_inventory_item(c: &mut Client, object: i32) -> Value {
    let slot = inventory_slot(c, object);
    let receipt = json!({"object":object,"name":c.cache.objs[object as usize].name,
        "slot":slot,"component":3214,"tile":tile(c),
        "input":"production doAction(OP_HELD5) on actual player inventory slot"});
    c.menu_action[0] = MiniMenuAction::OP_HELD5;
    c.menu_param_a[0] = object;
    c.menu_param_b[0] = slot;
    c.menu_param_c[0] = 3214;
    c.doAction(0);
    eprintln!("377 mainland ground drop {receipt}");
    receipt
}

fn cook_journal_components(c: &Client) -> String {
    interface_text(c, c.main_modal_id)
}

fn cook_journal_row(c: &Client) -> Option<(i32, i32, String)> {
    let row = c.ifaces_merged().find(|i| {
        i.layer_id == c.side_icon[2]
            && i.button_type != 0
            && (i.button_text.to_ascii_lowercase().contains("cook")
                || i.text.to_ascii_lowercase().contains("cook"))
    })?;
    Some((
        row.id,
        row.colour,
        format!("{}{}", row.text, row.button_text),
    ))
}

fn open_cook_journal(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    stage: &str,
) -> Value {
    tutorial_lesson_377::tab(c, r, 2);
    assert!(
        c.side_icon[2] >= 0,
        "server enabled the real quest-list tab"
    );
    let (row_id, colour, label) =
        cook_journal_row(c).expect("quest list exposes the real Cook quest component");
    let side = c.side_icon[2];
    click_component(c, r, side, row_id, (553, 205));
    pump_until(c, r, 15, |c| {
        c.main_modal_id >= 0 && cook_journal_components(c).contains("The Cook's Quest")
    });
    let text = cook_journal_components(c);
    let journal = json!({"title":"The Cook's Quest","row":{"component":row_id,
        "text":label,"colour":colour},"text":text,
        "components":interface_components(c,c.main_modal_id)});
    save_mainland_stage(c, r, root, stage, stages);
    journal
}

fn chat_contains(c: &Client, expected: &str) -> bool {
    c.chat_text.iter().any(|text| text.contains(expected))
}

fn climb_windmill(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
    up: bool,
) {
    let before = c.minusedlevel;
    let option = if up {
        "Climb-up"
    } else if before == 1 {
        "Climb"
    } else {
        "Climb-down"
    };
    let location = loaded_location(
        c,
        "ladder",
        Some(option),
        Some((WINDMILL_FLOOR.0, WINDMILL_FLOOR.1, 14)),
    )
    .unwrap_or_else(|| panic!("windmill ladder has {option}: {}", state(c)));
    actions.push(operate_loaded_location(c, location));
    if !up && before == 1 {
        pump_until(c, r, 15, |c| {
            interface_text(c, c.chat_modal_id).contains("Climb Down.")
        });
        actions.push(choose_chat_option(c, r, "Climb Down."));
    }
    pump_until(c, r, 30, |c| {
        if up {
            c.minusedlevel > before
        } else {
            c.minusedlevel < before
        }
    });
    save_mainland_stage(
        c,
        r,
        root,
        &format!("windmill-plane-{}", c.minusedlevel),
        stages,
    );
}

#[test]
#[ignore = "requires orchestrator-started Fairy Ring 377 engine and naturally progressed Lumbridge t2a account"]
fn natural_lumbridge_cook_quest_and_shop_377() {
    let root =
        PathBuf::from(std::env::var("CLIENT_377_LIVE_DIR").expect("explicit evidence directory"));
    let cache = std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("matched running cache");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("explicit t2a account");
    assert!(user.starts_with("t2a"));
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists());
    }
    let lesson = std::env::var("CLIENT_377_MAINLAND_LESSON").unwrap_or_else(|_| "full".into());
    assert!(
        matches!(
            lesson.as_str(),
            "full" | "shop" | "start" | "ingredients" | "mill" | "complete"
        ),
        "unknown mainland lesson {lesson:?}"
    );
    let run_shop = matches!(lesson.as_str(), "full" | "shop");
    let run_start = matches!(lesson.as_str(), "full" | "start");
    let run_ingredients = matches!(lesson.as_str(), "full" | "ingredients");
    let run_mill = matches!(lesson.as_str(), "full" | "mill");
    let run_complete = matches!(lesson.as_str(), "full" | "complete");
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
        r.draw_progress(c, message, percent)
    }));
    assert!(!c.error_loading);
    let outbound = Arc::new(Outbound::default());
    c.out.set_observer(Some(outbound.clone()));
    let _outbound_file = OutboundFile {
        packets: outbound.clone(),
        path: root.join("mainland-outbound.json"),
    };
    c.set_login_deadline(Instant::now() + Duration::from_secs(10));
    c.login(&user, "t2apass", false).unwrap();
    pump_until(&mut c, &mut r, 90, |c| {
        c.scene_state == 2 && c.npc_count > 0 && inventory_count(c, 1351) == 1
    });

    assert_eq!(
        c.minusedlevel, 0,
        "fresh mainland account is on Lumbridge ground floor"
    );
    let expected_colour = if run_shop || run_start {
        QUEST_UNSTARTED_COLOUR
    } else {
        QUEST_ACTIVE_COLOUR
    };
    pump_until(&mut c, &mut r, 15, |c| {
        cook_journal_row(c).is_some_and(|(_, colour, _)| colour == expected_colour)
    });
    assert_eq!(inventory_count(&c, BRONZE_SWORD), 1);
    if run_shop {
        assert_eq!(
            inventory_count(&c, COINS),
            0,
            "tutorial deposited the starter coins"
        );
        assert_eq!(inventory_count(&c, BRONZE_DAGGER), 1);
    }
    if run_ingredients {
        assert_eq!(
            inventory_count(&c, BUCKET_EMPTY) + inventory_count(&c, BUCKET_MILK),
            1
        );
        assert_eq!(inventory_count(&c, POT_EMPTY), 1);
        if lesson == "full" {
            assert_eq!(inventory_count(&c, EGG), 0);
            assert_eq!(inventory_count(&c, BUCKET_MILK), 0);
        } else {
            assert!(inventory_count(&c, EGG) <= 1);
        }
    }
    if lesson == "mill" {
        assert_eq!(inventory_count(&c, EGG), 1);
        assert_eq!(inventory_count(&c, BUCKET_MILK), 1);
        assert!(inventory_count(&c, GRAIN) <= 1);
        assert_eq!(inventory_count(&c, POT_EMPTY), 1);
    }
    if lesson == "complete" {
        assert_eq!(inventory_count(&c, EGG), 1);
        assert_eq!(inventory_count(&c, BUCKET_MILK), 1);
        assert_eq!(inventory_count(&c, POT_FLOUR), 1);
    }
    let mut stages = Vec::new();
    let mut actions = Vec::new();
    save_mainland_stage(&mut c, &mut r, &root, "lumbridge-ready", &mut stages);

    if run_shop {
        // The naturally issued bronze dagger funds a real shop purchase; no coins
        // are granted or synthesized for the shop path.
        walk_route(&mut c, &mut r, (3220, 3244), &mut actions);
        save_mainland_stage(&mut c, &mut r, &root, "general-store-approach", &mut stages);
        let shopkeeper = npc_type(&c, "shop keeper", "Trade");
        actions.push(operate_npc(
            &mut c,
            shopkeeper,
            MiniMenuAction::OP_NPC3,
            "Trade",
        ));
        pump_until(&mut c, &mut r, 20, |c| {
            c.main_modal_id >= 0
                && c.side_modal_id >= 0
                && interface_text(c, c.main_modal_id).contains("Lumbridge General Store")
        });
        assert!(interface_text(&c, c.main_modal_id).contains("Lumbridge General Store"));
        save_mainland_stage(&mut c, &mut r, &root, "general-store-open", &mut stages);

        let side_inv = shop_side_player_inventory(&c);
        let side_view = c
            .if_(side_inv as usize)
            .expect("actual transmitted shop-side inventory");
        let dagger_slot = shop_slot(
            side_view.link_obj_type.as_deref().unwrap(),
            side_view.link_obj_number.as_deref().unwrap(),
            BRONZE_DAGGER,
        );
        actions.push(shop_inventory_button(
            &mut c,
            side_inv,
            dagger_slot,
            BRONZE_DAGGER,
            "Sell 1",
        ));
        pump_until(&mut c, &mut r, 20, |c| {
            inventory_count(c, BRONZE_DAGGER) == 0 && inventory_count(c, COINS) > 0
        });
        assert!(
            inventory_count(&c, COINS) > 0,
            "shop sale funds a later Buy 1 action"
        );
        save_mainland_stage(
            &mut c,
            &mut r,
            &root,
            "general-store-sold-dagger",
            &mut stages,
        );

        let (stock_inv, stock_types, stock_counts) = shop_inventory_component(&c, false);
        let bucket_slot = shop_slot(&stock_types, &stock_counts, BUCKET_EMPTY);
        let buckets_before_buy = inventory_count(&c, BUCKET_EMPTY);
        let coins_before_buy = inventory_count(&c, COINS);
        actions.push(shop_inventory_button(
            &mut c,
            stock_inv,
            bucket_slot,
            BUCKET_EMPTY,
            "Buy 1",
        ));
        pump_until(&mut c, &mut r, 20, |c| {
            inventory_count(c, BUCKET_EMPTY) == buckets_before_buy + 1
                && inventory_count(c, COINS) < coins_before_buy
        });
        save_mainland_stage(
            &mut c,
            &mut r,
            &root,
            "general-store-bought-bucket",
            &mut stages,
        );

        let side_inv = shop_side_player_inventory(&c);
        let side_view = c
            .if_(side_inv as usize)
            .expect("actual transmitted shop-side inventory");
        let bucket_slot = shop_slot(
            side_view.link_obj_type.as_deref().unwrap(),
            side_view.link_obj_number.as_deref().unwrap(),
            BUCKET_EMPTY,
        );
        actions.push(shop_inventory_button(
            &mut c,
            side_inv,
            bucket_slot,
            BUCKET_EMPTY,
            "Sell 1",
        ));
        pump_until(&mut c, &mut r, 20, |c| {
            inventory_count(c, BUCKET_EMPTY) == buckets_before_buy
        });
        save_mainland_stage(
            &mut c,
            &mut r,
            &root,
            "general-store-sold-bucket",
            &mut stages,
        );
        tutorial_lesson_377::close_main(&mut c, &mut r);
        pump_until(&mut c, &mut r, 15, |c| {
            c.main_modal_id < 0 && c.side_modal_id < 0
        });
    }

    if run_start {
        let unstarted_journal =
            open_cook_journal(&mut c, &mut r, &root, &mut stages, "cook-journal-unstarted");
        assert!(unstarted_journal["text"]
            .as_str()
            .unwrap()
            .contains("I can start this quest by speaking"));
        let unstarted_colour = unstarted_journal["row"]["colour"].clone();
        tutorial_lesson_377::close_main(&mut c, &mut r);
        pump_until(&mut c, &mut r, 15, |c| {
            c.main_modal_id < 0 && c.side_modal_id < 0
        });

        approach_cook(&mut c, &mut r, &mut actions);
        let cook_type = npc_type(&c, "cook", "Talk-to");
        actions.push(operate_npc(
            &mut c,
            cook_type,
            MiniMenuAction::OP_NPC1,
            "Talk-to",
        ));
        pump_until(&mut c, &mut r, 20, |c| {
            c.chat_modal_id >= 0 && body(c).contains("What am I to do?")
        });
        save_mainland_stage(&mut c, &mut r, &root, "cook-start-choice", &mut stages);
        actions.push(choose_chat_option(&mut c, &mut r, "What's wrong?"));
        advance_chat_to(&mut c, &mut r, "Ooh dear, I'm in a terrible mess");
        actions.push(choose_chat_option(&mut c, &mut r, "Yes, I'll help you."));
        advance_chat_to(&mut c, &mut r, "I need milk, an egg and flour");
        pump_until(&mut c, &mut r, 25, |c| {
            cook_journal_row(c).is_some_and(|(_, colour, _)| colour == QUEST_ACTIVE_COLOUR)
                && dialogue_contains(c, "I need milk, an egg and flour")
        });
        save_mainland_stage(&mut c, &mut r, &root, "cook-quest-accepted", &mut stages);
        dismiss_chat(&mut c, &mut r);
        let start_journal =
            open_cook_journal(&mut c, &mut r, &root, &mut stages, "cook-journal-start");
        assert!(start_journal["text"]
            .as_str()
            .unwrap()
            .contains("birthday cake"));
        assert!(start_journal["text"].as_str().unwrap().contains("egg"));
        let active_colour = start_journal["row"]["colour"].clone();
        assert_ne!(
            unstarted_colour, active_colour,
            "quest-list colour changes to active"
        );
        tutorial_lesson_377::close_main(&mut c, &mut r);
        pump_until(&mut c, &mut r, 15, |c| {
            c.main_modal_id < 0 && c.side_modal_id < 0
        });
    }

    if run_ingredients {
        if lesson == "ingredients" {
            let journal =
                open_cook_journal(&mut c, &mut r, &root, &mut stages, "cook-journal-start");
            assert!(journal["text"].as_str().unwrap().contains("birthday cake"));
            tutorial_lesson_377::close_main(&mut c, &mut r);
        }
        tutorial_lesson_377::tab(&mut c, &mut r, 3);
        if inventory_count(&c, EGG) == 0 {
            // The river bends west of the chicken pen. Cross the actual northern
            // bridge at z=3262 before following the east-bank farm road.
            let position = tile(&c);
            if position.0 < 3237 || position.1 < 3285 {
                if (3204..=3213).contains(&position.0) && (3211..=3217).contains(&position.1) {
                    leave_kitchen(&mut c, &mut r, &mut actions);
                } else if position.1 < 3244 {
                    walk_route(&mut c, &mut r, (3220, 3244), &mut actions);
                }
                walk_route(&mut c, &mut r, (3234, 3262), &mut actions);
                walk_route(&mut c, &mut r, (3243, 3262), &mut actions);
                walk_route(&mut c, &mut r, (3246, 3274), &mut actions);
                walk_route(&mut c, &mut r, (3239, 3285), &mut actions);
            }
            walk_route(&mut c, &mut r, (3239, 3293), &mut actions);
            walk_route(&mut c, &mut r, (3238, 3296), &mut actions);
            open_passage(&mut c, &mut r, "Gate", (3236, 3296), &mut actions);
            walk_route(&mut c, &mut r, (3234, 3296), &mut actions);
            pump_until(&mut c, &mut r, 90, |c| ground_item(c, EGG).is_some());
            let egg_ground = ground_item(&c, EGG)
                .expect("the actual chicken-farm ground stack contains the quest egg");
            let (egg_x, egg_z, _) = egg_ground;
            assert_eq!(inventory_count(&c, EGG), 0);
            actions.push(pickup_ground_item(&mut c, EGG, egg_x, egg_z));
            pump_until(&mut c, &mut r, 25, |c| inventory_count(c, EGG) == 1);
            save_mainland_stage(&mut c, &mut r, &root, "farm-egg", &mut stages);

            walk_route(&mut c, &mut r, (3238, 3296), &mut actions);
            walk_route(&mut c, &mut r, (3239, 3293), &mut actions);
            walk_route(&mut c, &mut r, (3239, 3285), &mut actions);
            walk_route(&mut c, &mut r, (3246, 3274), &mut actions);
        }
        if inventory_count(&c, BUCKET_MILK) == 0 {
            walk_route(&mut c, &mut r, (3246, 3274), &mut actions);
            walk_route(&mut c, &mut r, (3244, 3268), &mut actions);
            walk_route(&mut c, &mut r, (3243, 3262), &mut actions);
            walk_route(&mut c, &mut r, (3249, 3262), &mut actions);
            walk_route(&mut c, &mut r, (3252, 3266), &mut actions);
            open_passage(&mut c, &mut r, "Gate", (3253, 3266), &mut actions);
            walk_route(&mut c, &mut r, (3254, 3266), &mut actions);
            walk_route(&mut c, &mut r, (3256, 3270), &mut actions);

            assert_eq!(inventory_count(&c, BUCKET_EMPTY), 1);
            let cow = loaded_location(
                &c,
                "Dairy Cow",
                Some("Milk"),
                Some((COW_ZONE.0, COW_ZONE.1, 18)),
            )
            .expect("server-loaded dairy cow exposes its real Milk option");
            actions.push(operate_loaded_location(&mut c, cow));
            pump_until(&mut c, &mut r, 25, |c| {
                inventory_count(c, BUCKET_EMPTY) == 0 && inventory_count(c, BUCKET_MILK) == 1
            });
            save_mainland_stage(&mut c, &mut r, &root, "dairy-cow-milk", &mut stages);
        }

        let journal_with_milk_egg =
            open_cook_journal(&mut c, &mut r, &root, &mut stages, "cook-journal-milk-egg");
        let journal_text = journal_with_milk_egg["text"].as_str().unwrap();
        assert!(journal_has_collected(
            journal_text,
            "I have a bucket of milk with me"
        ));
        assert!(journal_has_collected(journal_text, "I have an egg with me"));
        tutorial_lesson_377::close_main(&mut c, &mut r);
        tutorial_lesson_377::tab(&mut c, &mut r, 3);
        pump_until(&mut c, &mut r, 15, |c| {
            c.main_modal_id < 0 && c.side_modal_id < 0
        });

        open_passage(&mut c, &mut r, "Gate", (3253, 3266), &mut actions);
        walk_route(&mut c, &mut r, (3254, 3266), &mut actions);
        walk_route(&mut c, &mut r, (3252, 3266), &mut actions);
        walk_route(&mut c, &mut r, (3243, 3262), &mut actions);
        walk_route(&mut c, &mut r, (3234, 3262), &mut actions);
        for point in WEST_FARM_ROAD {
            walk_route(&mut c, &mut r, point, &mut actions);
        }
        open_passage(&mut c, &mut r, "Gate", (3163, 3290), &mut actions);
        let wheat = loaded_location(&c, "wheat", Some("Pick"), None)
            .expect("loaded Lumbridge field has the real Pick option on wheat");
        actions.push(operate_loaded_location(&mut c, wheat));
        pump_until(&mut c, &mut r, 25, |c| inventory_count(c, GRAIN) == 1);
        save_mainland_stage(&mut c, &mut r, &root, "wheat-grain", &mut stages);
    }

    if run_mill {
        if inventory_count(&c, GRAIN) == 0 {
            // Flour/hopper vars have temporary scope and do not survive logout.
            // A resumed mill lesson picks replacement wheat, never restores vars.
            if tile(&c).1 >= 3302 {
                open_passage(&mut c, &mut r, "Large door", (3166, 3302), &mut actions);
                walk_route(&mut c, &mut r, (3166, 3301), &mut actions);
            }
            walk_route(&mut c, &mut r, (3166, 3292), &mut actions);
            walk_route(&mut c, &mut r, (3163, 3289), &mut actions);
            open_passage(&mut c, &mut r, "Gate", (3163, 3290), &mut actions);
            let wheat = loaded_location(&c, "wheat", Some("Pick"), None)
                .expect("the nearby field exposes its real Pick option");
            actions.push(operate_loaded_location(&mut c, wheat));
            pump_until(&mut c, &mut r, 25, |c| inventory_count(c, GRAIN) == 1);
            save_mainland_stage(&mut c, &mut r, &root, "wheat-grain", &mut stages);
        }
        let position = tile(&c);
        if position.0 < 3164 && position.1 < 3300 {
            open_passage(&mut c, &mut r, "Gate", (3163, 3290), &mut actions);
            walk_route(&mut c, &mut r, (3163, 3289), &mut actions);
        }
        walk_route(&mut c, &mut r, (3166, 3292), &mut actions);
        walk_route(&mut c, &mut r, (3166, 3301), &mut actions);
        open_passage(&mut c, &mut r, "Large door", (3166, 3302), &mut actions);
        walk_route(&mut c, &mut r, (3166, 3305), &mut actions);
        for _ in 0..2 {
            climb_windmill(&mut c, &mut r, &root, &mut stages, &mut actions, true);
        }
        assert_eq!(
            c.minusedlevel, 2,
            "content hopper coordinate is on plane two"
        );
        let hopper = loaded_location(
            &c,
            "Hopper",
            None,
            Some((WINDMILL_FLOOR.0, WINDMILL_FLOOR.1, 16)),
        )
        .expect("server-loaded Lumbridge windmill hopper is on its content plane");
        let hopper_id = hopper.id;
        actions.push(use_item_on_location(&mut c, GRAIN, hopper_id as i32));
        pump_until(&mut c, &mut r, 25, |c| {
            inventory_count(c, GRAIN) == 0 && chat_contains(c, "You put the grain in the hopper.")
        });
        save_mainland_stage(&mut c, &mut r, &root, "windmill-hopper-loaded", &mut stages);

        let lever = loaded_location(
            &c,
            "Hopper controls",
            Some("Operate"),
            Some((hopper.x, hopper.z, 16)),
        )
        .expect("server-loaded windmill controls expose the real Operate option");
        actions.push(operate_loaded_location(&mut c, lever));
        pump_until(&mut c, &mut r, 25, |c| {
            chat_contains(c, "The grain slides down the chute")
        });
        save_mainland_stage(
            &mut c,
            &mut r,
            &root,
            "windmill-controls-operated",
            &mut stages,
        );

        for _ in 0..2 {
            climb_windmill(&mut c, &mut r, &root, &mut stages, &mut actions, false);
        }
        assert_eq!(c.minusedlevel, 0);
        let flour_bin = loaded_location(
            &c,
            "Flour bin",
            Some("Empty"),
            Some((WINDMILL_FLOOR.0, WINDMILL_FLOOR.1, 16)),
        )
        .expect("server flour-bin multiloc changed to its flour-filled form");
        save_mainland_stage(&mut c, &mut r, &root, "windmill-bin-filled", &mut stages);
        actions.push(operate_loaded_location(&mut c, flour_bin));
        pump_until(&mut c, &mut r, 25, |c| {
            inventory_count(c, POT_EMPTY) == 0 && inventory_count(c, POT_FLOUR) == 1
        });
        save_mainland_stage(&mut c, &mut r, &root, "windmill-flour", &mut stages);

        let journal_with_all = open_cook_journal(
            &mut c,
            &mut r,
            &root,
            &mut stages,
            "cook-journal-all-ingredients",
        );
        let journal_text = journal_with_all["text"].as_str().unwrap();
        assert!(journal_has_collected(
            journal_text,
            "I have a bucket of milk with me"
        ));
        assert!(journal_has_collected(journal_text, "I have an egg with me"));
        assert!(journal_has_collected(
            journal_text,
            "I have a pot of flour with me"
        ));
        tutorial_lesson_377::close_main(&mut c, &mut r);
        pump_until(&mut c, &mut r, 15, |c| {
            c.main_modal_id < 0 && c.side_modal_id < 0
        });
    }

    let completion = if run_complete {
        let position = tile(&c);
        if (3158..=3174).contains(&position.0) && (3295..=3311).contains(&position.1) {
            open_passage(&mut c, &mut r, "Large door", (3166, 3302), &mut actions);
            walk_route(&mut c, &mut r, (3166, 3301), &mut actions);
            walk_route(&mut c, &mut r, (3166, 3292), &mut actions);
            for &point in WEST_FARM_ROAD.iter().rev() {
                walk_route(&mut c, &mut r, point, &mut actions);
            }
        }
        approach_cook(&mut c, &mut r, &mut actions);
        tutorial_lesson_377::tab(&mut c, &mut r, 2);
        let active_colour = json!(cook_journal_row(&c).expect("active quest row exists").1);
        let cook_type = npc_type(&c, "cook", "Talk-to");
        let cooking_xp_before = c.stat_xp[7];
        actions.push(operate_npc(
            &mut c,
            cook_type,
            MiniMenuAction::OP_NPC1,
            "Talk-to",
        ));
        pump_until(&mut c, &mut r, 25, |c| {
            c.chat_modal_id >= 0
                && dialogue_contains(c, "How are you getting on with finding the ingredients")
        });
        save_mainland_stage(&mut c, &mut r, &root, "cook-hand-in-prompt", &mut stages);
        for _ in 0..8 {
            if c.main_modal_id >= 0 {
                break;
            }
            assert!(
                c.chat_modal_id >= 0,
                "server hand-in dialogue remains open before reward"
            );
            let previous = body(&c);
            continue_chat(&mut c, &mut r);
            pump_until(&mut c, &mut r, 15, |c| {
                c.main_modal_id >= 0
                    || c.chat_modal_id < 0
                    || !c.resumed_pause_button && body(c) != previous
            });
        }
        pump_until(&mut c, &mut r, 30, |c| {
            c.main_modal_id >= 0
                && cook_journal_row(c).is_some_and(|(_, colour, _)| colour == QUEST_COMPLETE_COLOUR)
        });
        let quest_scroll = interface_text(&c, c.main_modal_id);
        assert!(quest_scroll.contains("You have completed the Cook's Assistant Quest!"));
        assert!(quest_scroll.contains("300 Cooking XP"));
        assert_eq!(inventory_count(&c, EGG), 0);
        assert_eq!(inventory_count(&c, BUCKET_MILK), 0);
        assert_eq!(inventory_count(&c, POT_FLOUR), 0);
        save_mainland_stage(&mut c, &mut r, &root, "cook-quest-scroll", &mut stages);
        // stat_advance uses tenths; UPDATE_STAT sends exp / 10 to Java.
        assert_eq!(c.stat_xp[7] - cooking_xp_before, 300);

        // Capture the completed journal and its real quest-list colour after the
        // server's completion queue opened the quest scroll.
        tutorial_lesson_377::close_main(&mut c, &mut r);
        pump_until(&mut c, &mut r, 15, |c| {
            c.main_modal_id < 0 && c.side_modal_id < 0
        });
        let completed_journal =
            open_cook_journal(&mut c, &mut r, &root, &mut stages, "cook-journal-complete");
        let completed_text = completed_journal["text"].as_str().unwrap();
        assert!(completed_text.contains("QUEST COMPLETE!"));
        assert!(completed_text.contains("lets me burn things less"));
        assert_ne!(
            active_colour, completed_journal["row"]["colour"],
            "quest-list colour changes again after server completion"
        );
        tutorial_lesson_377::close_main(&mut c, &mut r);
        pump_until(&mut c, &mut r, 15, |c| {
            c.main_modal_id < 0 && c.side_modal_id < 0
        });

        // A naturally received tutorial sword is dropped with its inventory Drop
        // option and recovered from the server's real ground-object stack.
        tutorial_lesson_377::tab(&mut c, &mut r, 3);
        assert_eq!(inventory_count(&c, BRONZE_SWORD), 1);
        let drop_tile = tile(&c);
        actions.push(drop_inventory_item(&mut c, BRONZE_SWORD));
        pump_until(&mut c, &mut r, 25, |c| {
            inventory_count(c, BRONZE_SWORD) == 0
                && c.world
                    .ground_object_at(
                        c.minusedlevel,
                        drop_tile.0 - c.map_build_base_x,
                        drop_tile.1 - c.map_build_base_z,
                    )
                    .is_some_and(|ground| {
                        [ground.top, ground.middle, ground.bottom]
                            .into_iter()
                            .flatten()
                            .any(|(id, _)| id == BRONZE_SWORD)
                    })
        });
        save_mainland_stage(&mut c, &mut r, &root, "tutorial-sword-dropped", &mut stages);
        actions.push(pickup_ground_item(
            &mut c,
            BRONZE_SWORD,
            drop_tile.0,
            drop_tile.1,
        ));
        pump_until(&mut c, &mut r, 25, |c| {
            inventory_count(c, BRONZE_SWORD) == 1
        });
        save_mainland_stage(
            &mut c,
            &mut r,
            &root,
            "tutorial-sword-recovered",
            &mut stages,
        );
        Some(
            json!({"quest_scroll":quest_scroll,"completed_journal":completed_journal,
        "cooking_xp_before":cooking_xp_before,"cooking_xp_after":c.stat_xp[7]}),
        )
    } else {
        None
    };

    let final_state = evidence_state(&c);
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
    let exit = c.take_session_exit_reason().unwrap().to_string();
    assert!(exit.contains("server_logout"));
    let receipt = json!({"engine_dir":engine,"user":user,"revision":377,"lesson":lesson,
        "actions":actions,"stages":stages,"final_state":final_state,"completion":completion,
        "outbound":*outbound.0.lock().unwrap(),"logout":exit});
    std::fs::write(
        root.join("rust-mainland-quest-shop.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!(
        "PASS: natural mainland lesson {lesson}, received-state assertions and server logout"
    );
}
