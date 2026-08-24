// Minimenu chrome: `open_menu` clamps the menu into the panel holding the
// click (0 viewport, 1 side, 2 chat) and sizes it to the widest option.
// World picks: `add_world_options` fills the menu from `pix3d` picks.
// The /tmp cache has no packs, so `Client::new` falls back to
// `Cache::default()` and never touches the network (the /crc fetch on
// 127.0.0.1 is refused instantly).
use client::client::{Client, ClientConfig, MiniMenuAction};
use client::render::Renderer;
use client::config::if_type::{ButtonType, ComponentType, IfType};
use client::config::{LocType, NpcType, ObjType};
use client::dash3d::{ClientNpc, ClientObj, ClientPlayer};
use client::datastruct::LinkList;

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[test]
fn open_menu_in_viewport_sets_area_0_and_geometry() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 3;
    c.menu_option[0] = "Cancel".into();
    c.menu_option[1] = "Walk here".into();
    c.menu_option[2] = "Examine @cya@Tree".into();
    c.shell.mouse_click_x = 100;
    c.shell.mouse_click_y = 100;
    c.open_menu();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 0);
    assert!(c.menu_width >= 8);
    assert_eq!(c.menu_height, 3 * 15 + 22);
}

#[test]
fn open_menu_in_side_sets_area_1() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 2;
    c.menu_option[0] = "Cancel".into();
    c.menu_option[1] = "Wear".into();
    c.shell.mouse_click_x = 600;
    c.shell.mouse_click_y = 300;
    c.open_menu();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 1);
}

#[test]
fn open_menu_in_chat_sets_area_2() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 2;
    c.menu_option[0] = "Cancel".into();
    c.menu_option[1] = "Report abuse".into();
    c.shell.mouse_click_x = 100;
    c.shell.mouse_click_y = 400;
    c.open_menu();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 2);
}

#[test]
fn open_menu_clamps_viewport_menu_inside_512x334() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 3;
    c.menu_option[0] = "Cancel".into();
    c.menu_option[1] = "Walk here".into();
    c.menu_option[2] = "Examine @cya@Tree".into();
    c.shell.mouse_click_x = 514;
    c.shell.mouse_click_y = 330;
    c.open_menu();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 0);
    assert_eq!(c.menu_x + c.menu_width, 512);
    // The y-clamp fits the 15*3+21 height (TS 8473-8478); the stored
    // `menu_height` is 15*3+22 (TS 8481), one taller, kept verbatim.
    assert_eq!(c.menu_y, 334 - (3 * 15 + 21));
    assert_eq!(c.menu_height, 3 * 15 + 22);
}

/// `add_world_options` fills the menu from `pix3d` picks. With no pick and
/// no use/target armed, the only entry added on top of Cancel is Walk here.
#[test]
fn add_world_options_walk_when_idle() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 1; // Cancel already
    c.shell.mouse_x = 50;
    c.shell.mouse_y = 80;
    c.add_world_options();
    assert!(c.menu_num_entries >= 2);
    assert_eq!(c.menu_action[1], MiniMenuAction::WALK);
    assert_eq!(c.menu_param_b[1], 50);
    assert_eq!(c.menu_param_c[1], 80);
}

/// A picked loc (entity 2) shows its ops and Examine once `world.type_code2`
/// resolves the tile. `add_scenery` plants a sprite with the same typecode
/// so the real `World::type_code2` answers >= 0 (no loc fixture needed).
#[test]
fn add_world_options_loc_examine_from_pick() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 1;
    // typecode: entity 2, typeId 1, x=10, z=12
    let type_id = 1i32;
    let x = 10i32;
    let z = 12i32;
    let typecode = (2 << 29) | ((type_id & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.pick_count = 1;
    c.pick_typecodes[0] = typecode;
    if c.cache.locs.len() <= 1 {
        c.cache.locs.push(LocType::default());
        c.cache.locs.push(LocType::default());
    }
    c.cache.locs[1].name = "Tree".into();
    c.cache.locs[1].op = vec![Some("Chop".into()), None, None, None, None];
    c.world.add_scenery(
        0,
        x,
        z,
        0,
        typecode,
        0,
        1,
        1,
        0,
        0,
        0,
        0,
        0,
    );
    assert!(c.world.type_code2(0, x, z, typecode) >= 0);
    c.add_world_options();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::WALK));
    let chop = actions.iter().position(|&a| a == MiniMenuAction::OP_LOC1).expect("op[0]");
    assert_eq!(c.menu_option[chop], "Chop @cya@Tree");
    assert!(actions.contains(&MiniMenuAction::OP_LOC6), "Examine");
    let examine = actions.iter().position(|&a| a == MiniMenuAction::OP_LOC6).unwrap();
    assert_eq!(c.menu_param_a[examine], typecode);
}

