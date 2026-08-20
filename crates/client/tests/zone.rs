use client::client::{Client, ClientBuild, ClientConfig};
use client::config::{Cache, LocType, SeqType, SpotType};
use client::dash3d::{ClientObj, LocChange};
use client::datastruct::{LinkList, LinkableTrait};
use client::io::{Packet, ServerProt};

#[test]
fn loc_change_defaults_end_time_minus_one() {
    let loc = LocChange::default();
    assert_eq!(loc.end_time, -1);
    assert_eq!(loc.new_type, 0);
}

#[test]
fn client_obj_roundtrips_in_link_list() {
    let mut list = LinkList::new();
    list.push(ClientObj::new(42, 5));
    assert_eq!(list.head().unwrap().id, 42);
    assert_eq!(list.head().unwrap().count, 5);
}

use client::dash3d::world::LevelHeightmaps;
use client::dash3d::{BuildArea, ClientEntity, ClientPlayer, ClientProj, CollisionMap, LocShape, MapSpotAnim, Model, SceneModel, World};
use client::graphics::Pix3D;

#[test]
fn rotate_x_axis_90_swaps_y_and_z() {
    let mut m = Model::default();
    m.num_points = 1;
    m.point_x = Some(vec![0]);
    m.point_y = Some(vec![128]);
    m.point_z = Some(vec![0]);
    m.rotate_x_axis(512); // 90° in 2048-circle; sin≈65536, cos≈0
    let y = m.point_y.as_ref().unwrap()[0];
    let z = m.point_z.as_ref().unwrap()[0];
    assert_eq!(y, 0);
    assert_eq!(z, 128);
}

#[test]
fn client_proj_set_target_places_startpos_along_delta() {
    let mut p = ClientProj::new(0, 0, 0, 100, 0, 0, 10, 0, 64, 0, 0);
    p.set_target(128.0, 100.0, 0.0, 0);
    // d=128, startpos=64 → x = 0 + 128*64/128 = 64
    assert!((p.x - 64.0).abs() < 1e-6);
    assert!((p.z - 0.0).abs() < 1e-6);
    assert!((p.y - 100.0).abs() < 1e-6);
}

#[test]
fn map_spot_anim_start_cycle_is_cycle_plus_delay() {
    let s = MapSpotAnim::new(0, 0, 64, 64, 0, 10, 5);
    assert_eq!(s.start_cycle, 15);
    assert!(!s.anim_complete);
}

#[test]
fn client_proj_move_by_uses_bound_seq_delays() {
    let cache = Cache {
        seqs: vec![
            SeqType::default(),
            SeqType::default(),
            SeqType::default(),
            SeqType { num_frames: 2, frames: Some(vec![0, 1]), iframes: Some(vec![0, 1]), delay: Some(vec![10, 5]), ..SeqType::default() },
        ],
        spots: vec![
            SpotType::default(),
            SpotType { id: 1, seq: Some(3), ..SpotType::default() },
        ],
        ..Cache::default()
    };

    let mut p = ClientProj::new(1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    p.bind_seq(&cache);
    p.anim_cycle = 20;
    p.move_by(1);
    // 21 > 10 → cycle 10, frame 1; 10 > 5 → cycle 4, frame 2 → wrap to 0
    assert_eq!(p.anim_frame, 0);
    assert_eq!(p.anim_cycle, 4);

    // Unbound seq leaves the anim loop skipped.
    let mut q = ClientProj::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    q.anim_cycle = 20;
    q.move_by(1);
    assert_eq!(q.anim_frame, 0);
    assert_eq!(q.anim_cycle, 20);
}

fn empty_world() -> World {
    let groundh: LevelHeightmaps =
        vec![vec![vec![0i32; 105]; 105]; BuildArea::LEVELS as usize];
    World::new(groundh, BuildArea::SIZE, BuildArea::LEVELS, BuildArea::SIZE)
}

#[test]
fn scene_model_proj_min_y_defaults_1000() {
    let p = ClientProj::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let sm = SceneModel::Proj(p);
    assert_eq!(sm.min_y(), 1000);
}

#[test]
fn world_dynamic_count_starts_zero() {
    let w = empty_world();
    assert_eq!(w.dynamic_count(), 0);
}

#[test]
fn get_wall_mut_none_on_empty_tile() {
    let mut w = empty_world();
    assert!(w.get_wall_mut(0, 1, 1).is_none());
}

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
fn update_pid_sets_self_slot_and_members() {
    let mut c = client();
    let mut p = Packet::alloc(0);
    p.p2(7);
    p.p1(1);
    p.pos = 0;
    c.handle_packet(ServerProt::UPDATE_PID, &mut p);
    assert_eq!(c.self_slot, 7);
    assert_eq!(c.members_account, 1);
    assert_eq!(p.pos, 3);
}

#[test]
fn world_update_num_increments_when_draw_then_zeros_headless() {
    let mut c = client();
    assert_eq!(c.world_update_num, 0);
    c.ingame = true;
    c.game_loop(); // draw=false → increment then zero
    assert_eq!(c.world_update_num, 0);
    c.set_draw(true);
    c.game_loop(); // draw=true → increment, game_draw not called here
    assert!(c.world_update_num >= 1);
}

#[test]
fn logout_clears_zone_lists() {
    let mut c = client();
    c.projectiles.push(client::dash3d::ClientProj::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0));
    c.logout();
    assert!(c.projectiles.head().is_none());
}

