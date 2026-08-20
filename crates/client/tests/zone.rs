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
use client::dash3d::{BuildArea, ClientProj, CollisionMap, LocShape, MapSpotAnim, Model, SceneModel, World};
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