/// A picked npc (entity 1) lists its non-attack ops, then Attack with a
/// priority suffix when it outlevels the local player, then Examine. A
/// visible level appends the combat-colour level to the name.
#[test]
fn add_world_options_npc_ops_from_pick() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 1;
    // typecode: entity 1, npc slot 5, x=8, z=9
    let npc_slot = 5i32;
    let x = 8i32;
    let z = 9i32;
    let typecode = (1 << 29) | ((npc_slot & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.pick_count = 1;
    c.pick_typecodes[0] = typecode;
    if c.cache.npcs.len() <= 2 {
        c.cache.npcs.push(NpcType::default());
        c.cache.npcs.push(NpcType::default());
        c.cache.npcs.push(NpcType::default());
    }
    c.cache.npcs[2].name = "Goblin".into();
    c.cache.npcs[2].size = 1;
    c.cache.npcs[2].vislevel = 10;
    c.cache.npcs[2].op = vec![Some("Attack".into()), Some("Talk".into())];
    let mut local = ClientPlayer::default();
    local.combat_level = 3;
    c.local_player = Some(local);
    let mut npc = ClientNpc::default();
    npc.r#type = Some(2);
    npc.x = x * 128 + 64;
    npc.z = z * 128 + 64;
    c.npc[npc_slot as usize] = Some(npc);
    c.add_world_options();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::OP_NPC2), "Talk -> OP_NPC2");
    // vislevel 10 > local 3 makes Attack priority-pinned
    let attack = actions
        .iter()
        .position(|&a| a == MiniMenuAction::_PRIORITY + MiniMenuAction::OP_NPC1)
        .expect("prioritised Attack");
    assert_eq!(
        c.menu_option[attack],
        "Attack @yel@Goblin@or3@ (level-10)" // local 3 - 10 = -7 -> @or3@
    );
    assert!(actions.contains(&MiniMenuAction::OP_NPC6), "Examine");
}

