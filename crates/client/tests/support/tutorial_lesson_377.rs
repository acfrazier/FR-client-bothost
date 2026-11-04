//! Natural continuation of the bread proof. Each lesson can resume the same
//! account after a server-acknowledged logout. No tutorial state is assigned.
use super::*;

pub(super) fn tab(c: &mut Client, r: &mut Renderer, index: usize) {
    let (x, y) = match index {
        0 => (555, 187),
        2 => (612, 187),
        3 => (650, 187),
        4 => (681, 187),
        5 => (709, 187),
        6 => (740, 187),
        8 => (587, 485),
        9 => (614, 485),
        12 => (711, 485),
        13 => (741, 485),
        _ => panic!("lesson has no tab {index}"),
    };
    assert_ne!(c.side_icon[index], -1, "server must enable the tab");
    click(c, r, x, y, 1);
    pump_until(c, r, 10, |c| c.active_icon == index as i32);
}

fn dialogue(c: &mut Client, r: &mut Renderer, root: &Path, name: &str, stages: &mut Vec<Value>) {
    // The longest assigned conversation has nine pages. This ceiling detects
    // a fixture that does not advance. It is not a gameplay retry allowance.
    for page in 0..16 {
        if c.chat_modal_id < 0 {
            return;
        }
        save_stage(c, r, root, &format!("{name}-{page}"), stages);
        if !c
            .ifaces_merged()
            .any(|i| i.layer_id == c.chat_modal_id && i.button_type == 6)
        {
            return;
        }
        let previous = body(c);
        continue_chat(c, r);
        pump_until(c, r, 10, |c| {
            c.chat_modal_id < 0 || !c.resumed_pause_button && body(c) != previous
        });
    }
    panic!("dialogue exceeds script page count: {}", state(c));
}

fn speak(c: &mut Client, r: &mut Renderer, root: &Path, name: &str, stages: &mut Vec<Value>) {
    assert_eq!(c.hint_type, 1, "server indicates the next instructor");
    let kind = c.npc[c.hint_npc as usize].as_ref().unwrap().r#type.unwrap();
    talk_to(c, kind);
    pump_until(c, r, 30, |c| {
        c.chat_modal_id >= 0 && !c.resumed_pause_button && !body(c).is_empty()
    });
    dialogue(c, r, root, name, stages);
}

fn choose(c: &mut Client, r: &mut Renderer, text: &str) {
    pump_until(c, r, 10, |c| {
        c.chat_modal_id >= 0
            && c.ifaces_merged()
                .any(|i| i.layer_id == c.chat_modal_id && i.text == text)
    });
    let id = c
        .ifaces_merged()
        .find(|i| i.layer_id == c.chat_modal_id && i.text == text)
        .expect("choice belongs to the actual server-opened dialogue")
        .id;
    let root = c.chat_modal_id;
    click_component(c, r, root, id, (17, 357));
}

fn hint_location(c: &mut Client, option: usize) -> Value {
    assert_eq!(c.hint_type, 2);
    let (hx, hz) = (c.hint_tile_x, c.hint_tile_z);
    let target = (0..104)
        .flat_map(|x| (0..104).map(move |z| (x, z)))
        .filter(|(x, z)| {
            (*x + c.map_build_base_x - hx).abs() <= 1 && (*z + c.map_build_base_z - hz).abs() <= 1
        })
        .flat_map(|(x, z)| {
            [
                c.world.wall_type(c.minusedlevel, x, z),
                c.world.decor_type(c.minusedlevel, x, z),
                c.world.scene_type(c.minusedlevel, x, z),
                c.world.gd_type(c.minusedlevel, x, z),
            ]
        })
        .filter(|code| {
            *code != 0
                && c.cache.locs[((code >> 14) & 0x7fff) as usize]
                    .op
                    .get(option)
                    .is_some_and(Option::is_some)
        })
        .min_by_key(|code| {
            ((code & 0x7f) + c.map_build_base_x - hx).abs()
                + (((code >> 7) & 0x7f) + c.map_build_base_z - hz).abs()
        })
        .expect("server hint is adjacent to an actual loaded actionable location");
    location_action(c, target, option)
}

