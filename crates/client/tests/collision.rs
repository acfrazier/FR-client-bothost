// CollisionMap / World scene-graph port (Task 15): block flags, loc/wall
// placement, and entity route behaviour, pinned against `dash3d/CollisionMap.ts`.
use client::client::{Client, ClientConfig, ClientNpc, ClientPlayer};
use client::dash3d::{CollisionFlag, CollisionMap, DirectionFlag, LocAngle, LocShape};

#[test]
fn block_ground_sets_flag() {
    let mut map = CollisionMap::new();
    map.block_ground(10, 10);
    assert_ne!(map.flags[10][10] & CollisionFlag::WR_GRND, 0);
}

#[test]
fn reset_flags_bounds_and_open() {
    let map = CollisionMap::new();
    // the 104x104 build area edge is _BOUNDS, the interior _OPEN
    assert_eq!(map.flags[0][0] & CollisionFlag::_BOUNDS, CollisionFlag::_BOUNDS);
    assert_eq!(map.flags[103][103] & CollisionFlag::_BOUNDS, CollisionFlag::_BOUNDS);
    assert_eq!(map.flags[50][50], CollisionFlag::_OPEN);
}

#[test]
fn unblock_ground_clears_flag() {
    let mut map = CollisionMap::new();
    map.block_ground(10, 10);
    map.unblock_ground(10, 10);
    assert_eq!(map.flags[10][10] & CollisionFlag::WR_GRND, 0);
}

#[test]
fn add_loc_blocks_walk_scenery() {
    let mut map = CollisionMap::new();
    map.add_loc(10, 10, 2, 1, LocAngle::NORTH, true);
    // NORTH swaps sizeX/sizeZ: 2x1 becomes 1x2 over tiles (10,10)-(10,11)
    assert_ne!(map.flags[10][10] & CollisionFlag::WALK_SCENERY, 0);
    assert_ne!(map.flags[10][11] & CollisionFlag::WALK_SCENERY, 0);
    assert_ne!(map.flags[10][10] & CollisionFlag::VIS_SCENERY, 0);
    assert_eq!(map.flags[11][10] & CollisionFlag::WALK_SCENERY, 0);
}

#[test]
fn del_loc_restores_open() {
    let mut map = CollisionMap::new();
    map.add_loc(10, 10, 1, 1, LocAngle::WEST, false);
    map.del_loc(10, 10, 1, 1, LocAngle::WEST, false);
    assert_eq!(map.flags[10][10] & CollisionFlag::WALK_SCENERY, 0);
}

#[test]
fn add_wall_blocks_east_neighbour() {
    let mut map = CollisionMap::new();
    // WALL_STRAIGHT at (10,10) facing WEST blocks the tile and the east tile
    map.add_wall(10, 10, LocShape::WALL_STRAIGHT, LocAngle::WEST, false);
    assert_ne!(map.flags[10][10] & CollisionFlag::W_W, 0);
    assert_ne!(map.flags[9][10] & CollisionFlag::W_E, 0);
    // a straight west wall does not fill the full walk-block mask
    assert_eq!(map.flags[10][10] & CollisionFlag::WALK_SCENERY, 0);
}

#[test]
fn test_wall_matches_ts_shape_tests() {
    let mut map = CollisionMap::new();
    map.add_wall(10, 10, LocShape::WALL_STRAIGHT, LocAngle::WEST, false);
    // standing on the wall tile approaches it directly
    assert!(map.test_wall(10, 10, 10, 10, LocShape::WALL_STRAIGHT, LocAngle::WEST));
    assert!(map.test_wall(9, 10, 10, 10, LocShape::WALL_STRAIGHT, LocAngle::WEST));
    // walking over the open east side of the wall tile is still free
    map.add_wall(20, 10, LocShape::WALL_STRAIGHT, LocAngle::NORTH, false);
    assert!(map.test_wall(19, 10, 20, 10, LocShape::WALL_STRAIGHT, LocAngle::NORTH));
}

#[test]
fn test_loc_approaches_from_open_side() {
    let mut map = CollisionMap::new();
    map.add_loc(10, 10, 2, 2, LocAngle::WEST, false);
    // inside the loc is an immediate approach
    assert!(map.test_loc(10, 10, 10, 10, 2, 2, 0));
    // west of the loc with W_E free on the source tile; the forceapproach
    // bit must be unset for the side to be approachable (TS semantics)
    assert!(map.test_loc(9, 10, 10, 10, 2, 2, 0));
    // a forceapproach from the east blocks that side
    assert!(!map.test_loc(12, 10, 10, 10, 2, 2, DirectionFlag::EAST));
}

#[test]
fn walk_route_avoids_blocked_tile() {
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c.out.random = Some(client::io::Isaac::new(&[1, 2, 3, 4]));
    // block the direct diagonal crossing tile (6,6); the BFS must detour
    c.collision[0].block_ground(6, 6);
    assert!(c.tryMove(5, 5, 7, 7, true, 0, 0, 0, 0, 0, 0));
    assert!(c.out.pos > 0);
}

#[test]
fn client_player_at_and_route_arrays() {
    let mut p = ClientPlayer::at(3, 4);
    assert_eq!(p.route_x[0], 3);
    assert_eq!(p.route_z[0], 4);
    // teleport without jumping queues the tile onto the route
    p.teleport(&Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
    .cache, false, 5, 6);
    assert_eq!(p.route_x[0], 5);
    assert_eq!(p.route_z[0], 6);
}

#[test]
fn client_npc_route_readable() {
    let n = ClientNpc::at(8, 9);
    assert_eq!(n.route_x[0], 8);
    assert_eq!(n.route_z[0], 9);
}

#[test]
fn spot_recolour_guard_matches_ts() {
    use client::config::{Cache, SpotType};
    use client::dash3d::Model;

    // a one-face model whose face colour is 0x1122 (hand-crafted to avoid
    // needing OnDemand: 3 zero-delta vertices, face (0,1,2), colour 0x1122,
    // followed by the 18-byte trailer)
    const MODEL: &[u8] = &[
        7, 7, 7, // vertex order: x+y+z present for each of 3 vertices
        1, // face index order: 1 (a,b,c are deltas)
        0, 1, 2, // face index deltas: a=0, b=1, c=2
        0x11, 0x22, // face colour
        0, 0, 0, // vertexX deltas
        0, 0, 0, // vertexY deltas
        0, 0, 0, // vertexZ deltas
        0, 3, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 3, 0, 3, 0, 3, // trailer
    ];
    Model::unpack(0, Some(MODEL));

    // recol_s[0] == 0: the TS (`SpotType.ts` line 91) and Java 274 guard
    // skip the whole loop, so the face colour is untouched
    let mut spot = SpotType::default();
    spot.id = 71;
    spot.model = 0;
    spot.recol_s[1] = 0x1122;
    spot.recol_d[1] = 0x9999;
    let model = spot.get_temp_model2(&Cache::default()).unwrap();
    assert_eq!(model.face_colour.as_ref().unwrap()[0], 0x1122);

    // recol_s[0] != 0: all six pairs apply, so 0x1122 recolours to 0x9999
    let mut spot2 = SpotType::default();
    spot2.id = 72;
    spot2.model = 0;
    spot2.recol_s[0] = 0x1122;
    spot2.recol_d[0] = 0x9999;
    let model2 = spot2.get_temp_model2(&Cache::default()).unwrap();
    assert_eq!(model2.face_colour.as_ref().unwrap()[0], 0x9999);
}