// --- LocType.checkModel + ClientBuild.changeLocAvailable/Unchecked ---

#[test]
fn check_model_none_is_ready() {
    assert!(LocType::default().check_model(0));
}

#[test]
fn change_loc_available_remaps_shape_11_to_10() {
    let mut cache = Cache::default();
    cache.locs.push(LocType {
        model: Some(vec![60000]),
        shape: Some(vec![10]),
        ..LocType::default()
    });
    // shape 11 remaps to 10, matches shape array → request_download(60000) false
    assert!(!ClientBuild::change_loc_available(&cache, 0, 11));
}

#[test]
fn change_loc_unchecked_wall_straight_with_anim_sets_wall() {
    let mut cache = Cache::default();
    cache.locs.push(LocType {
        anim: 0,
        ..LocType::default()
    });
    cache.seqs.push(SeqType::default());
    let groundh: LevelHeightmaps =
        vec![vec![vec![0i32; 105]; 105]; BuildArea::LEVELS as usize];
    let mut world = World::new(groundh.clone(), BuildArea::SIZE, BuildArea::LEVELS, BuildArea::SIZE);
    let mut cmap = CollisionMap::new();
    ClientBuild::change_loc_unchecked(
        &cache, &mut world, Some(&mut cmap), &groundh,
        0, 2, 2, 0, LocShape::WALL_STRAIGHT, 0, 0, 0,
    );
    assert!(matches!(
        world.get_wall(0, 2, 2).and_then(|w| w.model1.as_ref()),
        Some(SceneModel::LocAnim(_))
    ));
}

fn seed_anim_loc(c: &mut Client, id: usize) {
    while c.cache.locs.len() <= id {
        c.cache.locs.push(LocType::default());
    }
    c.cache.locs[id].anim = 0;
    if c.cache.seqs.is_empty() {
        c.cache.seqs.push(client::config::SeqType::default());
    }
}

fn loc_add_payload(pos: u8, info: u8, id: u16) -> Packet {
    let mut p = Packet::alloc(0);
    p.p1(pos as i32);
    p.p1(info as i32);
    p.p2(id as i32);
    p.pos = 0;
    p
}

#[test]
fn loc_add_change_applies_on_do_queue() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    c.zone_update_x = 0;
    c.zone_update_z = 0;
    let mut p = loc_add_payload(0x11, 0x00, 0); // tile (1,1), shape 0 wall
    c.handle_packet(ServerProt::LOC_ADD_CHANGE, &mut p);
    c.game_loop();
    assert!(c.world.get_wall(0, 1, 1).is_some());
}

#[test]
fn loc_add_change_waits_when_model_not_ready() {
    let mut c = client();
    if c.cache.locs.is_empty() {
        c.cache.locs.push(LocType::default());
    }
    // shape 0 with model 60000 not downloaded → check_model(0) is false
    c.cache.locs[0].model = Some(vec![60000]);
    c.cache.locs[0].shape = Some(vec![0]);
    c.scene_state = 2;
    c.ingame = true;
    let mut p = loc_add_payload(0x11, 0x00, 0);
    c.handle_packet(ServerProt::LOC_ADD_CHANGE, &mut p);
    c.game_loop();
    assert!(c.world.get_wall(0, 1, 1).is_none());
    assert!(c.loc_changes.head().is_some());
}

