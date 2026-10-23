// Task 2: movePlayers / routeMove. The movement pass interpolates entity
// x/z along the queued route (Java `Client.java` 7559 / 10547 / 10639)
// before `followCamera` (9580), so the 3D viewport and the minimap follow
// the walk instead of only the debug tile line.
use client::client::{Client, ClientConfig};
use client::config::SeqType;
use client::dash3d::{ClientEntity, ClientNpc, ClientPlayer};
use client::render::Renderer;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

// SAFETY: This wrapper forwards every layout and pointer unchanged to
// `System`; it only updates a thread-local counter before each allocation.
unsafe impl GlobalAlloc for Counting {
    // SAFETY: The trait caller supplies a valid `Layout`, as required by
    // `GlobalAlloc::alloc`.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|count| {
            if let Some(n) = count.get() {
                count.set(Some(n + 1));
            }
        });
        // SAFETY: The caller's valid layout is forwarded unchanged to the
        // system allocator, which satisfies the same allocation contract.
        unsafe { System.alloc(layout) }
    }

    // SAFETY: The trait caller supplies a live pointer allocated by this
    // allocator and its original layout, as required by `GlobalAlloc`.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: The pointer came from `System` through this wrapper, and
        // both it and its layout are forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

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
/// `follow_camera` now lives on the renderer (`mainredraw` runs it ahead
/// of `game_draw`), so the test calls it explicitly.
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
    Renderer::follow_camera(&mut c);
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
    c.npc[3] = Some(Box::new(npc));
    c.loop_cycle = 1;
    c.game_loop();
    let npc = c.npc[3].as_ref().unwrap();
    assert!(npc.x > 20 * 128 + 64, "npc walk must advance x");
    assert!(npc.x <= 21 * 128 + 64);
}

/// Java `ClientPlayer.setAppearance` (274 `ClientPlayer.java` 111-138)
/// reads the fifth movement sequence into `walkanim_r` and the sixth into
/// `walkanim_l`; `routeMove` (`Client.java` 10690-10697) then plays
/// `walkanim_l` for a yaw delta in [256, 768) and `walkanim_r` for
/// [-768, -256]. A player walking sideways relative to its facing (the
/// usual case while it faces a combat target) must therefore step with
/// the sixth sequence one way and the fifth the other, never swapped.
#[test]
fn sidestep_walk_uses_java_appearance_slots() {
    use client::io::Packet;
    let c = client();
    let mut a = vec![0u8, 0]; // gender, headicons
    a.extend([0u8; 12]); // empty body parts
    a.extend([0u8; 5]); // colours
    for seq in 801u16..=807 {
        a.extend(seq.to_be_bytes()); // ready, turn, walk, back, 5th, 6th, run
    }
    a.extend(1u64.to_be_bytes());
    a.push(3);
    a.extend(0u16.to_be_bytes());
    let mut player = ClientPlayer::default();
    player.set_appearance(&mut Packet::new(a), &c.cache);

    let step = |dest_x: i32| {
        let mut e = player.entity.clone();
        e.x = 5 * 128 + 64;
        e.z = 5 * 128 + 64;
        e.yaw = 1024;
        e.route_x = vec![dest_x, 5, 0, 0, 0, 0, 0, 0, 0, 0];
        e.route_z = vec![5, 5, 0, 0, 0, 0, 0, 0, 0, 0];
        e.route_length = 1;
        e.route_move(&c.cache);
        e.secondary_anim
    };
    // East: dstYaw 1536, delta +512 -> walkanim_l (sixth value).
    assert_eq!(step(6), 806);
    // West: dstYaw 512, delta -512 -> walkanim_r (fifth value).
    assert_eq!(step(4), 805);
}

fn movement_entity(tile_x: i32, tile_z: i32, walking: bool) -> ClientEntity {
    let mut entity = ClientEntity::at(tile_x, tile_z);
    entity.x = tile_x * 128 + 64;
    entity.z = tile_z * 128 + 64;
    if walking {
        entity.route_x[0] = tile_x + 1;
        entity.route_z[0] = tile_z;
        entity.route_x[1] = tile_x;
        entity.route_z[1] = tile_z;
        entity.route_length = 1;
    }
    entity.readyanim = 0;
    entity.walkanim = 0;
    entity.primary_anim = 0;
    entity.chat_message = Some("movement pass chat".to_owned());
    entity.chat_timer = 100;
    entity
}