fn location_action(c: &mut Client, code: i32, option: usize) -> Value {
    let id = (code >> 14) & 0x7fff;
    let x = code & 0x7f;
    let z = (code >> 7) & 0x7f;
    let receipt = json!({"location":id,"typecode":code,"tile":[x+c.map_build_base_x,z+c.map_build_base_z],
        "option_index":option,"option":c.cache.locs[id as usize].op[option],"name":c.cache.locs[id as usize].name,
        "input":"production doAction on actual scene location"});
    c.menu_action[0] = [MiniMenuAction::OP_LOC1, MiniMenuAction::OP_LOC2][option];
    c.menu_param_a[0] = code;
    c.menu_param_b[0] = x;
    c.menu_param_c[0] = z;
    c.doAction(0);
    eprintln!("377 lesson location {receipt}");
    receipt
}

fn object_count(c: &Client, component: usize, object: i32) -> i32 {
    let view = c
        .if_(component)
        .expect("server-populated inventory component");
    view.link_obj_type
        .as_ref()
        .unwrap()
        .iter()
        .zip(view.link_obj_number.as_ref().unwrap())
        .filter(|(id, _)| **id == object + 1)
        .map(|(_, count)| *count)
        .sum()
}

fn inventory_operation(c: &mut Client, component: i32, object: i32, action: i32) -> Value {
    let slot = c
        .if_(component as usize)
        .unwrap()
        .link_obj_type
        .as_ref()
        .unwrap()
        .iter()
        .position(|id| *id == object + 1)
        .expect("object exists in actual server inventory") as i32;
    let receipt = json!({"component":component,"object":object,"slot":slot,"action":action,
        "input":"production doAction with actual server-populated inventory slot"});
    c.menu_action[0] = action;
    c.menu_param_a[0] = object;
    c.menu_param_b[0] = slot;
    c.menu_param_c[0] = component;
    c.doAction(0);
    eprintln!("377 lesson inventory {receipt}");
    receipt
}

fn mining(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("Mining and smithing"));
    walk_to(c, r, (3083, 9510), actions);
    save_stage(c, r, root, "mining-approach", stages);
    speak(c, r, root, "mining-introduction", stages);
    pump_until(c, r, 10, |c| tutorial_text(c).contains("Prospecting"));
    for (id, ore) in [(3042, "copper"), (3043, "tin")] {
        let (code, _, _) = scene_location(c, id).expect("actual tutorial rock");
        actions.push(location_action(c, code, 1));
        pump_until(c, r, 20, |c| {
            body(c).contains(&format!("This rock contains {ore}"))
        });
        dialogue(c, r, root, &format!("prospect-{ore}"), stages);
    }
    speak(c, r, root, "mining-tools", stages);
    assert_eq!(inventory_count(c, 1265), 1);
    for (id, object, ore) in [(3043, 438, "tin"), (3042, 436, "copper")] {
        let (code, _, _) = scene_location(c, id).expect("actual replenished tutorial rock");
        let xp = c.stat_xp[14];
        actions.push(location_action(c, code, 0));
        pump_until(c, r, 30, |c| {
            inventory_count(c, object) == 1
                && c.stat_xp[14] > xp
                && body(c).contains(&format!("mine some {ore}"))
        });
        dialogue(c, r, root, &format!("mine-{ore}"), stages);
    }
    assert!(tutorial_text(c).contains("Smelting."));
    actions.push(use_item_on_location(c, 436, 3044));
    pump_until(c, r, 20, |c| {
        inventory_count(c, 2349) == 1
            && inventory_count(c, 436) == 0
            && inventory_count(c, 438) == 0
            && tutorial_text(c).contains("made a bronze bar")
    });
    save_stage(c, r, root, "bronze-smelted", stages);
    speak(c, r, root, "smithing-hammer", stages);
    assert_eq!(inventory_count(c, 2347), 1);
    actions.push(use_item_on_location(c, 2349, 2783));
    pump_until(c, r, 20, |c| {
        c.main_modal_id >= 0
            && c.ifaces_merged().any(|i| {
                i.layer_id == c.main_modal_id
                    && i.link_obj_type
                        .as_ref()
                        .is_some_and(|types| types.contains(&1206))
            })
    });
    save_stage(c, r, root, "smithing-interface", stages);
    let component = c
        .ifaces_merged()
        .find(|i| {
            i.layer_id == c.main_modal_id
                && i.link_obj_type
                    .as_ref()
                    .is_some_and(|types| types.contains(&1206))
        })
        .unwrap()
        .id;
    let xp = c.stat_xp[13];
    actions.push(inventory_operation(
        c,
        component,
        1205,
        MiniMenuAction::INV_BUTTON1,
    ));
    pump_until(c, r, 20, |c| {
        inventory_count(c, 1205) == 1
            && inventory_count(c, 2349) == 0
            && c.stat_xp[13] > xp
            && tutorial_text(c).contains("finished this area")
    });
    save_stage(c, r, root, "dagger-smithed", stages);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 30, |c| {
        tutorial_text(c).contains("In this area you will find out about combat")
    });
    save_stage(c, r, root, "combat-entry", stages);
}

