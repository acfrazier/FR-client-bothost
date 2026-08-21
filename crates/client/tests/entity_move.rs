// Task 2: movePlayers / routeMove. The movement pass interpolates entity
// x/z along the queued route (Java `Client.java` 7559 / 10547 / 10639)
// before `followCamera` (9580), so the 3D viewport and the minimap follow
// the walk instead of only the debug tile line.
use client::client::{Client, ClientConfig};
use client::dash3d::{ClientEntity, ClientNpc, ClientPlayer};

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// Java `routeMove`: step x/z toward `route[routeLength - 1]` (the head of
/// the queued walk) at speed 4, or 2 while the yaw is turning, so a queued
/// one-tile walk advances toward the destination tile.
#[test]
fn route_move_advances_x_toward_queued_tile() {
    let c = client();
    let mut e = ClientEntity {
        x: 5 * 128 + 64,
        z: 5 * 128 + 64,
        route_x: vec![5, 6, 0, 0, 0, 0, 0, 0, 0, 0],
        route_z: vec![5, 5, 0, 0, 0, 0, 0, 0, 0, 0],
        route_length: 2,
        ..ClientEntity::default()
    };
    e.route_move(&c.cache);
    assert!(e.x > 5 * 128 + 64, "must step toward dest");
    assert!(e.x <= 6 * 128 + 64);
    assert_eq!(e.route_length, 2, "not arrived: dest is one tile east");
}

/// `game_loop` runs the move pass (Java 9466-9467) before `followCamera`
/// (9580): a queued walk advances `local_player.x/z` on the same pass the
/// orbit camera snaps to it, so the 3D view pans with the player. The
/// player sits at tile (20,20) — inside the 1536..11776 local bounds — so
/// `move_entity` walks instead of snapping back to the route head.
#[test]
fn game_loop_walk_moves_local_player_and_orbit_camera() {
    let mut c = client();
    c.ingame = true;
    c.scene_state = 2;
    let mut player = ClientPlayer {
        ready: true,
        name: Some("tester".into()),
        ..ClientPlayer::default()
    };
    player.entity.teleport(&c.cache, true, 20, 20);
    player.entity.teleport(&c.cache, false, 21, 20); // queue a walk east
    c.local_player = Some(player);
    c.loop_cycle = 1; // > 0 so the default exact-move fields fall to route_move
    c.game_loop();
    let p = c.local_player.as_ref().unwrap();
    assert!(p.x > 20 * 128 + 64, "walk must advance x from 2624");
    assert!(p.x <= 21 * 128 + 64);
    assert_eq!(c.orbit_camera_x, p.x, "orbit camera snaps to the walked x");
    assert_eq!(c.orbit_camera_z, p.z, "orbit camera snaps to the walked z");
}

/// `moveNpcs` interpolates tracked NPCs the same way as players.
#[test]
fn move_npcs_steps_tracked_npc() {
    let mut c = client();
    c.ingame = true;
    c.npc_count = 1;
    c.npc_ids[0] = 3;
    let mut npc = ClientNpc {
        entity: ClientEntity::at(20, 20),
        ..ClientNpc::default()
    };
    npc.entity.teleport(&c.cache, true, 20, 20);
    npc.entity.teleport(&c.cache, false, 21, 20);
    c.npc[3] = Some(npc);
    c.loop_cycle = 1;
    c.game_loop();
    let npc = c.npc[3].as_ref().unwrap();
    assert!(npc.x > 20 * 128 + 64, "npc walk must advance x");
    assert!(npc.x <= 21 * 128 + 64);
}