/// WALL_DECOR deletes must look the tile up at (x, z): the TS
/// `decorType(level, z, x)` names its parameters backwards but indexes
/// `squares[level][x][z]`, so a swapped call would leave a decor seeded at
/// (2, 5) in place.
#[test]
fn loc_del_wall_decor_uses_tile_xz_not_swapped() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    c.zone_update_x = 0;
    c.zone_update_z = 0;
    // Seed a wall-decor at (2, 5) directly; a swapped decor_type lookup
    // reads (5, 2) and finds nothing, so the delete would be skipped.
    c.world.set_decor(
        0,
        2,
        5,
        0,
        0,
        0,
        0x40000000,
        Some(SceneModel::Model(Model::default())),
        0,
        0,
        0,
    );
    // pos 0x25 → tile (2, 5), info 0x10 → shape 4 (wall-decor), LOC_DEL.
    let mut p = Packet::alloc(0);
    p.p1(0x25);
    p.p1(0x10);
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_DEL, &mut p);
    c.game_loop();
    assert!(c.world.get_decor(0, 2, 5).is_none());
}

#[test]
fn loc_del_removes_wall() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    let mut p = loc_add_payload(0x11, 0x00, 0);
    c.handle_packet(ServerProt::LOC_ADD_CHANGE, &mut p);
    c.game_loop();
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p1(0x00);
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_DEL, &mut p);
    c.game_loop();
    assert!(c.world.get_wall(0, 1, 1).is_none());
}

/// `info` bytes whose shape (info >> 2) is past the 23-entry
/// `LOC_SHAPE_TO_LAYER` table must skip the apply, not panic and log out.
#[test]
fn loc_add_change_out_of_range_shape_does_not_panic() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    let mut p = loc_add_payload(0x11, 0x5c, 0); // info 0x5c → shape 23
    c.handle_packet(ServerProt::LOC_ADD_CHANGE, &mut p);
    c.game_loop();
    assert!(c.ingame);
    assert!(c.world.get_wall(0, 1, 1).is_none());
    assert!(c.loc_changes.head().is_none());
}

// --- LOC_ANIM: ClientLocAnim onto wall/decor/sprite/gd (no queue) ---

/// A wall that was overwritten with a plain `Obj` model must come back as a
/// `LocAnim` after the LOC_ANIM packet. Heights read `groundh` with the
/// addLoc names (SW=[x][z], SE=[x+1][z], NE=[x+1][z+1], NW=[x][z+1]); the
/// distinct seeded values fail the assert if a transposed order is used.
#[test]
fn loc_anim_replaces_wall_model1() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    c.zone_update_x = 0;
    c.zone_update_z = 0;
    c.groundh[0][1][1] = 100;
    c.groundh[0][2][1] = 200;
    c.groundh[0][2][2] = 300;
    c.groundh[0][1][2] = 400;
    let mut p = loc_add_payload(0x11, 0x00, 0);
    c.handle_packet(ServerProt::LOC_ADD_CHANGE, &mut p);
    c.game_loop();
    if let Some(w) = c.world.get_wall_mut(0, 1, 1) {
        w.model1 = Some(SceneModel::Obj(ClientObj::new(0, 1)));
    }
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p1(0x00); // shape 0
    p.p2(0); // seq 0
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_ANIM, &mut p);
    let wall = c.world.get_wall(0, 1, 1).expect("wall present");
    let SceneModel::LocAnim(anim) = wall.model1.as_ref().expect("model1 set") else {
        panic!("model1 should be LocAnim, got Obj");
    };
    assert_eq!(anim.shape, 0);
    assert_eq!(anim.angle, 0);
    assert_eq!(anim.height_sw, 100);
    assert_eq!(anim.height_se, 200);
    assert_eq!(anim.height_ne, 300);
    assert_eq!(anim.height_nw, 400);
}