fn worn_component(c: &Client, object: i32) -> i32 {
    c.ifaces_merged()
        .find(|i| {
            i.layer_id == c.side_icon[4]
                && i.link_obj_type
                    .as_ref()
                    .is_some_and(|types| types.contains(&(object + 1)))
        })
        .expect("actual worn inventory contains equipment")
        .id
}

fn equipped(c: &Client, object: i32) -> bool {
    c.ifaces_merged().any(|i| {
        i.layer_id == c.side_icon[4]
            && i.link_obj_type
                .as_ref()
                .is_some_and(|types| types.contains(&(object + 1)))
    })
}

fn attack(c: &mut Client) -> Value {
    assert_eq!(c.hint_type, 1);
    attack_rat(c, c.hint_npc)
}

fn attack_rat(c: &mut Client, slot: i32) -> Value {
    let npc = c.npc[slot as usize]
        .as_ref()
        .expect("actual server-populated rat");
    assert_eq!(npc.r#type, Some(950));
    let receipt = json!({"slot":slot,"type":950,"tile":[npc.route_x[0]+c.map_build_base_x,npc.route_z[0]+c.map_build_base_z],
        "player":tile(c),"input":"production doAction(OP_NPC2) on actual server-populated rat"});
    c.menu_action[0] = MiniMenuAction::OP_NPC2;
    c.menu_param_a[0] = slot;
    c.menu_param_b[0] = 0;
    c.menu_param_c[0] = 0;
    c.doAction(0);
    eprintln!("377 tutorial attack {receipt}");
    receipt
}

fn combat(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("In this area you will find out about combat"));
    speak(c, r, root, "combat-introduction", stages);
    pump_until(c, r, 10, |c| c.tut_flash_icon == 4);
    tab(c, r, 4);
    pump_until(c, r, 10, |c| {
        tutorial_text(c).contains("This is your worn inventory")
    });
    save_stage(c, r, root, "equipment-screen", stages);
    tab(c, r, 3);
    actions.push(inventory_operation(c, 3214, 1205, MiniMenuAction::OP_HELD2));
    pump_until(c, r, 10, |c| {
        equipped(c, 1205) && tutorial_text(c).contains("holding your dagger")
    });
    save_stage(c, r, root, "dagger-wielded", stages);
    speak(c, r, root, "sword-and-shield", stages);
    assert_eq!(inventory_count(c, 1277), 1);
    assert_eq!(inventory_count(c, 1171), 1);
    tab(c, r, 4);
    let worn = worn_component(c, 1205);
    actions.push(inventory_operation(
        c,
        worn,
        1205,
        MiniMenuAction::INV_BUTTON1,
    ));
    pump_until(c, r, 10, |c| {
        !equipped(c, 1205) && inventory_count(c, 1205) == 1
    });
    save_stage(c, r, root, "dagger-removed", stages);
    tab(c, r, 3);
    for object in [1277, 1171] {
        actions.push(inventory_operation(
            c,
            3214,
            object,
            MiniMenuAction::OP_HELD2,
        ));
        pump_until(c, r, 10, |c| equipped(c, object));
    }
    pump_until(c, r, 10, |c| c.tut_flash_icon == 0);
    save_stage(c, r, root, "sword-shield-equipped", stages);
    tab(c, r, 0);
    pump_until(c, r, 10, |c| {
        tutorial_text(c).contains("This is your Combat interface")
    });
    save_stage(c, r, root, "combat-tab", stages);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 20, |c| tutorial_text(c).contains("To attack the rat"));
    save_stage(c, r, root, "rat-pit", stages);
    let xp = c.stat_xp[0] + c.stat_xp[1] + c.stat_xp[2];
    actions.push(attack(c));
    pump_until(c, r, 20, |c| {
        tutorial_text(c).contains("Sit back and watch")
    });
    save_stage(c, r, root, "melee-fighting", stages);
    pump_until(c, r, 180, |c| {
        tutorial_text(c).contains("made your first kill")
    });
    assert!(c.stat_xp[0] + c.stat_xp[1] + c.stat_xp[2] > xp);
    save_stage(c, r, root, "melee-rat-killed", stages);
    let (code, _, _) = scene_location(c, 3022).expect("actual rat pit gate");
    actions.push(location_action(c, code, 0));
    pump_until(c, r, 20, |c| tile(c).0 >= 3111);
    save_stage(c, r, root, "rat-pit-exit", stages);
    combat_ranged(c, r, root, stages, actions);
}