/// A picked player (entity 0) shows the `player_op` options with `_PRIORITY`
/// on priority ops, and renames the Walk here entry with the player name.
#[test]
fn add_world_options_player_ops_from_pick() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 1;
    c.shell.mouse_x = 50;
    c.shell.mouse_y = 80;
    // typecode: entity 0, player slot 3, x=6, z=7
    let player_slot = 3i32;
    let x = 6i32;
    let z = 7i32;
    let typecode = (0 << 29) | ((player_slot & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.pick_count = 1;
    c.pick_typecodes[0] = typecode;
    let mut local = ClientPlayer::default();
    local.combat_level = 10;
    c.local_player = Some(local);
    let mut player = ClientPlayer::default();
    player.name = Some("Bob".into());
    player.combat_level = 12; // outlevels the local player: Attack is pinned
    player.skill_level = 0;
    player.x = x * 128 + 64;
    player.z = z * 128 + 64;
    c.players[player_slot as usize] = Some(player);
    c.player_op[1] = Some("Trade with".into());
    c.player_op[2] = Some("Attack".into());
    // playerOpPriority only pins non-attack ops (TS 9640-9644)
    c.player_op_priority[1] = true;
    c.add_world_options();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(
        actions.contains(&(MiniMenuAction::_PRIORITY + MiniMenuAction::OP_PLAYER2)),
        "prioritised Trade with"
    );
    assert!(
        actions.contains(&(MiniMenuAction::_PRIORITY + MiniMenuAction::OP_PLAYER3)),
        "prioritised Attack"
    );
    // Walk here renamed with the player name: local 10 - 12 = -2 -> @or1@
    let walk = actions.iter().position(|&a| a == MiniMenuAction::WALK).unwrap();
    assert_eq!(c.menu_option[walk], "Walk here @whi@Bob@or1@ (level-12)");
}

/// A picked obj tile (entity 3) iterates the ground list tail->prev, with
/// Take as the default op[2] fallback and Examine last per object.
#[test]
fn add_world_options_obj_take_and_examine_from_pick() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 1;
    // typecode: entity 3, typeId 9 (idle), x=4, z=5
    let x = 4i32;
    let z = 5i32;
    let typecode = (3 << 29) | ((9 & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.pick_count = 1;
    c.pick_typecodes[0] = typecode;
    if c.cache.objs.len() <= 7 {
        while c.cache.objs.len() < 8 {
            c.cache.objs.push(ObjType::default());
        }
    }
    c.cache.objs[7].name = "Coins".into();
    c.cache.objs[7].op[2] = Some("Take".into());
    c.cache.objs[6].name = "Rune".into();
    c.cache.objs[6].op[0] = Some("Loot".into());
    let mut objs = LinkList::new();
    objs.push(ClientObj::new(7, 1)); // tail
    objs.push(ClientObj::new(6, 1)); // head
    c.ground_obj[0][x as usize][z as usize] = Some(objs);
    c.add_world_options();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::OP_OBJ1), "Loot");
    assert!(actions.contains(&MiniMenuAction::OP_OBJ3), "Take fallback");
    assert!(actions.contains(&MiniMenuAction::OP_OBJ6), "Examine");
    // tail->prev (newest first): Rune pushed last, so it iterates first
    let rune = actions.iter().position(|&a| a == MiniMenuAction::OP_OBJ6).unwrap();
    assert_eq!(c.menu_option[rune], "Examine @lre@Rune");
    let coins = actions.iter().rposition(|&a| a == MiniMenuAction::OP_OBJ6).unwrap();
    assert_eq!(c.menu_option[coins], "Examine @lre@Coins");
}

/// Duplicate picked typecodes add their options only once.
#[test]
fn add_world_options_skips_duplicate_typecodes() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 1;
    let typecode = (3 << 29) | ((9 & 0x7fff) << 14) | ((5 & 0x7f) << 7) | (4 & 0x7f);
    c.pick_count = 2;
    c.pick_typecodes[0] = typecode;
    c.pick_typecodes[1] = typecode;
    if c.cache.objs.len() <= 7 {
        while c.cache.objs.len() < 8 {
            c.cache.objs.push(ObjType::default());
        }
    }
    c.cache.objs[7].name = "Coins".into();
    let mut objs = LinkList::new();
    objs.push(ClientObj::new(7, 1));
    c.ground_obj[0][4][5] = Some(objs);
    c.add_world_options();
    let examines: i32 = (0..c.menu_num_entries)
        .filter(|&i| c.menu_action[i as usize] == MiniMenuAction::OP_OBJ6)
        .count() as i32;
    assert_eq!(examines, 1);
}

/// `add_npc_options`/`add_player_options` are `pub` so the minimenu walk can
/// call them directly (they are also reachable through `add_world_options`).
#[test]
fn add_npc_options_and_add_player_options_callable_directly() {
let _r = Renderer::new(false);
    let mut c = client();
    c.menu_num_entries = 1;
    if c.cache.npcs.len() <= 2 {
        c.cache.npcs.push(NpcType::default());
        c.cache.npcs.push(NpcType::default());
        c.cache.npcs.push(NpcType::default());
    }
    c.cache.npcs[2].name = "Rat".into();
    c.cache.npcs[2].op = vec![Some("Attack".into())];
    c.add_npc_options(2, 7, 3, 4);
    let mut p = ClientPlayer::default();
    p.name = Some("Bob".into());
    p.skill_level = 1; // no combat suffix: (skill-1) tooltip
    c.players[9] = Some(p);
    c.player_op[1] = Some("Attack".into());
    c.add_player_options(9, 2, 3);
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::OP_NPC6), "npc Examine");
    assert!(actions.contains(&MiniMenuAction::OP_NPC1), "npc Attack");
    assert!(actions.contains(&MiniMenuAction::OP_PLAYER2), "player op[1]");
}

