use client::client::{Client, ClientBuild, ClientConfig};
use client::render::Renderer;
use client::config::{Cache, LocType, SeqType, SpotType};
use client::dash3d::{ClientObj, LocChange};
use client::datastruct::LinkList;
use client::io::{Packet, ServerProt};

#[test]
fn loc_change_defaults_end_time_minus_one() {
let _r = Renderer::new(false);
    let loc = LocChange::default();
    assert_eq!(loc.end_time, -1);
    assert_eq!(loc.new_type, 0);
}

#[test]
fn client_obj_roundtrips_in_link_list() {
let _r = Renderer::new(false);
    let mut list = LinkList::new();
    list.push(ClientObj::new(42, 5));
    assert_eq!(list.head().unwrap().id, 42);
    assert_eq!(list.head().unwrap().count, 5);
}

use client::dash3d::world::LevelHeightmaps;
use client::dash3d::{AnimFrame, BuildArea, ClientEntity, ClientPlayer, ClientProj, CollisionMap, LocShape, MapSpotAnim, Model, SceneModel, World};

#[test]
fn rotate_x_axis_90_swaps_y_and_z() {
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
    let mut p = ClientProj::new(0, 0, 0, 100, 0, 0, 10, 0, 64, 0, 0);
    p.set_target(128.0, 100.0, 0.0, 0);
    // d=128, startpos=64 → x = 0 + 128*64/128 = 64
    assert!((p.x - 64.0).abs() < 1e-6);
    assert!((p.z - 0.0).abs() < 1e-6);
    assert!((p.y - 100.0).abs() < 1e-6);
}

#[test]
fn map_spot_anim_start_cycle_is_cycle_plus_delay() {
let _r = Renderer::new(false);
    let s = MapSpotAnim::new(0, 0, 64, 64, 0, 10, 5);
    assert_eq!(s.start_cycle, 15);
    assert!(!s.anim_complete);
}