fn combat_ranged(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("made your first kill"));
    speak(c, r, root, "ranged-instruction", stages);
    assert_eq!(inventory_count(c, 841), 1);
    assert_eq!(inventory_count(c, 882), 50);
    tab(c, r, 3);
    for object in [841, 882] {
        actions.push(inventory_operation(
            c,
            3214,
            object,
            MiniMenuAction::OP_HELD2,
        ));
        pump_until(c, r, 10, |c| equipped(c, object));
    }
    save_stage(c, r, root, "ranged-equipped", stages);
    combat_shoot(c, r, root, stages, actions);
}

fn combat_shoot(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(equipped(c, 841) && equipped(c, 882));
    assert!(tutorial_text(c).contains("Rat ranging"));
    bow_style(c, r, root, stages);
    let xp = c.stat_xp[4];
    actions.push(attack(c));
    pump_until(c, r, 180, |c| {
        tutorial_text(c).contains("completed the tasks here")
    });
    assert!(c.stat_xp[4] > xp);
    save_stage(c, r, root, "ranged-rat-killed", stages);
    combat_exit(c, r, root, stages, actions);
}

// A protocol failure can disconnect after combat starts but before the server
// finishes the kill. Exercise the repaired projectile/ground-stack path using
// another naturally spawned rat, without resetting the tutorial or supplies.
fn combat_confirm(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("completed the tasks here"));
    assert!(equipped(c, 841) && equipped(c, 882));
    bow_style(c, r, root, stages);
    let slot = c
        .npc_ids
        .iter()
        .take(c.npc_count as usize)
        .copied()
        .filter(|slot| {
            c.npc[*slot as usize]
                .as_ref()
                .is_some_and(|n| n.r#type == Some(950))
        })
        .min_by_key(|slot| {
            let n = c.npc[*slot as usize].as_ref().unwrap();
            (n.route_x[0] + c.map_build_base_x - tile(c).0).abs()
                + (n.route_z[0] + c.map_build_base_z - tile(c).1).abs()
        })
        .expect("actual rat remains visible from the ranging area");
    let xp = c.stat_xp[4];
    actions.push(attack_rat(c, slot));
    pump_until(c, r, 30, |c| {
        let mut present = false;
        c.projectiles.for_each(|_| present = true);
        present
    });
    capture_stage(c, r, root, "ranged-projectile", stages);
    pump_until(c, r, 180, |c| {
        c.stat_xp[4] > xp
            && c.npc[slot as usize]
                .as_ref()
                .is_none_or(|n| n.health == 0 && n.combat_cycle > c.loop_cycle)
    });
    save_stage(c, r, root, "ranged-confirmation-kill", stages);
    combat_exit(c, r, root, stages, actions);
}

fn bow_style(c: &mut Client, r: &mut Renderer, root: &Path, stages: &mut Vec<Value>) {
    tab(c, r, 0);
    let combat = c.side_icon[0];
    let rapid = c
        .ifaces_merged()
        .find(|i| {
            i.layer_id == combat
                && i.button_type == 5
                && i.script_operand.as_deref() == Some(&[1][..])
        })
        .expect("actual shortbow Rapid select button")
        .id;
    // set_attackstyle recomputes the engine's transient weapon damage mode;
    // a resumed login does not otherwise restore that non-transmitted state.
    click_component(c, r, combat, rapid, (553, 205));
    save_stage(c, r, root, "ranged-style", stages);
}

fn combat_exit(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("completed the tasks here"));
    assert!(c.stat_xp[4] > 0);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 30, |c| {
        c.scene_state == 2 && tile(c).1 < 4000 && tutorial_text(c).contains("Banking.")
    });
    save_stage(c, r, root, "bank-entry", stages);
}

pub(super) fn close_main(c: &mut Client, r: &mut Renderer) {
    let main = c.main_modal_id;
    let close = c
        .ifaces_merged()
        .find(|i| i.layer_id == main && i.button_type == 3)
        .expect("open interface has its real close button")
        .id;
    click_component(c, r, main, close, (4, 4));
    pump_until(c, r, 10, |c| c.main_modal_id < 0 && c.side_modal_id < 0);
}