// ---- `build_minimenu`: Cancel seed, inv/held/button options, sort (Task 3) ----

/// A side-panel TYPE_INV tree: the side icon at `active_icon` is walked
/// when no side modal is open (`side_modal_id == -1`), the same path the
/// hover walk uses.
fn side_inv_fixture(c: &mut Client) {
    c.side_modal_id = -1;
    c.side_icon[3] = 1;
    c.active_icon = 3;
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 190;
    layer.height = 261;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut inv = IfType::default();
    inv.id = 2;
    inv.r#type = ComponentType::TYPE_INV;
    inv.obj_ops = true;
    inv.width = 1;
    inv.height = 1;
    inv.link_obj_type = Some(vec![2]); // obj id 1
    inv.link_obj_number = Some(vec![1]);
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(inv);
    if c.cache.objs.len() < 2 {
        c.cache.objs.resize(2, ObjType::default());
        c.cache.objs[1].name = "Rune".into();
    }
    c.shell.mouse_x = 553 + 16;
    c.shell.mouse_y = 205 + 16;
}

#[test]
fn build_minimenu_starts_with_cancel() {
let _r = Renderer::new(false);
    let mut c = client();
    c.build_minimenu();
    assert_eq!(c.menu_num_entries, 1);
    assert_eq!(c.menu_action[0], MiniMenuAction::CANCEL);
}

#[test]
fn inv_slot_adds_drop_and_examine() {
let _r = Renderer::new(false);
    let mut c = client();
    side_inv_fixture(&mut c);
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::OP_HELD5), "Drop");
    assert!(actions.contains(&MiniMenuAction::OP_HELD6), "Examine");
}

/// The bubble sort (TS 2569-2598) moves adjacent `<1000`/`>1000` pairs so
/// the `>1000` entry sinks: Cancel (1106) pins index 0, Examine (1328)
/// lands below Drop (100).
#[test]
fn build_minimenu_sorts_1000_plus_below_actions() {
let _r = Renderer::new(false);
    let mut c = client();
    side_inv_fixture(&mut c);
    c.build_minimenu();
    assert_eq!(c.menu_action[1], MiniMenuAction::OP_HELD6, "Examine sinks below the actions");
    assert_eq!(c.menu_action[2], MiniMenuAction::OP_HELD5, "Drop stays last entry");
}

/// `obj.iop` and the component's `iop` fill the held ops: obj iop[3] is
/// OP_HELD4, iop[0] is OP_HELD1, and the component iop[0] is INV_BUTTON1
/// (TS 9737-9781). All carry the obj id, slot and component id params.
#[test]
fn inv_slot_obj_iop_and_component_iop_options() {
let _r = Renderer::new(false);
    let mut c = client();
    side_inv_fixture(&mut c);
    let inv = c.cache.ifaces[2].as_mut().unwrap();
    inv.iop[0] = Some("Bank".into());
    if c.cache.objs.len() < 2 {
        c.cache.objs.resize(2, ObjType::default());
    }
    c.cache.objs[1].name = "Rune".into();
    c.cache.objs[1].iop[0] = Some("Wield".into());
    c.cache.objs[1].iop[3] = Some("Unnote".into());
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::OP_HELD1), "obj iop[0]");
    assert!(actions.contains(&MiniMenuAction::OP_HELD4), "obj iop[3]");
    assert!(actions.contains(&MiniMenuAction::INV_BUTTON1), "component iop[0]");
    let held1 = actions.iter().position(|&a| a == MiniMenuAction::OP_HELD1).unwrap();
    assert_eq!(c.menu_option[held1], "Wield @lre@Rune");
    assert_eq!(c.menu_param_a[held1], 1, "obj id");
    assert_eq!(c.menu_param_b[held1], 0, "slot");
    assert_eq!(c.menu_param_c[held1], 2, "component id");
    let inv1 = actions.iter().position(|&a| a == MiniMenuAction::INV_BUTTON1).unwrap();
    assert_eq!(c.menu_option[inv1], "Bank @lre@Rune");
}