/// Shape 2 walls animate both `model1` (angle rotate+4) and `model2`
/// (angle (rotate+1)&3).
#[test]
fn loc_anim_wall_door_animates_both_models() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    let mut p = loc_add_payload(0x11, 0x08, 0); // shape 2
    c.handle_packet(ServerProt::LOC_ADD_CHANGE, &mut p);
    c.game_loop();
    if let Some(w) = c.world.get_wall_mut(0, 1, 1) {
        w.model1 = Some(SceneModel::Obj(ClientObj::new(0, 1)));
        w.model2 = Some(SceneModel::Obj(ClientObj::new(0, 1)));
    }
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p1(0x08); // shape 2, rotate 0
    p.p2(0);
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_ANIM, &mut p);
    let wall = c.world.get_wall(0, 1, 1).expect("wall present");
    let SceneModel::LocAnim(m1) = wall.model1.as_ref().expect("model1 set") else {
        panic!("model1 should be LocAnim");
    };
    let SceneModel::LocAnim(m2) = wall.model2.as_ref().expect("model2 set") else {
        panic!("model2 should be LocAnim");
    };
    assert_eq!(m1.shape, 2);
    assert_eq!(m1.angle, 4);
    assert_eq!(m2.shape, 2);
    assert_eq!(m2.angle, 1);
}

/// WALL_DECOR applies to the decor at tile (x, z): the TS
/// `decorType(level, z, x)` names its parameters backwards but indexes
/// `squares[level][x][z]`, so a swapped call leaves the seeded decor
/// untouched. Seed at (2, 5) so x != z and the swap would miss.
#[test]
fn loc_anim_wall_decor_uses_tile_xz() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    c.zone_update_x = 0;
    c.zone_update_z = 0;
    c.world.set_decor(
        0,
        2,
        5,
        0,
        0,
        0,
        0x40000000,
        Some(SceneModel::Model(Model::default())),
        0,
        0,
        0,
    );
    let mut p = Packet::alloc(0);
    p.p1(0x25); // tile (2, 5)
    p.p1(0x10); // shape 4 → WALL_DECOR
    p.p2(0);
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_ANIM, &mut p);
    let decor = c.world.get_decor(0, 2, 5).expect("decor present");
    assert!(matches!(decor.model, SceneModel::LocAnim(_)));
}

/// GROUND scenery (layer 2): shape 11 remaps to 10 before building the
/// ClientLocAnim on the sprite.
#[test]
fn loc_anim_scene_remaps_shape_11_to_10() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    // typecode bits 29-30 == 2 marks the sprite as a scene (get_scene_mut
    // looks for that bit pattern on the tile's sprites).
    c.world.add_scenery(
        0,
        1,
        1,
        0,
        Some(SceneModel::Model(Model::default())),
        0x40000000,
        0,
        1,
        1,
        0,
    );
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p1(0x2c); // shape 11
    p.p2(0);
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_ANIM, &mut p);
    let sprite = c.world.get_scene(0, 1, 1).expect("sprite present");
    let SceneModel::LocAnim(anim) = sprite.model.as_ref().expect("sprite model set") else {
        panic!("sprite model should be LocAnim");
    };
    assert_eq!(anim.shape, 10);
}

/// GROUND_DECOR (layer 3) animates the ground decor's model with shape 22.
#[test]
fn loc_anim_ground_decor_model() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    c.world.set_ground_decor(
        Some(SceneModel::Model(Model::default())),
        0,
        1,
        1,
        0,
        0x40000000,
        0,
    );
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p1(0x58); // shape 22 → GROUND_DECOR
    p.p2(0);
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_ANIM, &mut p);
    let gd = c.world.get_gd(0, 1, 1).expect("ground decor present");
    let SceneModel::LocAnim(anim) = gd.model.as_ref().expect("gd model set") else {
        panic!("gd model should be LocAnim");
    };
    assert_eq!(anim.shape, 22);
}

/// The LOC_ANIM arm must apply the same `shape < LOC_SHAPE_TO_LAYER.len()`
/// gate as LOC_ADD_CHANGE / LOC_DEL: a shape past the 23-entry table is
/// skipped instead of panicking and logging out.
#[test]
fn loc_anim_out_of_range_shape_does_not_panic() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.scene_state = 2;
    c.ingame = true;
    let mut p = loc_add_payload(0x11, 0x00, 0);
    c.handle_packet(ServerProt::LOC_ADD_CHANGE, &mut p);
    c.game_loop();
    if let Some(w) = c.world.get_wall_mut(0, 1, 1) {
        w.model1 = Some(SceneModel::Obj(ClientObj::new(0, 1)));
    }
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p1(0x5c); // shape 23
    p.p2(0);
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_ANIM, &mut p);
    assert!(c.ingame);
    assert!(matches!(
        c.world.get_wall(0, 1, 1).and_then(|w| w.model1.as_ref()),
        Some(SceneModel::Obj(_))
    ));
}