pub(super) fn walk_to(
    c: &mut Client,
    r: &mut Renderer,
    target: (i32, i32),
    actions: &mut Vec<Value>,
) {
    // Match production minimap_loop's transform; its real pathfinder handles
    // intervening cave walls. No route or collision state is assigned.
    use client::graphics::Pix3D;
    assert_eq!(c.minimap_state, 0);
    let before = tile(c);
    if before == target {
        return;
    }
    let fine = (
        (target.0 - c.map_build_base_x) * 128 + 64,
        (target.1 - c.map_build_base_z) * 128 + 64,
    );
    let p = c.local_player.as_ref().unwrap();
    let (px, pz) = (p.x, p.z);
    let yaw = ((c.orbit_camera_yaw + c.macro_minimap_angle) & 0x7ff) as usize;
    let sin = (Pix3D::sin_table()[yaw] * (c.macro_minimap_zoom + 256)) >> 8;
    let cos = (Pix3D::cos_table()[yaw] * (c.macro_minimap_zoom + 256)) >> 8;
    let (s, t) = (f64::from(sin), f64::from(cos));
    let denominator = s * s + t * t;
    let (dx, dz) = (f64::from(fine.0 - px), f64::from(fine.1 - pz));
    let mx = ((dx * t + dz * s) * 2048.0 / denominator).round() as i32;
    let my = ((dx * s - dz * t) * 2048.0 / denominator).round() as i32;
    let (x, y) = (-2..=2)
        .flat_map(|dy| (-2..=2).map(move |dx| (mx + dx, my + dy)))
        .find(|&(x, y)| {
            x * x + y * y < 70 * 70
                && ((px + ((y * sin + x * cos) >> 11)) >> 7) + c.map_build_base_x == target.0
                && ((pz - ((y * cos - x * sin) >> 11)) >> 7) + c.map_build_base_z == target.1
        })
        .expect("visible minimap pixel maps to the intended destination");
    let pixel = (648 + x, 83 + y);
    assert!(Client::minimap_hit(pixel.0, pixel.1).is_some());
    let receipt =
        json!({"input":"rendered minimap click","from":before,"target":target,"pixel":pixel});
    eprintln!("377 lesson walk {receipt}");
    actions.push(receipt);
    click(c, r, pixel.0, pixel.1, 1);
    pump_until(c, r, 30, |c| {
        tile(c) == target && c.local_player.as_ref().is_some_and(|p| (p.x, p.z) == fine)
    });
}

fn bank(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("Banking."));
    actions.push(hint_location(c, 0));
    pump_until(c, r, 30, |c| body(c).contains("access your bank account"));
    dialogue(c, r, root, "banker", stages);
    choose(c, r, "Yes.");
    pump_until(c, r, 10, |c| c.main_modal_id >= 0 && c.side_modal_id >= 0);
    save_stage(c, r, root, "bank-interface", stages);
    let bank = c
        .ifaces_merged()
        .find(|i| i.layer_id == c.main_modal_id && i.link_obj_type.is_some())
        .expect("actual main bank inventory")
        .id;
    let side = c
        .ifaces_merged()
        .find(|i| {
            i.layer_id == c.side_modal_id
                && i.link_obj_type
                    .as_ref()
                    .is_some_and(|types| types.contains(&2310))
        })
        .expect("actual bank side inventory contains natural bread")
        .id;
    assert_eq!(object_count(c, bank as usize, 995), 25);
    let before = object_count(c, bank as usize, 2309);
    actions.push(inventory_operation(
        c,
        side,
        2309,
        MiniMenuAction::INV_BUTTON1,
    ));
    pump_until(c, r, 10, |c| {
        object_count(c, side as usize, 2309) == 0
            && object_count(c, bank as usize, 2309) == before + 1
    });
    save_stage(c, r, root, "bread-deposited", stages);
    actions.push(inventory_operation(
        c,
        bank,
        2309,
        MiniMenuAction::INV_BUTTON1,
    ));
    pump_until(c, r, 10, |c| {
        object_count(c, side as usize, 2309) == 1 && object_count(c, bank as usize, 2309) == before
    });
    save_stage(c, r, root, "bread-withdrawn", stages);
    close_main(c, r);
    pump_until(c, r, 10, |c| inventory_count(c, 2309) == 1);
    save_stage(c, r, root, "bank-closed", stages);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 20, |c| tutorial_text(c).contains("Financial advice"));
    speak(c, r, root, "financial-advisor", stages);
    bank_exit(c, r, root, stages, actions);
}

fn bank_exit(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    pump_until(c, r, 10, |c| {
        c.chat_modal_id < 0 && tutorial_text(c).contains("Continue through the next door")
    });
    save_stage(c, r, root, "financial-advice-complete", stages);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 20, |c| {
        tutorial_text(c).contains("Follow the path to the chapel")
    });
    save_stage(c, r, root, "chapel-path", stages);
}