/// `use_mode`/`target_mode` replace the held ops: Use → USEHELD_ONHELD
/// (skipping the selected slot itself), target with the `0x10` mask bit →
/// TGT_HELD (TS 9684-9701).
#[test]
fn inv_slot_use_and_target_replace_ops() {
let _r = Renderer::new(false);
    let mut c = client();
    side_inv_fixture(&mut c);
    c.use_mode = 1;
    c.obj_selected_name = "Knife".into();
    c.obj_selected_com_id = 2;
    c.obj_selected_slot = 1; // not the hovered slot 0
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::USEHELD_ONHELD), "Use with");
    assert!(!actions.contains(&MiniMenuAction::OP_HELD5), "no Drop in use mode");
    assert!(!actions.contains(&MiniMenuAction::OP_HELD6), "no Examine in use mode");

    c.use_mode = 0;
    c.target_mode = 1;
    c.target_op = "Cast".into();
    c.target_mask = 0x10;
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::TGT_HELD), "target with 0x10 mask");
    assert!(!actions.contains(&MiniMenuAction::OP_HELD6));

    c.target_mask = 0x1; // no 0x10 bit: nothing is pushed for the slot
    c.build_minimenu();
    assert_eq!(c.menu_num_entries, 1, "no target option without the 0x10 mask bit");
}

/// Overlapping non-inv children: each visible button under the pointer
/// pushes its option — OK→IF_BUTTON (button_text), CLOSE→'Close',
/// TOGGLE/SELECT→button_text, CONTINUE→PAUSE_BUTTON, TARGET→TGT_BUTTON
/// (prefix verb + base) — with the component id in `menu_param_c`
/// (TS 9795-9839).
#[test]
fn non_inv_buttons_push_button_actions() {
let _r = Renderer::new(false);
    let mut c = client();
    c.side_modal_id = -1;
    c.side_icon[3] = 1;
    c.active_icon = 3;
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 190;
    layer.height = 261;
    layer.children = Some(vec![2, 3, 4, 5, 6, 7]);
    layer.child_x = Some(vec![0, 0, 0, 0, 0, 0]);
    layer.child_y = Some(vec![0, 0, 0, 0, 0, 0]);
    c.cache.ifaces.resize(8, None);
    c.cache.ifaces[1] = Some(layer);
    let button = |id: i32, button_type: i32| IfType {
        id,
        r#type: ComponentType::TYPE_RECT,
        width: 50,
        height: 15,
        button_type,
        ..IfType::default()
    };
    let mut ok = button(2, ButtonType::BUTTON_OK);
    ok.button_text = "Ok".into();
    c.cache.ifaces[2] = Some(ok);
    c.cache.ifaces[3] = Some(button(3, ButtonType::BUTTON_CLOSE));
    let mut toggle = button(4, ButtonType::BUTTON_TOGGLE);
    toggle.button_text = "Trade".into();
    c.cache.ifaces[4] = Some(toggle);
    let mut select = button(5, ButtonType::BUTTON_SELECT);
    select.button_text = "Deposit".into();
    c.cache.ifaces[5] = Some(select);
    let mut cont = button(6, ButtonType::BUTTON_CONTINUE);
    cont.button_text = "Continue".into();
    c.cache.ifaces[6] = Some(cont);
    let mut target = button(7, ButtonType::BUTTON_TARGET);
    target.target_verb = "Cast on".into();
    target.target_base = "Tree".into();
    c.cache.ifaces[7] = Some(target);
    c.shell.mouse_x = 553 + 10;
    c.shell.mouse_y = 205 + 10;
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::IF_BUTTON));
    assert!(actions.contains(&MiniMenuAction::CLOSE_BUTTON));
    assert!(actions.contains(&MiniMenuAction::TOGGLE_BUTTON));
    assert!(actions.contains(&MiniMenuAction::SELECT_BUTTON));
    assert!(actions.contains(&MiniMenuAction::PAUSE_BUTTON));
    assert!(actions.contains(&MiniMenuAction::TGT_BUTTON));
    let close = actions.iter().position(|&a| a == MiniMenuAction::CLOSE_BUTTON).unwrap();
    assert_eq!(c.menu_option[close], "Close");
    assert_eq!(c.menu_param_c[close], 3);
    let target = actions.iter().position(|&a| a == MiniMenuAction::TGT_BUTTON).unwrap();
    assert_eq!(c.menu_option[target], "Cast @gre@Tree", "prefix is the first word of targetVerb");
}

