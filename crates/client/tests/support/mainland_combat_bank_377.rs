//! Natural mainland continuation. Only real inputs and server-received state.
use super::*;
use client::dash3d::client_entity::ClientEntity;

const BONES: i32 = 526;

// Same lossless PNG chunk/zlib pattern as revision_377_render. The image
// dependency deliberately enables JPEG only; do not change production codecs.
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

#[test]
fn lossless_capture_keeps_rgb_scanlines_and_chunk_checksums() {
    use std::io::Read;
    let png = lossless_png(&[0x123456, 0, 0xffffff, 0xabcdef], 2, 2);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut offset = 8;
    let mut kinds = Vec::new();
    while offset < png.len() {
        let length = u32::from_be_bytes(png[offset..offset + 4].try_into().unwrap()) as usize;
        let body = &png[offset + 8..offset + 8 + length];
        let kind = &png[offset + 4..offset + 8];
        kinds.push(kind);
        assert_eq!(
            png_crc32(&png[offset + 4..offset + 8 + length]),
            u32::from_be_bytes(
                png[offset + 8 + length..offset + 12 + length]
                    .try_into()
                    .unwrap()
            )
        );
        if kind == b"IDAT" {
            let mut scanlines = Vec::new();
            flate2::read::ZlibDecoder::new(body)
                .read_to_end(&mut scanlines)
                .unwrap();
            assert_eq!(
                scanlines,
                [0, 0x12, 0x34, 0x56, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xab, 0xcd, 0xef]
            );
        }
        offset += 12 + length;
    }
    assert_eq!(kinds, [b"IHDR", b"IDAT", b"IEND"]);
}

fn actor_state(e: &ClientEntity) -> Value {
    json!({"x":e.x,"z":e.z,"yaw":e.yaw,"size":e.size,"height":e.height,
        "readyanim":e.readyanim,"secondary_anim":e.secondary_anim,
        "secondary_anim_frame":e.secondary_anim_frame,"secondary_anim_cycle":e.secondary_anim_cycle,
        "primary_anim":e.primary_anim,"primary_anim_frame":e.primary_anim_frame,
        "primary_anim_cycle":e.primary_anim_cycle,"primary_anim_delay":e.primary_anim_delay,
        "primary_anim_loop":e.primary_anim_loop,"spotanim_id":e.spotanim_id,
        "spotanim_frame":e.spotanim_frame,"spotanim_height":e.spotanim_height,
        "combat_cycle":e.combat_cycle,"health":e.health,"total_health":e.total_health,
        "damage_values":e.damage_values,"damage_types":e.damage_types,"damage_cycles":e.damage_cycles,
        "chat_message":e.chat_message,"chat_timer":e.chat_timer,
        "chat_colour":e.chat_colour,"chat_effect":e.chat_effect})
}