#[test]
fn client_proj_move_by_uses_bound_seq_delays() {
let _r = Renderer::new(false);
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

#[test]
fn client_proj_bind_seq_resolves_zero_delays_via_animframe_fallback() {
let _r = Renderer::new(false);
    // One AnimFrame in the process-wide store: id 0, delay 2. A seq frame
    // with raw delay 0 falls back to its transform's delay; a transform id
    // that is not in the store falls back to 1 (`SeqType::getDelay`).
    AnimFrame::unpack(&[
        0, 1, // head: total frames = 1
        0, 0, // head: frame id = 0
        0, // head: group count = 0
        2, // del: frame delay = 2
        0, // base: size 0
        0, 3, // headLength = 3
        0, 0, // tran1Length = 0
        0, 0, // tran2Length = 0
        0, 1, // delLength = 1
    ]);

    let cache = Cache {
        seqs: vec![
            SeqType {
                num_frames: 2,
                frames: Some(vec![0, 1]),
                iframes: Some(vec![0, 1]),
                delay: Some(vec![0, 0]),
                ..SeqType::default()
            },
            SeqType {
                num_frames: 2,
                frames: Some(vec![999, 999]),
                iframes: Some(vec![0, 0]),
                delay: Some(vec![0, 0]),
                ..SeqType::default()
            },
        ],
        spots: vec![
            SpotType::default(),
            SpotType { id: 1, seq: Some(0), ..SpotType::default() },
            SpotType { id: 2, seq: Some(1), ..SpotType::default() },
        ],
        ..Cache::default()
    };

    // Seq 0: raw delays [0, 0] resolve to [2, 1] (frame 0 → AnimFrame 0's
    // delay 2, frame 1 → missing transform → 1).
    let mut p = ClientProj::new(1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    p.bind_seq(&cache);
    p.anim_cycle = 0;
    p.move_by(1);
    // anim_cycle 1: raw [0, 0] would advance to frame 1 (cycle 0); the
    // resolved delays hold the frame.
    assert_eq!(p.anim_frame, 0);
    assert_eq!(p.anim_cycle, 1);

    // Seq 1: raw delays [0, 0], both transforms missing → resolved [1, 1].
    let mut q = ClientProj::new(2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    q.bind_seq(&cache);
    q.anim_cycle = 1;
    q.move_by(1);
    // anim_cycle 2: raw [0, 0] would wrap back to frame 0 (cycle 0); the
    // resolved [1, 1] advances one frame.
    assert_eq!(q.anim_frame, 1);
    assert_eq!(q.anim_cycle, 0);
}

fn empty_world() -> World {
    let groundh: LevelHeightmaps =
        vec![vec![vec![0i32; 105]; 105]; BuildArea::LEVELS as usize];
    World::new(groundh, BuildArea::SIZE, BuildArea::LEVELS, BuildArea::SIZE)
}

#[test]
fn scene_model_proj_min_y_defaults_1000() {
let _r = Renderer::new(false);
    let p = ClientProj::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let sm = SceneModel::Proj(p);
    assert_eq!(sm.min_y(), 1000);
}

#[test]
fn world_dynamic_count_starts_zero() {
let _r = Renderer::new(false);
    let w = empty_world();
    assert_eq!(w.dynamic_count(), 0);
}

#[test]
fn get_wall_mut_none_on_empty_tile() {
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
    let mut c = client();
    c.projectiles.push(client::dash3d::ClientProj::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0));
    c.logout();
    assert!(c.projectiles.head().is_none());
}

// --- LocType.checkModel + ClientBuild.changeLocAvailable/Unchecked ---

#[test]
fn check_model_none_is_ready() {
let _r = Renderer::new(false);
    assert!(LocType::default().check_model(0));
}

#[test]
fn change_loc_available_remaps_shape_11_to_10() {
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
    let mut c = client();
    seed_obj(&mut c, 3, 10);
    obj_add(&mut c, 0x11, 3, 1);
    let list = c.ground_obj[0][1][1].as_mut().expect("list");
    assert_eq!(list.head().unwrap().id, 3);
    assert!(c.world.ground_object_at(0, 1, 1).is_some());
}

#[test]
fn obj_del_clears_empty_cell() {
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
    let mut r = Renderer::new(false);
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
    r.game_draw(&mut c);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
    let mut r = Renderer::new(false);
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
    r.game_draw(&mut c);
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

// --- MAP_ANIM + addMapAnim ---

/// MAP_ANIM pushes a `MapSpotAnim`; the real `add_map_anim` path (inside
/// `gameDrawMain`) advances it and places a dynamic sprite. `removeSprites`
/// clears the frame's dynamic sprites at the end of the 3D pass, so after
/// `game_draw` the loop's effect shows as the advanced anim (`update` ran
/// with delta `world_update_num`) and render_all having run.
#[test]
fn map_anim_pushes_and_add_map_anim_places_dynamic() {
let _r = Renderer::new(false);
    let mut r = Renderer::new(false);
    let mut c = client();
    seed_spot(&mut c, 0);
    // A seq whose delays keep the anim in-frame for this delta lets
    // `update` advance `anim_cycle` without completing the anim.
    c.cache.spots[0].seq = Some(1);
    while c.cache.seqs.len() <= 1 {
        c.cache.seqs.push(SeqType::default());
    }
    c.cache.seqs[1].num_frames = 2;
    c.cache.seqs[1].frames = Some(vec![0, 1]);
    c.cache.seqs[1].iframes = Some(vec![0, 1]);
    c.cache.seqs[1].delay = Some(vec![10, 5]);
    c.loop_cycle = 5;
    let mut p = Packet::alloc(0);
    p.p1(0x11);
    p.p2(0);
    p.p1(0);
    p.p2(0); // time 0 → start_cycle = loop_cycle
    p.pos = 0;
    c.handle_packet(ServerProt::MAP_ANIM, &mut p);
    assert!(c.spotanims.head().is_some());
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    c.world_update_num = 1;
    r.game_draw(&mut c);
    let spot = c.spotanims.head().expect("spot stays linked");
    assert_eq!(spot.anim_cycle, 1, "add_map_anim must update the spot");
    assert!(c.world.render_count() > 0, "gameDrawMain must run render_all");
}

// --- P_LOCMERGE ---

/// P_LOCMERGE with a loc whose model is not built (no pack bytes seeded)
/// must consume its bytes and no-op: no locChange, no player loc_* writes.
/// The Some-model branch (locChange + loc offsets) is covered by live proof.
#[test]
fn p_locmerge_consumes_and_noops_without_model() {
let _r = Renderer::new(false);
    let mut c = client();
    seed_anim_loc(&mut c, 0); // get_model is still None without pack bytes
    c.self_slot = 3;
    c.local_player = Some(ClientPlayer::at(1, 1));
    c.loop_cycle = 10;
    c.ingame = true;
    let mut p = Packet::new(vec![
        0x11, // pos → tile (1,1)
        0x00, // info → shape 0, rotate 0
        0x00, 0x00, // id
        0x00, 0x02, // t1
        0x00, 0x05, // t2
        0x00, 0x03, // pid
        0x00, 0x00, 0x00, 0x00, // east, south, west, north
    ]);
    c.handle_packet(ServerProt::P_LOCMERGE, &mut p);
    assert_eq!(p.pos, p.length());
    assert!(c.ingame);
    // TS writes loc_model / locChange only inside `if (model)`. No pack → None.
    assert!(c.local_player.as_ref().unwrap().loc_model.is_none());
    assert!(c.loc_changes.head().is_none());
}

/// A pid that resolves to no player must stop the whole arm before any
/// apply: no `cache.loc(id)` index (TS skips it for missing players), no
/// locChange. `id` 0xffff is past every locs table, so the misplaced order
/// (get_model + locChange before the player gate) would panic on the
/// unguarded index.
#[test]
fn p_locmerge_no_player_stops_before_apply() {
let _r = Renderer::new(false);
    let mut c = client();
    c.self_slot = 5;
    c.ingame = true;
    let mut p = Packet::new(vec![
        0x11, // pos → tile (1,1)
        0x00, // info → shape 0, rotate 0
        0xff, 0xff, // id 0xffff — beyond cache.locs
        0x00, 0x02, // t1
        0x00, 0x05, // t2
        0x00, 0x09, // pid 9 — players[9] is None
        0x00, 0x00, 0x00, 0x00, // east, south, west, north
    ]);
    c.handle_packet(ServerProt::P_LOCMERGE, &mut p);
    assert_eq!(p.pos, p.length());
    assert!(c.ingame);
    assert!(c.loc_changes.head().is_none());
}

// --- UPDATE_ZONE_FULL_FOLLOWS + REBUILD_NORMAL shift ---

/// FULL_FOLLOWS nulls the 8×8 ground-obj cells on `minusedlevel` and
/// expires every loc change inside that zone (`end_time = 0`).
#[test]
fn full_follows_clears_8x8_ground_obj_and_expires_locs() {
let _r = Renderer::new(false);
    let mut c = client();
    seed_obj(&mut c, 1, 1);
    c.minusedlevel = 0;
    c.ground_obj[0][3][4] = Some({
        let mut l = LinkList::new();
        l.push(ClientObj::new(1, 1));
        l
    });
    c.loc_changes.push(LocChange {
        level: 0,
        x: 3,
        z: 4,
        end_time: -1,
        ..LocChange::default()
    });
    let mut p = Packet::alloc(0);
    p.p1(0); // zone x
    p.p1(0); // zone z
    p.pos = 0;
    c.handle_packet(ServerProt::UPDATE_ZONE_FULL_FOLLOWS, &mut p);
    assert!(c.ground_obj[0][3][4].is_none());
    assert_eq!(c.loc_changes.head().unwrap().end_time, 0);
}

/// REBUILD_NORMAL shifts ground_obj and loc changes by the base delta.
/// Centre (10,10) → base 32, prev base 32; the packet's zone (11,10) →
/// base 40, so dx = 8 and `last_x = x + dx` moves tile 20 to 12.
#[test]
fn rebuild_normal_shifts_ground_obj_by_base_delta() {
let _r = Renderer::new(false);
    let mut c = client();
    seed_obj(&mut c, 1, 1);
    c.map_build_centre_zone_x = 10;
    c.map_build_centre_zone_z = 10;
    c.map_build_base_x = 32;
    c.map_build_base_z = 32;
    c.map_build_prev_base_x = 32;
    c.map_build_prev_base_z = 32;
    c.scene_state = 2;
    c.ground_obj[0][20][20] = Some({
        let mut l = LinkList::new();
        l.push(ClientObj::new(1, 1));
        l
    });
    c.loc_changes.push(LocChange {
        x: 20,
        z: 20,
        ..LocChange::default()
    });
    let mut p = Packet::alloc(0);
    p.p2(11);
    p.p2(10);
    p.pos = 0;
    c.handle_packet(ServerProt::REBUILD_NORMAL, &mut p);
    // dx = (11-6)*8 - 32 = 40-32 = 8; last_x = x+8 so tile 12 gets old 20
    assert!(c.ground_obj[0][12][20].is_some());
    assert!(c.ground_obj[0][20][20].is_none());
    assert_eq!(c.loc_changes.head().unwrap().x, 12);
}

/// A same-zone REBUILD_NORMAL while `scene_state != 2` skips the early
/// return, so the shift body runs with dx == 0 && dz == 0. The TS
/// self-assign `groundObj[level][x][z] = groundObj[level][x][z]` is a no-op
/// that preserves stacked items; a `.take()` of the same cell would null it.
#[test]
fn rebuild_normal_zero_delta_preserves_ground_obj() {
let _r = Renderer::new(false);
    let mut c = client();
    seed_obj(&mut c, 1, 1);
    c.map_build_centre_zone_x = 10;
    c.map_build_centre_zone_z = 10;
    c.map_build_base_x = 32;
    c.map_build_base_z = 32;
    c.map_build_prev_base_x = 32;
    c.map_build_prev_base_z = 32;
    c.scene_state = 1; // != 2, so the same-zone early return is not taken
    c.ground_obj[0][20][20] = Some({
        let mut l = LinkList::new();
        l.push(ClientObj::new(1, 1));
        l
    });
    let mut p = Packet::alloc(0);
    p.p2(10); // same zone → base 32 → dx = 0, dz = 0
    p.p2(10);
    p.pos = 0;
    c.handle_packet(ServerProt::REBUILD_NORMAL, &mut p);
    assert!(c.ground_obj[0][20][20].is_some(), "zero-delta shift must not clear the cell");
}

/// dx < 0 must scan descending (from tile SIZE-1) so each source tile is
/// still unread when its value is moved. Zone 9 → base 24, dx = -8: tile 12
/// lands at tile 20, and the loc change shifts the same way.
#[test]
fn rebuild_normal_negative_dx_shifts_descending() {
let _r = Renderer::new(false);
    let mut c = client();
    seed_obj(&mut c, 1, 1);
    c.map_build_centre_zone_x = 10;
    c.map_build_centre_zone_z = 10;
    c.map_build_base_x = 32;
    c.map_build_base_z = 32;
    c.map_build_prev_base_x = 32;
    c.map_build_prev_base_z = 32;
    c.scene_state = 2;
    c.ground_obj[0][12][20] = Some({
        let mut l = LinkList::new();
        l.push(ClientObj::new(1, 1));
        l
    });
    c.loc_changes.push(LocChange {
        x: 12,
        z: 20,
        ..LocChange::default()
    });
    let mut p = Packet::alloc(0);
    p.p2(9); // base (9-6)*8 = 24 → dx = 24-32 = -8
    p.p2(10);
    p.pos = 0;
    c.handle_packet(ServerProt::REBUILD_NORMAL, &mut p);
    // last_x = x + dx = x - 8, so the value at tile 12 lands at tile 20.
    assert!(c.ground_obj[0][20][20].is_some());
    assert!(c.ground_obj[0][12][20].is_none());
    assert_eq!(c.loc_changes.head().unwrap().x, 20);
}