/// Java `addComponentOptions` buttonType 1 has no empty-text gate
/// (`Client.java` 5962-5966). Emote tiles on the player-controls panel
/// (`controls:com_13` etc.) are BUTTON_OK graphics with empty option
/// text; skipping them was a live no-op.
#[test]
fn empty_ok_button_still_fires_if_button() {
let _r = Renderer::new(false);
    let mut c = client();
    c.side_modal_id = -1;
    c.side_icon[13] = 1;
    c.active_icon = 13;
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 190;
    layer.height = 261;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(IfType {
        id: 2,
        r#type: ComponentType::TYPE_GRAPHIC,
        width: 36,
        height: 25,
        button_type: ButtonType::BUTTON_OK,
        button_text: String::new(),
        ..IfType::default()
    });
    c.shell.mouse_x = 553 + 10;
    c.shell.mouse_y = 205 + 10;
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(
        actions.contains(&MiniMenuAction::IF_BUTTON),
        "emote tiles with empty button_text must still send IF_BUTTON"
    );
}

/// `resumed_pause_button` suppresses the CONTINUE option and `target_mode`
/// suppresses the TARGET option (TS 9831-9839).
#[test]
fn paused_and_targeting_suppress_continue_and_target_buttons() {
let _r = Renderer::new(false);
    let mut c = client();
    c.side_modal_id = -1;
    c.side_icon[3] = 1;
    c.active_icon = 3;
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 190;
    layer.height = 261;
    layer.children = Some(vec![2, 3]);
    layer.child_x = Some(vec![0, 0]);
    layer.child_y = Some(vec![0, 0]);
    let mut cont = IfType::default();
    cont.id = 2;
    cont.r#type = ComponentType::TYPE_RECT;
    cont.width = 50;
    cont.height = 15;
    cont.button_type = ButtonType::BUTTON_CONTINUE;
    cont.button_text = "Continue".into();
    let mut target = IfType::default();
    target.id = 3;
    target.r#type = ComponentType::TYPE_RECT;
    target.width = 50;
    target.height = 15;
    target.button_type = ButtonType::BUTTON_TARGET;
    target.target_verb = "Cast".into();
    target.target_base = "Tree".into();
    c.cache.ifaces.resize(4, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(cont);
    c.cache.ifaces[3] = Some(target);
    c.shell.mouse_x = 553 + 10;
    c.shell.mouse_y = 205 + 10;
    c.resumed_pause_button = true;
    c.target_mode = 1;
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(!actions.contains(&MiniMenuAction::PAUSE_BUTTON), "paused latches the Continue");
    assert!(!actions.contains(&MiniMenuAction::TGT_BUTTON), "targeting replaces TGT_BUTTON");
}

/// An in-flight inventory drag returns before seeding Cancel (TS 2515).
#[test]
fn build_minimenu_returns_while_obj_drag_active() {
let _r = Renderer::new(false);
    let mut c = client();
    c.obj_drag_area = 2;
    c.menu_num_entries = 0;
    c.build_minimenu();
    assert_eq!(c.menu_num_entries, 0);
}

/// The no-chat-modal branch (TS 2556-2560): a staff player hovering a
/// public line gets "Report abuse" (ABUSE_REPORT); a non-staff player gets
/// nothing (friends/ignore options are slice 5).
#[test]
fn chat_region_adds_report_abuse_for_staff() {
let _r = Renderer::new(false);
    let mut c = client();
    let mut local = ClientPlayer::default();
    local.name = Some("Me".into());
    c.local_player = Some(local);
    c.staffmodlevel = 1;
    c.add_chat(1, "hello", "Bob");
    c.shell.mouse_x = 200;
    c.shell.mouse_y = 420; // line 0's band: relative 63 in (60, 74]
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::ABUSE_REPORT), "staff report-abuse");
    let abuse = actions.iter().position(|&a| a == MiniMenuAction::ABUSE_REPORT).unwrap();
    assert_eq!(c.menu_option[abuse], "Report abuse @whi@Bob");

    c.staffmodlevel = 0;
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(!actions.contains(&MiniMenuAction::ABUSE_REPORT), "non-staff has no report abuse");
}