fn prayer(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("Follow the path to the chapel"));
    walk_to(c, r, (3130, 3108), actions);
    save_stage(c, r, root, "chapel-entry", stages);
    if let Some((code, _, _)) = scene_location(c, 1516) {
        actions.push(location_action(c, code, 0));
        pump_until(c, r, 10, |c| {
            scene_location(c, 1517).is_some() && scene_location(c, 1516).is_none()
        });
    }
    assert!(
        scene_location(c, 1517).is_some(),
        "actual chapel front door is open"
    );
    save_stage(c, r, root, "chapel-door-open", stages);
    speak(c, r, root, "brace-introduction", stages);
    pump_until(c, r, 10, |c| c.tut_flash_icon == 5);
    tab(c, r, 5);
    pump_until(c, r, 10, |c| {
        tutorial_text(c).contains("Talk with Brother Brace")
    });
    save_stage(c, r, root, "prayer-tab", stages);
    speak(c, r, root, "prayer-explanation", stages);
    pump_until(c, r, 10, |c| c.tut_flash_icon == 8);
    tab(c, r, 8);
    pump_until(c, r, 10, |c| c.tut_flash_icon == 9);
    save_stage(c, r, root, "friends-tab", stages);
    tab(c, r, 9);
    pump_until(c, r, 10, |c| {
        tutorial_text(c).contains("This is your ignore list")
    });
    save_stage(c, r, root, "ignore-tab", stages);
    speak(c, r, root, "friends-ignore-explanation", stages);
    pump_until(c, r, 10, |c| {
        tutorial_text(c).contains("Your final instructor")
    });
    save_stage(c, r, root, "brace-complete", stages);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 20, |c| tutorial_text(c).contains("wizards house"));
    save_stage(c, r, root, "magic-path", stages);
}

fn cast_wind_strike(c: &mut Client, r: &mut Renderer) -> Value {
    let spell = c
        .ifaces_merged()
        .find(|i| i.target_base.eq_ignore_ascii_case("Wind strike"))
        .expect("Java-decoded Wind Strike target component")
        .id;
    let magic = c.side_icon[6];
    click_component(c, r, magic, spell, (553, 205));
    assert_eq!(c.target_com_id, spell);
    assert_eq!(c.target_mode, 1);
    assert_eq!(c.hint_type, 1);
    let slot = c.hint_npc;
    assert_eq!(c.npc[slot as usize].as_ref().unwrap().r#type, Some(951));
    let receipt = json!({"input":"rendered Wind Strike component then production targeted NPC action",
        "spell_component":spell,"npc_slot":slot,"npc_type":951,"air":inventory_count(c,556),"mind":inventory_count(c,558)});
    c.menu_action[0] = MiniMenuAction::TGT_NPC;
    c.menu_param_a[0] = slot;
    c.menu_param_b[0] = 0;
    c.menu_param_c[0] = 0;
    c.doAction(0);
    receipt
}

fn magic(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("wizards house"));
    walk_to(c, r, (3134, 3095), actions);
    save_stage(c, r, root, "wizard-approach", stages);
    speak(c, r, root, "magic-introduction", stages);
    pump_until(c, r, 10, |c| c.tut_flash_icon == 6);
    tab(c, r, 6);
    pump_until(c, r, 10, |c| {
        c.chat_modal_id >= 0 && body(c).contains("list of your spells")
    });
    dialogue(c, r, root, "magic-runes", stages);
    pump_until(c, r, 10, |c| {
        inventory_count(c, 556) == 5 && inventory_count(c, 558) == 5
    });
    save_stage(c, r, root, "wind-strike-ready", stages);
    // The cage-facing window has a clear cast path; Terrova can finish the
    // dialogue on a tile where the stone house blocks the indicated chicken.
    walk_to(c, r, (3140, 3089), actions);
    save_stage(c, r, root, "chicken-approach", stages);
    actions.push(cast_wind_strike(c, r));
    pump_until(c, r, 30, |c| {
        tutorial_text(c).contains("Sadly it didn't have any effect")
            && inventory_count(c, 556) == 4
            && inventory_count(c, 558) == 4
    });
    magic_success(c, r, root, stages, actions);
}