fn combat_bank_state(c: &Client) -> Value {
    let mut snapshot = state(c);
    let p = c.local_player.as_ref().unwrap();
    snapshot["player"] = json!({"entity":actor_state(&p.entity),"name":p.name,"ready":p.ready,
        "gender":p.gender,"headicons":p.headicons,"appearance":p.appearance,"colour":p.colour,
        "combat_level":p.combat_level,"base_id":p.base_id,"y":p.y,
        "transmog":p.transmog,"skill_level":p.skill_level});
    snapshot["npc_actors"] = json!(c
        .npc_ids
        .iter()
        .take(c.npc_count as usize)
        .filter_map(|slot| {
            let npc = c.npc[*slot as usize].as_ref()?;
            Some(json!({"slot":slot,"type":npc.r#type,"entity":actor_state(&npc.entity)}))
        })
        .collect::<Vec<_>>());
    snapshot["varps"] = json!(c.var);
    snapshot["side_icon"] = json!(c.side_icon);
    snapshot["components"] = json!(c
        .ifaces_merged()
        .filter(|i| {
            [
                c.main_modal_id,
                c.side_modal_id,
                c.chat_modal_id,
                c.side_icon[c.active_icon as usize],
            ]
            .contains(&i.layer_id)
        })
        .map(
            |i| json!({"component":i.id,"layer":i.layer_id,"text":i.text,"colour":i.colour,
        "objects":i.link_obj_type,"counts":i.link_obj_number,"hide":i.hide,
        "scroll_pos":i.scroll_pos,"scroll_height":i.scroll_height,"x":i.x,"y":i.y,
        "model1_type":i.model1_type,"model1_id":i.model1_id,"model_anim":i.model_anim,
        "anim_frame":i.anim_frame,"anim_cycle":i.anim_cycle,"model_xan":i.model_xan,
        "model_yan":i.model_yan,"model_zoom":i.model_zoom,"graphic_name":i.graphic_name})
        )
        .collect::<Vec<_>>());
    snapshot
}

fn stage(c: &mut Client, r: &mut Renderer, root: &Path, name: &str, stages: &mut Vec<Value>) {
    let FrameOutput::PixMap(map) = r.mainredraw(c) else {
        panic!("CPU proof required")
    };
    let path = root.join(format!("rust-{name}.png"));
    std::fs::write(
        &path,
        lossless_png(&map.pixels, map.width as usize, map.height as usize),
    )
    .unwrap();
    if let Some(minimap) = &r.minimap {
        std::fs::write(
            root.join(format!("rust-{name}-minimap-raw.png")),
            lossless_png(&minimap.data, minimap.wi as usize, minimap.hi as usize),
        )
        .unwrap();
    }
    let icons = (0..r.active_map_function_count as usize)
        .map(|index| {
            json!({"x":r.active_map_function_x[index],"z":r.active_map_function_z[index],
            "sprite":r.active_map_functions[index].as_ref().map(|p| json!({
                "width":p.wi,"height":p.hi,"xof":p.xof,"yof":p.yof,
                "original_width":p.owi,"original_height":p.ohi,"pixels":p.data}))})
        })
        .collect::<Vec<_>>();
    let snapshot = json!({"stage":name,"screenshot":path,"roof_top":r.roof_check(c),
        "minimap_functions":icons,"state":combat_bank_state(c)});
    std::fs::write(
        root.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();
    eprintln!(
        "377 stage11 {name} tile={:?} xp={} level={}",
        tile(c),
        c.stat_xp[0],
        c.stat_base_level[0]
    );
    stages
        .push(json!({"stage":name,"snapshot":root.join(format!("{name}.json")),"screenshot":path}));
    std::fs::write(
        root.join("stages.json"),
        serde_json::to_vec_pretty(stages).unwrap(),
    )
    .unwrap();
}

fn worn(c: &Client, object: i32) -> bool {
    c.ifaces_merged().any(|i| {
        i.layer_id == c.side_icon[4]
            && i.link_obj_type
                .as_ref()
                .is_some_and(|ids| ids.contains(&(object + 1)))
    })
}

fn inventory_option(
    c: &mut Client,
    component: i32,
    object: i32,
    action: i32,
    label: &str,
) -> Value {
    let view = c
        .if_(component as usize)
        .expect("received inventory component");
    let slot = view
        .link_obj_type
        .as_ref()
        .unwrap()
        .iter()
        .position(|id| *id == object + 1)
        .expect("operation targets actual received object") as i32;
    let option = match action {
        MiniMenuAction::INV_BUTTON1 => 0,
        MiniMenuAction::INV_BUTTON4 => 3,
        _ => panic!("only assigned bank inventory options"),
    };
    assert_eq!(view.iop[option].as_deref(), Some(label));
    let receipt = json!({"component":component,"object":object,"slot":slot,"action":action,
        "option":label,"input":"production doAction on real received bank slot"});
    c.menu_action[0] = action;
    c.menu_param_a[0] = object;
    c.menu_param_b[0] = slot;
    c.menu_param_c[0] = component;
    c.doAction(0);
    eprintln!("377 stage11 inventory {receipt}");
    receipt
}

fn choose_attack_style(c: &mut Client, r: &mut Renderer, actions: &mut Vec<Value>) {
    tutorial_lesson_377::tab(c, r, 0);
    let root = c.side_icon[0];
    let button = c
        .ifaces_merged()
        .find(|i| {
            i.layer_id == root
                && i.button_type == 5
                && i.script_operand.as_deref() == Some(&[0][..])
        })
        .expect("actual weapon Accurate attack style")
        .id;
    let varp = c.if_(button as usize).unwrap().scripts.as_ref().unwrap()[0][1] as usize;
    let generation = c.gens.player_info;
    click_component(c, r, root, button, (553, 205));
    pump_until(c, r, 10, |c| {
        c.var.get(varp) == Some(&0) && c.gens.player_info >= generation + 2
    });
    actions.push(
        json!({"component":button,"interface":root,"style":"Accurate",
        "varp":varp,"value":0,"input":"rendered attack-style component click"}),
    );
}

fn nearest_goblin(c: &Client) -> i32 {
    let player = tile(c);
    c.npc_ids
        .iter()
        .take(c.npc_count as usize)
        .copied()
        .filter(|slot| {
            c.npc[*slot as usize]
                .as_ref()
                .and_then(|npc| npc.r#type)
                .is_some_and(|kind| c.cache.npcs[kind].name.eq_ignore_ascii_case("Goblin"))
        })
        .min_by_key(|slot| {
            let npc = c.npc[*slot as usize].as_ref().unwrap();
            (npc.route_x[0] + c.map_build_base_x - player.0).abs()
                + (npc.route_z[0] + c.map_build_base_z - player.1).abs()
        })
        .expect("naturally populated Lumbridge goblin")
}

fn fight_goblin(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    kill: usize,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) -> Value {
    let slot = nearest_goblin(c);
    let npc = c.npc[slot as usize].as_ref().unwrap();
    let kind = npc.r#type.unwrap();
    assert_eq!(c.cache.npcs[kind].op[1].as_deref(), Some("Attack"));
    let xp_before = c.stat_xp[0];
    let levels_before = c.stat_base_level.clone();
    let inventory_before = inventory_count(c, BONES);
    let action = json!({"slot":slot,"type":kind,"name":c.cache.npcs[kind].name,
        "tile":[npc.route_x[0]+c.map_build_base_x,npc.route_z[0]+c.map_build_base_z],
        "player":tile(c),"input":"production doAction(OP_NPC2), real mainland goblin"});
    actions.push(action.clone());
    c.menu_action[0] = MiniMenuAction::OP_NPC2;
    c.menu_param_a[0] = slot;
    c.menu_param_b[0] = 0;
    c.menu_param_c[0] = 0;
    c.doAction(0);
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut hits_frame = false;
    let mut saw_zero_health = false;
    let mut events = Vec::new();
    let mut last = Value::Null;
    let mut food_tick = c.gens.player_info;
    while Instant::now() < deadline {
        c.shell.latch_click();
        c.mainloop();
        assert!(
            c.ingame,
            "combat session ended: {:?}",
            c.take_session_exit_reason()
        );
        drop(r.mainredraw(c));
        let actor = c.npc[slot as usize]
            .as_ref()
            .map(|npc| actor_state(&npc.entity));
        let event = json!({"player_info":c.gens.player_info,"cycle":c.loop_cycle,
            "npc":actor,"attack_xp":c.stat_xp[0],"levels":c.stat_base_level,
            "player_health":c.stat_effective_level[3]});
        if event["npc"] != last["npc"]
            || event["attack_xp"] != last["attack_xp"]
            || event["levels"] != last["levels"]
        {
            events.push(event.clone());
            last = event;
        }
        if let Some(npc) = c.npc[slot as usize].as_ref() {
            saw_zero_health |= npc.total_health > 0 && npc.health == 0;
            if !hits_frame
                && npc.combat_cycle > c.loop_cycle
                && npc.total_health > 0
                && npc.damage_cycles.iter().any(|cycle| *cycle > c.loop_cycle)
            {
                stage(
                    c,
                    r,
                    root,
                    &format!("combat-{kill}-hitsplat-healthbar"),
                    stages,
                );
                hits_frame = true;
            }
        } else if saw_zero_health && c.stat_xp[0] > xp_before && ground_item(c, BONES).is_some() {
            break;
        }
        if c.stat_effective_level[3] <= 4 && c.gens.player_info >= food_tick + 4 {
            if let Some(food) = [315, 2309]
                .into_iter()
                .find(|object| inventory_count(c, *object) > 0)
            {
                let food_slot = inventory_slot(c, food);
                c.menu_action[0] = MiniMenuAction::OP_HELD1;
                c.menu_param_a[0] = food;
                c.menu_param_b[0] = food_slot;
                c.menu_param_c[0] = 3214;
                c.doAction(0);
                food_tick = c.gens.player_info;
                actions.push(json!({"object":food,"slot":food_slot,"option":"Eat",
                    "input":"production inventory action on naturally earned tutorial food"}));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::write(
        root.join(format!("combat-{kill}-events.json")),
        serde_json::to_vec_pretty(&events).unwrap(),
    )
    .unwrap();
    assert!(hits_frame, "received hitsplat and healthbar frame");
    assert!(
        saw_zero_health,
        "received target zero-health update before death"
    );
    assert!(
        c.npc[slot as usize].is_none(),
        "goblin death removed actual NPC"
    );
    assert!(
        c.stat_xp[0] > xp_before,
        "server awarded attack XP for real damage"
    );
    stage(c, r, root, &format!("combat-{kill}-dead-loot"), stages);
    if c.chat_modal_id >= 0 {
        dismiss_chat(c, r);
    }
    let (x, z, _) = ground_item(c, BONES).expect("natural goblin bones drop");
    actions.push(pickup_ground_item(c, BONES, x, z));
    pump_until(c, r, 30, |c| {
        inventory_count(c, BONES) == inventory_before + 1
    });
    stage(c, r, root, &format!("combat-{kill}-loot-picked-up"), stages);
    json!({"target":action,"xp_before":xp_before,"xp_after":c.stat_xp[0],
        "levels_before":levels_before,"levels_after":c.stat_base_level,
        "hitsplat_healthbar":hits_frame,"received_zero_health":saw_zero_health,
        "loot":{"object":BONES,"tile":[x,z],"before":inventory_before,
            "after":inventory_count(c,BONES)}})
}

fn combat(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) -> Vec<Value> {
    stage(c, r, root, "lumbridge-minimap", stages);
    tutorial_lesson_377::tab(c, r, 3);
    for object in [BRONZE_SWORD, 1171] {
        if !worn(c, object) {
            let slot = inventory_slot(c, object);
            c.menu_action[0] = MiniMenuAction::OP_HELD2;
            c.menu_param_a[0] = object;
            c.menu_param_b[0] = slot;
            c.menu_param_c[0] = 3214;
            c.doAction(0);
            pump_until(c, r, 10, |c| worn(c, object));
            actions.push(json!({"object":object,"slot":slot,"option":"Wield",
                "input":"production inventory action on naturally earned tutorial equipment"}));
        }
    }
    choose_attack_style(c, r, actions);
    stage(c, r, root, "mainland-attack-styles", stages);
    // The fountain occupies (3222,3226); walk around its south side.
    for point in [
        (3220, 3222),
        (3226, 3222),
        (3230, 3226),
        (3235, 3226),
        (3244, 3240),
        (3248, 3245),
    ] {
        walk_route(c, r, point, actions);
    }
    stage(c, r, root, "goblins-approach", stages);
    let initial_level = c.stat_base_level[0];
    let mut kills = Vec::new();
    for kill in 1..=12 {
        kills.push(fight_goblin(c, r, root, kill, stages, actions));
        if c.stat_base_level[0] > initial_level {
            break;
        }
    }
    assert!(
        c.stat_base_level[0] > initial_level,
        "natural combat crossed an attack-level threshold"
    );
    click(c, r, 583, 187, 1);
    pump_until(c, r, 10, |c| c.active_icon == 1);
    stage(c, r, root, "combat-xp-level", stages);
    kills
}

fn bank_count(c: &Client, component: i32, object: i32) -> i32 {
    let view = c.if_(component as usize).expect("received bank inventory");
    view.link_obj_type
        .as_ref()
        .unwrap()
        .iter()
        .zip(view.link_obj_number.as_ref().unwrap())
        .filter(|(id, _)| **id == object + 1)
        .map(|(_, count)| *count)
        .sum()
}

fn bank(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) -> Value {
    for point in [
        (3244, 3240),
        (3235, 3226),
        (3230, 3226),
        (3226, 3222),
        (3220, 3222),
        (3220, 3244),
        (3203, 3244),
        (3183, 3244),
        (3163, 3244),
        (3143, 3244),
        (3123, 3244),
        (3103, 3244),
        (3097, 3244),
    ] {
        walk_route(c, r, point, actions);
        if point == (3220, 3222) {
            stage(c, r, root, "lumbridge-minimap", stages);
        }
    }
    stage(c, r, root, "draynor-minimap", stages);
    let banker = npc_type(c, "banker", "Bank");
    actions.push(operate_npc(c, banker, MiniMenuAction::OP_NPC3, "Bank"));
    pump_until(c, r, 30, |c| {
        c.main_modal_id >= 0
            && c.side_modal_id >= 0
            && interface_text(c, c.main_modal_id).contains("Bank of RuneScape")
    });
    stage(c, r, root, "draynor-bank-no-pin", stages);
    let (bank_component, _, _) = shop_inventory_component(c, false);
    let side_component = shop_side_player_inventory(c);
    let object = BONES;
    let held_before = inventory_count(c, object);
    assert!(
        held_before > 1,
        "multiple natural combat drops distinguish Deposit All from Deposit 1"
    );
    let bank_before = bank_count(c, bank_component, object);
    actions.push(inventory_option(
        c,
        side_component,
        object,
        MiniMenuAction::INV_BUTTON4,
        "Deposit All",
    ));
    pump_until(c, r, 15, |c| {
        inventory_count(c, object) == 0
            && bank_count(c, bank_component, object) == bank_before + held_before
    });
    stage(c, r, root, "bank-deposit-all", stages);
    actions.push(inventory_option(
        c,
        bank_component,
        object,
        MiniMenuAction::INV_BUTTON1,
        "Withdraw 1",
    ));
    pump_until(c, r, 15, |c| {
        inventory_count(c, object) == 1
            && bank_count(c, bank_component, object) == bank_before + held_before - 1
    });
    stage(c, r, root, "bank-withdraw-one", stages);
    actions.push(inventory_option(
        c,
        side_component,
        object,
        MiniMenuAction::INV_BUTTON1,
        "Deposit 1",
    ));
    pump_until(c, r, 15, |c| {
        inventory_count(c, object) == 0
            && bank_count(c, bank_component, object) == bank_before + held_before
    });
    stage(c, r, root, "bank-deposit-one", stages);
    tutorial_lesson_377::close_main(c, r);
    actions.push(operate_npc(c, banker, MiniMenuAction::OP_NPC1, "Talk-to"));
    pump_until(c, r, 20, |c| c.chat_modal_id >= 0 && !body(c).is_empty());
    actions.push(choose_chat_option(
        c,
        r,
        "I'd like to check my PIN settings.",
    ));
    pump_until(c, r, 15, |c| {
        c.main_modal_id >= 0 && interface_text(c, c.main_modal_id).contains("No PIN set")
    });
    stage(c, r, root, "bank-pin-free-settings", stages);
    let pin_state = interface_text(c, c.main_modal_id);
    tutorial_lesson_377::close_main(c, r);
    json!({"object":object,"held_before":held_before,"held_after":inventory_count(c,object),
        "bank_before":bank_before,"bank_after":bank_before+held_before,"bank_component":bank_component,
        "side_component":side_component,"direct_open_without_keypad":true,"pin_settings":pin_state})
}

fn varrock(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    for point in [
        (3103, 3244),
        (3103, 3263),
        (3123, 3263),
        (3143, 3263),
        (3163, 3263),
        (3183, 3263),
        (3203, 3263),
        (3216, 3263),
        (3216, 3279),
        (3234, 3295),
        (3253, 3310),
        (3264, 3328),
        (3264, 3347),
        (3251, 3364),
        (3240, 3380),
        (3222, 3400),
    ] {
        walk_route(c, r, point, actions);
    }
    stage(c, r, root, "varrock-minimap", stages);
}

#[test]
#[ignore = "requires Fairy Ring 377 engine and a naturally completed tutorial t2a account"]
fn natural_mainland_combat_and_bank_377() {
    let root =
        PathBuf::from(std::env::var("CLIENT_377_LIVE_DIR").expect("explicit evidence directory"));
    let cache = std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("matched running cache");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("explicit t2a account");
    assert!(user.starts_with("t2a"));
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists());
    }
    let lesson = std::env::var("CLIENT_377_STAGE11").unwrap_or_else(|_| "full".into());
    assert!(matches!(
        lesson.as_str(),
        "full" | "combat" | "bank" | "varrock"
    ));
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
        path: root.join("outbound.json"),
    };
    c.set_login_deadline(Instant::now() + Duration::from_secs(10));
    c.login(&user, "t2apass", false).unwrap();
    pump_until(&mut c, &mut r, 90, |c| {
        c.scene_state == 2
            && c.npc_count > 0
            && c.side_icon[0] >= 0
            && inventory_count(c, 1351) == 1
            && c.stat_base_level[0] > 0
            && c.stat_base_level[3] > 0
    });
    assert_eq!(c.minusedlevel, 0);
    let mut stages = Vec::new();
    let mut actions = Vec::new();
    let kills = matches!(lesson.as_str(), "full" | "combat")
        .then(|| combat(&mut c, &mut r, &root, &mut stages, &mut actions));
    let bank = matches!(lesson.as_str(), "full" | "bank")
        .then(|| bank(&mut c, &mut r, &root, &mut stages, &mut actions));
    if matches!(lesson.as_str(), "full" | "varrock") {
        varrock(&mut c, &mut r, &root, &mut stages, &mut actions);
    }
    let final_state = combat_bank_state(&c);
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
    assert!(!c.ingame, "server acknowledged logout");
    let exit = c.take_session_exit_reason().unwrap().to_string();
    assert!(exit.contains("server_logout"));
    let receipt = json!({"engine_dir":engine,"user":user,"revision":377,"lesson":lesson,
        "actions":actions,"kills":kills,"bank":bank,"stages":stages,"final_state":final_state,
        "outbound":*outbound.0.lock().unwrap(),"logout":exit,
        "interventions":[],"xp_presentation":"Java377 stat/level updates; no invented modern floating XP overlay"});
    std::fs::write(
        root.join("combat-bank.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!(
        "PASS: natural mainland stage11 {lesson}, received-state assertions and server logout"
    );
}