/// A warmed movement pass must not clone per-entity routes or overhead chat.
/// Self-targeting, the `self_slot` alias, and player/NPC update order retain
/// the positions the original detached-clone pass observed.
#[test]
fn movement_passes_do_not_allocate_per_entity() {
    let mut c = client();
    {
        let cache = std::sync::Arc::get_mut(&mut c.cache).expect("client owns its cache");
        cache.seqs.resize_with(1, SeqType::default);
        cache.seqs[0] = SeqType {
            num_frames: 2,
            frames: Some(vec![0, 1]),
            delay: Some(vec![2, 2]),
            ..SeqType::default()
        };
    }
    c.loop_cycle = 1;
    c.self_slot = 500;

    let mut local = ClientPlayer {
        entity: movement_entity(20, 20, true),
        ..ClientPlayer::default()
    };
    local.face_entity = 32768 + c.self_slot;
    c.local_player = Some(local);

    c.player_count = 128;
    for index in 0..128 {
        c.player_ids[index] = index as i32;
        let (tile_x, tile_z, walking) = match index {
            0 => (40, 40, true),
            1 => (30, 30, false),
            2 => (40, 40, false),
            _ => (30 + (index % 12) as i32, 30 + (index / 12) as i32, true),
        };
        let mut player = ClientPlayer {
            entity: movement_entity(tile_x, tile_z, walking),
            ..ClientPlayer::default()
        };
        if index == 0 {
            player.face_entity = 32768;
        } else if index == 1 {
            player.face_entity = 32768 + c.self_slot;
        } else if index == 2 {
            player.face_entity = 32768;
        }
        c.players[index] = Some(Box::new(player));
    }

    c.npc_count = 32;
    for index in 0..32 {
        c.npc_ids[index] = index as i32;
        let (tile_x, tile_z, walking) = match index {
            0 => (50, 50, true),
            1 => (50, 50, false),
            _ => (55 + (index % 8) as i32, 55 + (index / 8) as i32, true),
        };
        let mut npc = ClientNpc {
            entity: movement_entity(tile_x, tile_z, walking),
            ..ClientNpc::default()
        };
        if index < 2 {
            npc.face_entity = 0;
        }
        c.npc[index] = Some(Box::new(npc));
    }

    // Warm the paths before measuring one pass of each movement function.
    c.move_players();
    c.move_npcs();

    COUNT.with(|count| count.set(Some(0)));
    std::hint::black_box(&mut c).move_players();
    let player_allocations = COUNT.with(|count| count.replace(None).unwrap());

    COUNT.with(|count| count.set(Some(0)));
    std::hint::black_box(&mut c).move_npcs();
    let npc_allocations = COUNT.with(|count| count.replace(None).unwrap());

    assert_eq!(
        player_allocations, 0,
        "move_players must not allocate while moving the local player and 128 players"
    );
    assert_eq!(
        npc_allocations, 0,
        "move_npcs must not allocate while moving 32 NPCs"
    );

    let local = c.local_player.as_ref().unwrap();
    assert_eq!(local.x, 20 * 128 + 64 + 8);
    assert_eq!(local.yaw, 1984, "the local player's self alias is itself");
    assert_eq!(local.primary_anim, 0);
    assert_eq!(local.secondary_anim, 0);
    assert_eq!(local.chat_message.as_deref(), Some("movement pass chat"));

    let self_facing_player = c.players[0].as_ref().unwrap();
    assert_eq!(self_facing_player.x, 40 * 128 + 64 + 8);
    assert_eq!(
        self_facing_player.yaw, 64,
        "self-target reads its old position"
    );
    assert_eq!(self_facing_player.primary_anim, 0);
    assert_eq!(self_facing_player.secondary_anim, 0);
    assert_eq!(
        self_facing_player.chat_message.as_deref(),
        Some("movement pass chat")
    );
    assert_eq!(
        c.players[1].as_ref().unwrap().yaw,
        64,
        "self_slot resolves to the already-moved local player"
    );
    assert_eq!(
        c.players[2].as_ref().unwrap().yaw,
        1984,
        "player targets see earlier players after their movement"
    );

    let self_facing_npc = c.npc[0].as_ref().unwrap();
    assert_eq!(self_facing_npc.x, 50 * 128 + 64 + 8);
    assert_eq!(
        self_facing_npc.yaw, 64,
        "NPC self-target reads its old position"
    );
    assert_eq!(self_facing_npc.primary_anim, 0);
    assert_eq!(self_facing_npc.secondary_anim, 0);
    assert_eq!(
        self_facing_npc.chat_message.as_deref(),
        Some("movement pass chat")
    );
    assert_eq!(
        c.npc[1].as_ref().unwrap().yaw,
        1984,
        "NPC targets see earlier NPCs after their movement"
    );
}