/// No wall on the tile → LOC_ANIM is a no-op (bytes consumed, no panic).
#[test]
fn loc_anim_missing_wall_is_noop() {
    let mut c = client();
    seed_anim_loc(&mut c, 0);
    c.ingame = true;
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p1(0x00);
    p.p2(0);
    p.pos = 0;
    c.handle_packet(ServerProt::LOC_ANIM, &mut p);
    assert!(c.ingame);
    assert!(c.world.get_wall(0, 1, 1).is_none());
}

// --- OBJ_ADD / OBJ_DEL / OBJ_COUNT / OBJ_REVEAL + showObject ---

fn seed_obj(c: &mut Client, id: usize, cost: i32) {
    while c.cache.objs.len() <= id {
        c.cache.objs.push(client::config::ObjType::default());
    }
    c.cache.objs[id].id = id as i32;
    c.cache.objs[id].cost = cost;
}

fn obj_add(c: &mut Client, pos: i32, id: i32, count: i32) {
    let mut p = Packet::alloc(0);
    p.p1(pos);
    p.p2(id);
    p.p2(count);
    p.pos = 0;
    c.handle_packet(ServerProt::OBJ_ADD, &mut p);
}

#[test]
fn obj_add_sets_ground_object() {
    let mut c = client();
    seed_obj(&mut c, 3, 10);
    obj_add(&mut c, 0x11, 3, 1);
    let list = c.ground_obj[0][1][1].as_mut().expect("list");
    assert_eq!(list.head().unwrap().id, 3);
    assert!(c.world.ground_object_at(0, 1, 1).is_some());
}

#[test]
fn obj_del_clears_empty_cell() {
    let mut c = client();
    seed_obj(&mut c, 3, 10);
    obj_add(&mut c, 0x11, 3, 1);
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p2(3);
    p.pos = 0;
    c.handle_packet(ServerProt::OBJ_DEL, &mut p);
    assert!(c.ground_obj[0][1][1].is_none());
    assert!(c.world.ground_object_at(0, 1, 1).is_none());
}

#[test]
fn obj_count_rewrites_matching_stack() {
    let mut c = client();
    seed_obj(&mut c, 3, 10);
    obj_add(&mut c, 0x11, 3, 2);
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p2(3);
    p.p2(2);
    p.p2(9);
    p.pos = 0;
    c.handle_packet(ServerProt::OBJ_COUNT, &mut p);
    assert_eq!(c.ground_obj[0][1][1].as_mut().unwrap().head().unwrap().count, 9);
}

#[test]
fn obj_reveal_skips_self_slot() {
    let mut c = client();
    seed_obj(&mut c, 3, 10);
    c.self_slot = 5;
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p2(3);
    p.p2(1);
    p.p2(5);
    p.pos = 0;
    c.handle_packet(ServerProt::OBJ_REVEAL, &mut p);
    assert!(c.ground_obj[0][1][1].is_none());
}

#[test]
fn obj_reveal_adds_for_other_pid() {
    let mut c = client();
    seed_obj(&mut c, 3, 10);
    c.self_slot = 5;
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p2(3);
    p.p2(1);
    p.p2(6);
    p.pos = 0;
    c.handle_packet(ServerProt::OBJ_REVEAL, &mut p);
    assert!(c.ground_obj[0][1][1].is_some());
}

#[test]
fn obj_add_headless_still_sets_world() {
    let mut c = client();
    assert!(!c.draw);
    seed_obj(&mut c, 3, 10);
    obj_add(&mut c, 0x11, 3, 1);
    assert!(c.world.ground_object_at(0, 1, 1).is_some());
}

// --- MAP_PROJANIM + addProjectiles ---

/// Seed `cache.spots[id]` so `ClientProj.get_temp_model` during
/// `render_all` does not panic on the spotanim index.
fn seed_spot(c: &mut Client, id: usize) {
    while c.cache.spots.len() <= id {
        c.cache.spots.push(SpotType::default());
    }
}

