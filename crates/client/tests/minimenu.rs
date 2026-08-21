// Minimenu chrome: `open_menu` clamps the menu into the panel holding the
// click (0 viewport, 1 side, 2 chat) and sizes it to the widest option.
// World picks: `add_world_options` fills the menu from `pix3d` picks.
// The /tmp cache has no packs, so `Client::new` falls back to
// `Cache::default()` and never touches the network (the /crc fetch on
// 127.0.0.1 is refused instantly).
use client::client::{Client, ClientConfig, MiniMenuAction};
use client::config::{LocType, NpcType, ObjType};
use client::dash3d::{ClientNpc, ClientObj, ClientPlayer, Model, SceneModel};
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
    let mut c = client();
    c.menu_num_entries = 1;
    // typecode: entity 2, typeId 1, x=10, z=12
    let type_id = 1i32;
    let x = 10i32;
    let z = 12i32;
    let typecode = (2 << 29) | ((type_id & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.pix3d.picked_count = 1;
    c.pix3d.picked_entity_typecode[0] = typecode;
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
        Some(SceneModel::Model(Model::default())),
        typecode,
        0,
        1,
        1,
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
    let mut c = client();
    c.menu_num_entries = 1;
    // typecode: entity 1, npc slot 5, x=8, z=9
    let npc_slot = 5i32;
    let x = 8i32;
    let z = 9i32;
    let typecode = (1 << 29) | ((npc_slot & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.pix3d.picked_count = 1;
    c.pix3d.picked_entity_typecode[0] = typecode;
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
    let mut c = client();
    c.menu_num_entries = 1;
    c.shell.mouse_x = 50;
    c.shell.mouse_y = 80;
    // typecode: entity 0, player slot 3, x=6, z=7
    let player_slot = 3i32;
    let x = 6i32;
    let z = 7i32;
    let typecode = (0 << 29) | ((player_slot & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.pix3d.picked_count = 1;
    c.pix3d.picked_entity_typecode[0] = typecode;
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
    let mut c = client();
    c.menu_num_entries = 1;
    // typecode: entity 3, typeId 9 (idle), x=4, z=5
    let x = 4i32;
    let z = 5i32;
    let typecode = (3 << 29) | ((9 & 0x7fff) << 14) | ((z & 0x7f) << 7) | (x & 0x7f);
    c.pix3d.picked_count = 1;
    c.pix3d.picked_entity_typecode[0] = typecode;
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
    let mut c = client();
    c.menu_num_entries = 1;
    let typecode = (3 << 29) | ((9 & 0x7fff) << 14) | ((5 & 0x7f) << 7) | (4 & 0x7f);
    c.pix3d.picked_count = 2;
    c.pix3d.picked_entity_typecode[0] = typecode;
    c.pix3d.picked_entity_typecode[1] = typecode;
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