fn magic_success(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("Sadly it didn't have any effect"));
    assert_eq!(inventory_count(c, 556), 4);
    assert_eq!(inventory_count(c, 558), 4);
    save_stage(c, r, root, "wind-strike-splash", stages);
    tab(c, r, 6);
    let xp = c.stat_xp[6];
    actions.push(cast_wind_strike(c, r));
    pump_until(c, r, 30, |c| {
        tutorial_text(c).contains("almost completed the tutorial")
            && c.stat_xp[6] > xp
            && inventory_count(c, 556) == 3
            && inventory_count(c, 558) == 3
    });
    assert!(c.stat_xp[6] > xp);
    assert_eq!(inventory_count(c, 556), 3);
    assert_eq!(inventory_count(c, 558), 3);
    save_stage(c, r, root, "wind-strike-success", stages);
    speak(c, r, root, "mainland-offer", stages);
    choose(c, r, "Yes.");
    pump_until(c, r, 45, |c| {
        c.scene_state == 2 && tile(c) == (3222, 3222) && c.main_overlay_id < 0
    });
    save_stage(c, r, root, "lumbridge-mainland", stages);
}

fn quest(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert!(tutorial_text(c).contains("first loaf of bread"));
    assert_eq!(inventory_count(c, 2309), 1);
    tab(c, r, 13);
    pump_until(c, r, 10, |c| tutorial_text(c).contains("The Music Player"));
    save_stage(c, r, root, "music-tab", stages);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 30, |c| c.tut_flash_icon == 12);
    save_stage(c, r, root, "chef-exit", stages);
    quest_run(c, r, root, stages, actions);
}

fn quest_run(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert_ne!(c.side_icon[12], -1);
    tab(c, r, 12);
    // The engine's tutorial_step_enable_run returns after p_run(off) before
    // sending its instruction. The real controls still expose the run button.
    save_stage(c, r, root, "controls-tab", stages);
    let controls = c.side_icon[12];
    let run = c.if_(controls as usize).unwrap().children.as_ref().unwrap()[5];
    assert_eq!(
        c.if_(run as usize).unwrap().script_operand.as_deref(),
        Some(&[1][..])
    );
    click_component(c, r, controls, run, (553, 205));
    pump_until(c, r, 10, |c| {
        tutorial_text(c).contains("Run to the next guide")
    });
    save_stage(c, r, root, "running", stages);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 40, |c| {
        tutorial_text(c).contains("Talk with the Quest Guide")
    });
    save_stage(c, r, root, "quest-door", stages);
    speak(c, r, root, "quest-introduction", stages);
    pump_until(c, r, 10, |c| c.tut_flash_icon == 2);
    tab(c, r, 2);
    pump_until(c, r, 10, |c| {
        tutorial_text(c).contains("Your Quest journal")
    });
    save_stage(c, r, root, "quest-tab", stages);
    speak(c, r, root, "quest-explanation", stages);
    pump_until(c, r, 10, |c| {
        tutorial_text(c).contains("Click on the ladder")
    });
    save_stage(c, r, root, "quest-complete", stages);
    actions.push(hint_location(c, 0));
    pump_until(c, r, 30, |c| {
        c.scene_state == 2 && tile(c).1 > 9000 && tutorial_text(c).contains("Mining and smithing")
    });
    save_stage(c, r, root, "mine-entry", stages);
}

fn chef_bread(
    c: &mut Client,
    r: &mut Renderer,
    root: &Path,
    stages: &mut Vec<Value>,
    actions: &mut Vec<Value>,
) {
    assert_eq!(
        c.var.get(406),
        Some(&5),
        "server tutorial state is making dough"
    );
    assert_eq!(
        c.var_serv.get(406),
        Some(&5),
        "the engine acknowledged the chef's natural tutorial progress"
    );
    assert!(
        tutorial_text(c).contains("Making dough"),
        "the server tutorial instruction matches the chef recipe"
    );
    assert_eq!(
        inventory_count(c, 2516),
        1,
        "the chef supplied one pot of flour"
    );
    assert_eq!(
        inventory_count(c, 1929),
        1,
        "the chef supplied one bucket of water"
    );
    save_stage(c, r, root, "chef-resume-supplies", stages);

    // A disconnect may restore the chef's final item-box dialogue. Dismiss
    // only server-rendered continue buttons, never a synthetic interface.
    for page in 0..8 {
        if c.chat_modal_id < 0 {
            break;
        }
        let current = body(c);
        let has_continue = c
            .ifaces_merged()
            .any(|i| i.layer_id == c.chat_modal_id && i.button_type == 6);
        assert!(
            has_continue,
            "resumed chef dialogue has a real continue button: {current}"
        );
        save_stage(c, r, root, &format!("chef-resume-dialogue-{page}"), stages);
        continue_chat(c, r);
        pump_until(c, r, 10, |c| {
            c.chat_modal_id < 0 || !c.resumed_pause_button && body(c) != current
        });
    }
    assert!(
        c.chat_modal_id < 0,
        "resumed chef dialogue ended before mixing: {}",
        state(c)
    );
    assert_eq!(c.var.get(406), Some(&5));
    assert_eq!(inventory_count(c, 2516), 1);
    assert_eq!(inventory_count(c, 1929), 1);

    actions.push(use_items(c, 1929, 2516));
    pump_until(c, r, 20, |c| {
        inventory_count(c, 2516) == 0
            && inventory_count(c, 1929) == 0
            && inventory_count(c, 2307) == 1
            && tutorial_text(c).contains("Cooking dough")
            && c.var.get(406) == Some(&5)
            && c.var_serv.get(406) == Some(&5)
    });
    save_stage(c, r, root, "chef-resume-dough", stages);

    actions.push(use_item_on_location(c, 2307, 3039));
    pump_until(c, r, 30, |c| {
        inventory_count(c, 2307) == 0
            && inventory_count(c, 2309) == 1
            && tutorial_text(c).contains("your first loaf of bread")
            && c.var.get(406) == Some(&6)
            && c.var_serv.get(406) == Some(&6)
    });
    save_stage(c, r, root, "chef-resume-bread", stages);
}