/// MAP_PROJANIM pushes a `ClientProj`; the real `add_projectiles` path
/// (inside `gameDrawMain`) advances it and places a dynamic sprite.
/// `removeSprites` clears the frame's dynamic sprites at the end of the 3D
/// pass (TS `renderAll` then `removeSprites`), so after `game_draw` the
/// observable is the moved projectile and the 3D pass having run.
#[test]
fn map_projanim_pushes_and_add_projectiles_places_dynamic() {
    let mut c = client();
    seed_spot(&mut c, 0);
    c.loop_cycle = 5;
    let mut p = Packet::alloc(0);
    p.p1(0x11); // tile 1,1
    p.p1(4);    // x2 offset 4 → dest tile 5,1 (nonzero flight, no NaN)
    p.p1(0);    // z2
    p.p2(0);    // target 0
    p.p2(0);    // spotanim
    p.p1(1);    // h1 (×4 in apply)
    p.p1(1);    // h2
    p.p2(0);    // t1
    p.p2(10);   // t2
    p.p1(0);
    p.p1(0);
    p.pos = 0;
    c.handle_packet(ServerProt::MAP_PROJANIM, &mut p);
    assert!(c.projectiles.head().is_some());
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    c.world_update_num = 1;
    c.game_draw();
    // loop_cycle 5 ∈ [t1+5, t2+5] → addProjectiles moved the proj (mobile)
    // and addDynamic placed it; frame-end removeSprites zeroed the count.
    let proj = c.projectiles.head().unwrap();
    assert!(proj.mobile, "addProjectiles must move the projectile");
    assert_eq!(c.world.render_count(), 1, "gameDrawMain must run render_all");
}

/// src tile (101,101) is in `0..104` but the dest `x2 = 101 + 10 = 111`
/// is not: the MAP_PROJANIM arm must consume the bytes without pushing.
#[test]
fn map_projanim_out_of_range_dest_is_dropped() {
    let mut c = client();
    c.zone_update_x = 100;
    c.zone_update_z = 100;
    let mut p = Packet::alloc(0);
    p.p1(0x11); // src tile 101,101 — in range
    p.p1(10);   // x2 offset +10 → dest tile 111 — out of 0..104
    p.p1(0);    // z2 offset 0 → dest z 101 — in range
    p.p2(0);    // target 0
    p.p2(0);    // spotanim
    p.p1(0);    // h1 (×4 in apply)
    p.p1(0);    // h2
    p.p2(0);    // t1
    p.p2(0);    // t2
    p.p1(0);
    p.p1(0);
    p.pos = 0;
    c.handle_packet(ServerProt::MAP_PROJANIM, &mut p);
    assert!(c.projectiles.head().is_none());
}

/// A negative `target` retargets the proj onto a player:
/// `index = -target - 1`, and `index == self_slot` resolves to the local
/// player. The packet aims at dest tile (5,1) (scene 576,192); the local
/// player stands at scene (704,704), so after `add_projectiles` both x and
/// z must have stepped toward the player — a proj that only tracked the
/// packet dest would leave z at 192.
#[test]
fn map_projanim_negative_target_retargets_to_local_player() {
    let mut c = client();
    seed_spot(&mut c, 0);
    c.self_slot = 5;
    let mut player = ClientPlayer::default();
    player.ready = true;
    player.entity = ClientEntity::at(5, 5);
    player.entity.teleport(&c.cache, true, 5, 5); // scene x/z 704
    c.local_player = Some(player);

    c.loop_cycle = 5;
    let mut p = Packet::alloc(0);
    p.p1(0x11); // tile 1,1 → scene 192,192
    p.p1(4);    // x2 offset 4 → dest tile 5,1 (in range)
    p.p1(0);    // z2 offset 0 → dest z 192
    p.p2(-6);   // target -(self_slot + 1) → local player index 5
    p.p2(0);    // spotanim
    p.p1(1);    // h1 (×4 in apply)
    p.p1(1);    // h2
    p.p2(0);    // t1
    p.p2(10);   // t2
    p.p1(0);
    p.p1(0);
    p.pos = 0;
    c.handle_packet(ServerProt::MAP_PROJANIM, &mut p);
    let proj = c.projectiles.head().expect("proj pushed");
    assert_eq!(proj.target, -6);

    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    c.world_update_num = 1;
    c.game_draw();
    let proj = c.projectiles.head().unwrap();
    assert!(proj.mobile, "addProjectiles must move the projectile");
    // startpos 0 leaves the src; the retarget must aim at (704,704).
    assert!(
        proj.x > 192.0 && proj.x < 704.0,
        "proj must track the local player on x, got {}",
        proj.x
    );
    assert!(
        proj.z > 192.0 && proj.z < 704.0,
        "proj must track the local player on z, got {}",
        proj.z
    );
}