// ---- `mouse_loop`: right-click opens, left-click fires the last entry (Task 4) ----

/// A right click in the viewport opens the minimenu in the viewport
/// (`menu_area` 0) via `open_menu` (TS 8379-8380).
#[test]
fn right_click_viewport_opens_menu() {
let _r = Renderer::new(false);
    let mut c = client();
    c.shell.mouse_x = 100;
    c.shell.mouse_y = 100;
    c.build_minimenu();
    c.shell.apply_mouse_down(2, 100, 100);
    c.shell.latch_click();
    c.mouse_loop();
    assert!(c.is_menu_open);
    assert_eq!(c.menu_area, 0);
}

/// A left click fires the last menu entry (TS 8375-8376). With no picks
/// and no use/target armed, the last entry is Walk here: WALK arms picking
/// and does not write `out`.
#[test]
fn left_click_fires_last_entry_walk() {
let _r = Renderer::new(false);
    let mut c = client();
    c.shell.mouse_x = 100;
    c.shell.mouse_y = 100;
    c.build_minimenu();
    let last = c.menu_num_entries - 1;
    assert_eq!(c.menu_action[last as usize], MiniMenuAction::WALK);
    c.shell.apply_mouse_down(1, 100, 100);
    c.shell.latch_click();
    c.mouse_loop();
    // WALK arms picking; does not write out
    assert!(!c.is_menu_open);
}

/// With the menu open, a left click on an option row fires it and closes
/// the menu (TS 8266-8291). The two-entry viewport menu renders bottom-up:
/// row 1 (Walk here) sits at `menu_y + 31`, so a click there arms picking.
#[test]
fn left_click_on_open_menu_row_fires_option_and_closes() {
let _r = Renderer::new(false);
    let mut c = client();
    c.shell.mouse_x = 100;
    c.shell.mouse_y = 100;
    c.build_minimenu();
    c.shell.apply_mouse_down(2, 100, 100);
    c.shell.latch_click();
    c.mouse_loop();
    assert!(c.is_menu_open);
    // row 1 band: option_y = menu_y + 31, click within option_y - 13 .. + 3
    // (menu_y + 23 lands at shifted click_y menu_y + 19, inside the band)
    c.shell.apply_mouse_down(1, 100, c.menu_y + 23);
    c.shell.latch_click();
    c.mouse_loop();
    assert!(!c.is_menu_open);
    assert!(c.world.click, "the Walk row fires doAction(WALK)");
}

// ---- Task 5: friends/ignore/PM menu options ----

/// A public line (TS gate: type 1 always counts, type 2 only for friends
/// in chatPublicMode 1) offers Add ignore / Add friend after the staff
/// Report abuse (TS 2687-2698).
#[test]
fn chat_line_adds_friend_and_ignore_for_friend() {
let _r = Renderer::new(false);
    let mut c = client();
    let mut local = ClientPlayer::default();
    local.name = Some("Me".into());
    c.local_player = Some(local);
    c.chat_public_mode = 1;
    c.friend_count = 1;
    c.friend_username[0] = "Bob".into();
    c.add_chat(1, "hello", "Bob");
    c.shell.mouse_x = 200;
    c.shell.mouse_y = 420; // line 0's band: relative 63 in (60, 74]
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(actions.contains(&MiniMenuAction::FRIENDLIST_ADD), "Add friend");
    assert!(actions.contains(&MiniMenuAction::IGNORELIST_ADD), "Add ignore");
    let add = actions
        .iter()
        .position(|&a| a == MiniMenuAction::FRIENDLIST_ADD)
        .unwrap();
    assert_eq!(c.menu_option[add], "Add friend @whi@Bob");

    // a non-friend's type-2 line in chatPublicMode 1 counts/hovers nothing
    let _r = Renderer::new(false);
    let mut c2 = client();
    let mut local = ClientPlayer::default();
    local.name = Some("Me".into());
    c2.local_player = Some(local);
    c2.chat_public_mode = 1;
    c2.friend_count = 1;
    c2.friend_username[0] = "Bob".into();
    c2.add_chat(2, "hello", "Eve");
    c2.shell.mouse_x = 200;
    c2.shell.mouse_y = 420;
    c2.build_minimenu();
    let actions: Vec<i32> =
        (0..c2.menu_num_entries).map(|i| c2.menu_action[i as usize]).collect();
    assert!(!actions.contains(&MiniMenuAction::FRIENDLIST_ADD), "non-friend has no Add friend");
}