#[test]
#[ignore = "requires orchestrator-started Fairy Ring 377 engine and naturally progressed t2a account"]
fn natural_tutorial_lesson() {
    let root =
        PathBuf::from(std::env::var("CLIENT_377_LIVE_DIR").expect("explicit evidence directory"));
    let cache = std::env::var("CLIENT_377_RUNTIME_CACHE_DIR").expect("matched running cache");
    let user = std::env::var("CLIENT_377_LIVE_USER").expect("explicit t2a account");
    let lesson = std::env::var("CLIENT_377_TUTORIAL_LESSON").expect("explicit natural lesson");
    assert!(user.starts_with("t2a"));
    if let Ok(lock) = std::env::var("CLIENT_377_QUIET_LOCK") {
        assert!(!Path::new(&lock).exists());
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
        r.draw_progress(c, message, percent)
    }));
    assert!(!c.error_loading);
    let outbound = Arc::new(Outbound::default());
    c.out.set_observer(Some(outbound.clone()));
    c.set_login_deadline(Instant::now() + Duration::from_secs(10));
    c.login(&user, "t2apass", false).unwrap();
    pump_until(&mut c, &mut r, 90, |c| {
        c.scene_state == 2 && c.npc_count > 0
    });
    let mut stages = Vec::new();
    let mut actions = Vec::new();
    save_stage(&mut c, &mut r, &root, "lesson-start", &mut stages);
    match lesson.as_str() {
        "chef-bread" => chef_bread(&mut c, &mut r, &root, &mut stages, &mut actions),
        "quest" => quest(&mut c, &mut r, &root, &mut stages, &mut actions),
        "quest-run" => quest_run(&mut c, &mut r, &root, &mut stages, &mut actions),
        "mining" => mining(&mut c, &mut r, &root, &mut stages, &mut actions),
        "combat" => combat(&mut c, &mut r, &root, &mut stages, &mut actions),
        "combat-ranged" => combat_ranged(&mut c, &mut r, &root, &mut stages, &mut actions),
        "combat-shoot" => combat_shoot(&mut c, &mut r, &root, &mut stages, &mut actions),
        "combat-confirm" => combat_confirm(&mut c, &mut r, &root, &mut stages, &mut actions),
        "bank-exit" => bank_exit(&mut c, &mut r, &root, &mut stages, &mut actions),
        "bank" => bank(&mut c, &mut r, &root, &mut stages, &mut actions),
        "prayer" => prayer(&mut c, &mut r, &root, &mut stages, &mut actions),
        "magic" => magic(&mut c, &mut r, &root, &mut stages, &mut actions),
        "magic-success" => magic_success(&mut c, &mut r, &root, &mut stages, &mut actions),
        _ => panic!("unknown tutorial lesson {lesson}"),
    }
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
    let exit = c.take_session_exit_reason().unwrap().to_string();
    assert!(exit.contains("server_logout"));
    let receipt = json!({"engine_dir":engine,"user":user,"revision":377,"lesson":lesson,
        "actions":actions,"stages":stages,"final_state":final_state,"outbound":*outbound.0.lock().unwrap(),"logout":exit});
    std::fs::write(
        root.join("lesson.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("PASS: natural tutorial lesson {lesson}; server logout");
}
