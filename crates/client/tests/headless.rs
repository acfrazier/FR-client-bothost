//! Task 8: the headless proof.
//!
//! A `Client` with no `Renderer`, no `Present`, no `RenderBackend` and no
//! wgpu runs the whole sim machine end-to-end: `login` over a stub socket,
//! `REBUILD_NORMAL`, a synthetic scene build, `UPDATE_INV_FULL`, several
//! `mainloop` passes, and the sim reads (`tryMove`, `doAction`, typecode
//! queries) — without ever constructing a scene mesh or a GPU device.
//!
//! The honest mechanism is three process-wide construction counters:
//! `Renderer::constructed` (any renderer), `GpuBackend::tried` (any wgpu
//! device init) and `ModelStore::decode_count` (any model geometry decoded
//! from the packed source). This file is its own test binary, so no other
//! test in this process can construct a renderer and trip the assertions.

use std::io::{Read, Write};
use std::sync::Arc;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use client::client::{Client, ClientConfig, MiniMenuAction};
use client::config::IfType;
use client::config::LocType;
use client::dash3d::store::ModelStore;
use client::dash3d::ClientEntity;
use client::io::{Packet, ServerProt};
use client::render::backend::GpuBackend;
use client::render::Renderer;

/// An empty cache dir (no pack): `Client::new` yields `Cache::default()`
/// with `error_loading` false, like the other no-pack tests.
fn cache_dir() -> String {
    std::env::temp_dir()
        .join(format!("274-headless-{}", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

/// The 274 cold-login handshake (probe 14, 8 junk bytes, seed grant,
/// loginout, response 2), then the socket stays open and the server drains
/// the client's in-game writes (`NO_TIMEOUT`, `MAP_BUILD_COMPLETE`, ...)
/// until `close` fires, so `mainloop` sees a live connection.
fn stub_server(listener: TcpListener, close: mpsc::Receiver<()>) {
    let (mut s, _) = listener.accept().unwrap();
    let mut hdr = [0u8; 2];
    s.read_exact(&mut hdr).unwrap();
    assert_eq!(hdr[0], 14); // login server probe
    for _ in 0..8 {
        let _ = s.write_all(&[0]);
    }
    s.write_all(&[0]).unwrap(); // response 0 → send seed
    s.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap(); // g8 seed
    let mut buf = [0u8; 512];
    let n = s.read(&mut buf).unwrap();
    assert!(n > 0);
    assert_eq!(buf[0], 16); // cold login
    s.write_all(&[2, 0, 0]).unwrap(); // response 2, staff=0, mouseTrack=0

    s.set_nonblocking(true).unwrap();
    let mut junk = [0u8; 4096];
    loop {
        if close.try_recv().is_ok() {
            return;
        }
        match s.read(&mut junk) {
            Ok(0) | Err(_) if close.try_recv().is_ok() => return,
            Ok(_) => {} // drained
            Err(_) => thread::sleep(Duration::from_millis(2)),
        }
    }
}

/// One 64×64×4 map square of opcode-0 tiles: level-0 heights fall back to
/// the perlin terrain, deeper levels step down 240 per level.
fn perlin_ground() -> Vec<u8> {
    let mut map = Packet::alloc(2);
    for _level in 0..4 {
        for _x in 0..64 {
            for _z in 0..64 {
                map.p1(0);
            }
        }
    }
    map.data().to_vec()
}

/// A two-loc `.loc` stream for the centre square (region 6,6 → x_offset 32
/// with the zone-50 base 352): loc 0 a wall at raw (8,8) → stx/stz 40, loc
/// 1 a centrepiece (shape 10) at raw (10,10) → stx/stz 42. Both `anim` locs
/// place without a model table, and the centrepiece is non-shadow so the
/// documented sim-side shadow-radius decode stays off.
fn loc_stream() -> Vec<u8> {
    vec![
        0x01, 0x82, 0x09, 0x00, 0x00, // deltaId 1 → loc 0 at raw (8,8); info 0 = wall
        0x01, 0x82, 0x8b, 0x28, 0x00, 0x00, // deltaId 1 → loc 1 at raw (10,10); info 0x28 = centrepiece
    ]
}

/// The typecode `addLoc` computes for a placement: `x + (z<<7) + (id<<14)
/// + 0x40000000` on the offset (stx/stz) coords.
fn typecode(x: i32, z: i32, loc_id: i32) -> i32 {
    0x4000_0000 + x + (z << 7) + (loc_id << 14)
}

#[test]
fn headless_client_core_runs_sim_without_renderer() {
    // A pure `Client` sim machine: no Renderer, no Present, no backend.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<()>();
    let server = thread::spawn(move || stub_server(listener, rx));

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: cache_dir(),
        members: true,
        lowmem: false,
    });
    c.login("bot", "pw", false).unwrap();
    assert!(c.ingame);
    // a real local body for the movement pass and the walk read
    let local = c.local_player.as_mut().unwrap();
    local.ready = true;
    local.entity = ClientEntity::at(10, 10);
    local.entity.teleport(&c.cache, true, 10, 10);

    // Packet apply: REBUILD_NORMAL (zone 50,50) arms the scene build.
    let mut payload = Packet::alloc(0);
    payload.p2(50);
    payload.p2(50);
    payload.pos = 0;
    c.handle_packet(ServerProt::REBUILD_NORMAL, &mut payload);
    assert_eq!(c.scene_state, 1);
    assert_eq!(c.map_build_base_x, (50 - 6) * 8);
    assert_eq!(c.map_build_base_z, (50 - 6) * 8);

    // Synthetic centre square for the headless build: perlin ground + the
    // wall/centrepiece loc stream. File ids -1 so check_scene never waits
    // on the on-demand queue.
    c.map_build_index = vec![(6 << 8) + 6];
    c.map_build_ground_file = vec![-1];
    c.map_build_location_file = vec![-1];
    c.map_build_ground_data = vec![Some(perlin_ground())];
    c.map_build_location_data = vec![Some(loc_stream())];
    c.awaiting_player_info = false;
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
        active: true,
        anim: 0,
        ..LocType::default()
    });
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
        active: true,
        anim: 0,
        shadow: false, // the documented sim-side radius decode stays off
        ..LocType::default()
    });

    // `mainloop` drives `game_loop` → `check_minimap` → `check_scene` →
    // `map_build`: the headless scene build must reach `scene_state = 2`
    // with `draw` off, exactly like a bot slot.
    assert!(!c.draw, "headless default: no pixels");
    for _ in 0..3 {
        c.mainloop();
    }
    assert_eq!(c.scene_state, 2, "game_loop must build the scene headless");

    // Typecode queries resolve from the sim world with no renderer: the
    // wall at (40,40), the centrepiece at (42,42), an empty tile in between.
    let wall_tc = typecode(40, 40, 0);
    assert_eq!(c.world.wall_type(0, 40, 40), wall_tc);
    assert_eq!(c.world.type_code2(0, 40, 40, wall_tc), 0);
    assert_eq!(c.world.type_code2(0, 40, 40, wall_tc + 1), -1);
    let scene_tc = typecode(42, 42, 1);
    assert_eq!(c.world.scene_type(0, 42, 42), scene_tc);
    assert_eq!(c.world.type_code2(0, 42, 42, scene_tc), 10);
    assert_eq!(c.world.wall_type(0, 45, 45), 0, "empty tile stays empty");

    // Packet apply: UPDATE_INV_FULL resolves into the iface's inv arrays.
    c.ifaces.resize(11, None);
    c.ifaces[10] = Some(IfType {
        width: 4,
        height: 4,
        link_obj_type: Some(vec![0; 16]),
        link_obj_number: Some(vec![0; 16]),
        ..IfType::default()
    });
    let mut inv = Packet::alloc(0);
    inv.p2(10); // com id
    inv.p1(2); // slot count
    inv.p2(415); // slot 0: obj id
    inv.p1(3); // slot 0: count
    inv.p2(1522); // slot 1: obj id
    inv.p1(1); // slot 1: count
    inv.pos = 0;
    c.handle_packet(ServerProt::UPDATE_INV_FULL, &mut inv);
    let iface = c.ifaces[10].as_ref().unwrap();
    let types = iface.link_obj_type.as_ref().unwrap();
    let counts = iface.link_obj_number.as_ref().unwrap();
    assert_eq!(types[0], 415);
    assert_eq!(counts[0], 3);
    assert_eq!(types[1], 1522);
    assert_eq!(counts[1], 1);
    assert_eq!(types[15], 0, "slots past the frame stay cleared");

    // Sim reads: doAction(WALK) arms world picking; tryMove encodes a walk.
    c.menu_num_entries = 2;
    c.is_menu_open = true;
    c.menu_action[1] = MiniMenuAction::WALK;
    c.menu_param_b[1] = 100;
    c.menu_param_c[1] = 80;
    c.doAction(1);
    assert!(c.world.click);
    assert_eq!(c.world.click_x, 96);
    assert_eq!(c.world.click_y, 76);
    c.out.pos = 0;
    assert!(
        c.tryMove(10, 10, 12, 10, false, 0, 0, 0, 0, 0, 2),
        "tryMove must route on the sim collision map"
    );
    assert!(c.out.pos > 0, "tryMove must write the MOVE_OPCLICK walk packet");

    // More mainloop passes over the built scene (loc-change, movement,
    // timeout ticks).
    for _ in 0..10 {
        c.mainloop();
    }

    // The proof: nothing render-side was ever constructed or decoded.
    assert_eq!(
        Renderer::constructed(),
        0,
        "no Renderer may exist on the headless path"
    );
    assert_eq!(
        GpuBackend::tried(),
        0,
        "no wgpu device may be created on the headless path"
    );
    assert_eq!(
        ModelStore::decode_count(),
        0,
        "no model geometry may be decoded on the headless path"
    );

    tx.send(()).unwrap();
    server.join().unwrap();
}