/// A BUTTON_OK with a friend-list client code (1..=200 or 701..=900)
/// pushes Remove/Message via `addSocialOptions` instead of the button text
/// (TS 9799-9807); the ignore range pushes Remove only (TS 9866-9872).
#[test]
fn social_component_ok_override_pushes_remove_and_message() {
let _r = Renderer::new(false);
    let mut c = client();
    c.friend_count = 1;
    c.friend_username[0] = "Bob".into();
    let mut layer = IfType::default();
    layer.r#type = ComponentType::TYPE_LAYER;
    layer.width = 190;
    layer.height = 261;
    layer.children = Some(vec![2]);
    layer.child_x = Some(vec![0]);
    layer.child_y = Some(vec![0]);
    let mut button = IfType::default();
    button.id = 2;
    button.r#type = ComponentType::TYPE_RECT;
    button.button_type = ButtonType::BUTTON_OK;
    button.button_text = "OK".into();
    button.client_code = 1; // CC_FRIENDS_START → friendUsername[0]
    button.width = 50;
    button.height = 15;
    c.cache.ifaces.resize(3, None);
    c.cache.ifaces[1] = Some(layer);
    c.cache.ifaces[2] = Some(button);
    c.side_modal_id = -1;
    c.side_icon[3] = 1;
    c.active_icon = 3;
    c.shell.mouse_x = 553 + 10;
    c.shell.mouse_y = 205 + 10;
    c.build_minimenu();
    let actions: Vec<i32> =
        (0..c.menu_num_entries).map(|i| c.menu_action[i as usize]).collect();
    assert!(!actions.contains(&MiniMenuAction::IF_BUTTON), "override suppresses IF_BUTTON");
    assert!(actions.contains(&MiniMenuAction::FRIENDLIST_DEL), "Remove");
    assert!(actions.contains(&MiniMenuAction::MESSAGE_PRIVATE), "Message");
    let remove = actions
        .iter()
        .position(|&a| a == MiniMenuAction::FRIENDLIST_DEL)
        .unwrap();
    assert_eq!(c.menu_option[remove], "Remove @whi@Bob");
}

/// A left click whose last menu entry is FRIENDLIST_ADD opens the
/// multi-entry menu instead of firing (TS 8370-8372).
#[test]
fn left_click_add_friend_last_entry_opens_menu() {
let _r = Renderer::new(false);
    let mut c = client();
    let mut local = ClientPlayer::default();
    local.name = Some("Me".into());
    c.local_player = Some(local);
    c.chat_public_mode = 1;
    c.friend_count = 1;
    c.friend_username[0] = "Bob".into();
    c.add_chat(1, "hello", "Bob");
    c.shell.mouse_x = 200;
    c.shell.mouse_y = 420;
    c.build_minimenu();
    let last = c.menu_num_entries - 1;
    assert_eq!(
        c.menu_action[last as usize],
        MiniMenuAction::FRIENDLIST_ADD,
        "last entry is Add friend"
    );
    c.shell.apply_mouse_down(1, 200, 420);
    c.shell.latch_click();
    c.mouse_loop();
    assert!(c.is_menu_open, "the add-friend last entry must open the menu");
}
