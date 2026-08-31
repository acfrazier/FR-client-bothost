//! Task 16: in-game read loop and REBUILD_NORMAL.
//!
//! `handle_packet` is the inner `ptype` switch, callable from tests without a
//! socket; `ClientBuild::load_ground` decodes a map square into `groundh`.
use client::client::{Client, ClientBuild, ClientConfig, ClientPlayer};
use client::graphics::PixMap;
use client::io::{ClientProt, Packet, ServerProt};
use client::render::Renderer;

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
fn rebuild_normal_sets_base() {
    let _r = Renderer::new(false);
    let mut c = client();
    c.ingame = true;
    // zoneX=50, zoneZ=50 -> base = (zone - 6) * 8. Client.ts REBUILD_NORMAL:
    // `this.mapBuildBaseX = (this.mapBuildCentreZoneX - 6) * 8` (also Java 274
    // Client.java `mapBuildBaseX = (mapBuildCenterZoneX - 6) * 8`).
    let mut payload = Packet::alloc(0);
    payload.p2(50);
    payload.p2(50);
    payload.pos = 0;
    c.handle_packet(ServerProt::REBUILD_NORMAL, &mut payload);
    assert_eq!(c.map_build_base_x, (50 - 6) * 8);
    assert_eq!(c.map_build_base_z, (50 - 6) * 8);
    assert_eq!(c.map_build_centre_zone_x, 50);
    assert_eq!(c.map_build_centre_zone_z, 50);
    assert_eq!(c.scene_state, 1);
}

/// Java `localPlayer` IS `players[LOCAL_PLAYER_INDEX]`, so REBUILD_NORMAL's
/// entity shift also moves the local body with the build origin; the Rust
/// clone must be shifted the same way or NPC_INFO places first-login NPCs
/// relative to an unshifted (0,0) local.
#[test]
fn rebuild_normal_shifts_local_player() {
    let _r = Renderer::new(false);
    let mut c = client();
    c.ingame = true;
    c.map_build_prev_base_x = 0;
    c.map_build_prev_base_z = 0;
    let mut local = ClientPlayer::at(10, 10);
    local.entity.x = 10 * 128;
    local.entity.z = 10 * 128;
    c.local_player = Some(local);
    let mut payload = Packet::alloc(0);
    payload.p2(50); // zone → base (50-6)*8 = 352
    payload.p2(50);
    payload.pos = 0;
    c.handle_packet(ServerProt::REBUILD_NORMAL, &mut payload);
    let local = c.local_player.as_ref().unwrap();
    assert_eq!(local.x, 10 * 128 - 352 * 128);
    assert_eq!(local.z, 10 * 128 - 352 * 128);
    assert_eq!(local.route_x[0], 10 - 352);
    assert_eq!(local.route_z[0], 10 - 352);
}

#[test]
fn rebuild_normal_same_zone_scene_2_is_ignored() {
    let _r = Renderer::new(false);
    let mut c = client();
    c.scene_state = 2;
    c.map_build_centre_zone_x = 50;
    c.map_build_centre_zone_z = 50;
    c.map_build_base_x = 7;
    c.map_build_base_z = 9;
    let mut payload = Packet::alloc(0);
    payload.p2(50);
    payload.p2(50);
    payload.pos = 0;
    c.handle_packet(ServerProt::REBUILD_NORMAL, &mut payload);
    assert_eq!(c.map_build_base_x, 7);
    assert_eq!(c.map_build_base_z, 9);
    assert_eq!(c.scene_state, 2);
}

/// A 64x64x4 map square whose every tile is opcode 0: level 0 heights fall
/// back to the perlin terrain, deeper levels step down 240 per level. Golden
/// values are generated from `ClientBuild.ts` perlinNoise (node run, same
/// cos table).
#[test]
fn load_ground_opcode_zero_uses_perlin_terrain() {
    let _r = Renderer::new(false);
    let mut c = client();
    let mut map = Packet::alloc(2);
    for _level in 0..4 {
        for _x in 0..64 {
            for _z in 0..64 {
                map.p1(0);
            }
        }
    }
    let mut build = ClientBuild::new();
    // origin = zone-50 build base, square offset 0,0 (centre square)
    build.load_ground(&mut c.groundh, &mut c.mapl, map.data(), 352, 352, 0, 0);
    for (stx, stz, height) in [(10, 20, -264), (30, 40, -280), (0, 0, -352), (63, 63, -264)] {
        assert_eq!(c.groundh[0][stx][stz], height, "level 0 tile {stx},{stz}");
        assert_eq!(c.groundh[1][stx][stz], height - 240);
        assert_eq!(c.groundh[2][stx][stz], height - 480);
        assert_eq!(c.groundh[3][stx][stz], height - 720);
    }
}

/// Opcode 1 gives an explicit height (1..=255, `1` read as `0`): level 0 is
/// `-height * 8`, deeper levels step down `-height * 8` from the level below.
#[test]
fn load_ground_opcode_one_sets_explicit_height() {
    let _r = Renderer::new(false);
    let mut c = client();
    let mut map = Packet::alloc(2);
    for _level in 0..4 {
        for x in 0..64 {
            for z in 0..64 {
                if x == 10 && z == 20 {
                    map.p1(1);
                    map.p1(7);
                } else {
                    map.p1(0);
                }
            }
        }
    }
    let mut build = ClientBuild::new();
    build.load_ground(&mut c.groundh, &mut c.mapl, map.data(), 352, 352, 0, 0);
    assert_eq!(c.groundh[0][10][20], -7 * 8);
    assert_eq!(c.groundh[1][10][20], c.groundh[0][10][20] - 7 * 8);
    assert_eq!(c.groundh[2][10][20], c.groundh[1][10][20] - 7 * 8);
    assert_eq!(c.groundh[3][10][20], c.groundh[2][10][20] - 7 * 8);
}

/// `load_ground` zeroes in-area `mapl` tiles before decoding, so a 64×64×4
/// stream of opcode 0 leaves `mapl[0][x][z] == 0`; opcode 50-81 (map-land
/// flags, value `opcode - 49`) write the flag bit.
#[test]
fn load_ground_writes_client_mapl_flags() {
    let _r = Renderer::new(false);
    let mut c = client();
    // opcode 49+1 = 50 → mapl bit (opcode-49) on tile (0,0) level 0 after
    // offsets; a 64×64×4 stream of opcode 0 still leaves mapl[0][x][z]==0
    // for in-area tiles.
    let mut src = vec![0u8; 4 * 64 * 64];
    ClientBuild::new().load_ground(&mut c.groundh, &mut c.mapl, &src, 0, 0, 0, 0);
    assert_eq!(c.mapl[0][0][0], 0);

    // opcode 50 on tile (0,0) level 0 → mapl flag 1. The flag tile consumes
    // an extra opcode byte (50 then 0), shifting the tail by one, so pad
    // the stream with a trailing 0 to keep the last tile in bounds.
    src.push(0);
    src[0] = 50;
    ClientBuild::new().load_ground(&mut c.groundh, &mut c.mapl, &src, 0, 0, 0, 0);
    assert_eq!(c.mapl[0][0][0], 1);
}

/// Loopback `tcp_in`: Isaac-encode `REBUILD_NORMAL` on a listener, login,
/// then read the framed packet off the socket and assert bases.
#[test]
fn tcp_in_rebuild_normal_over_socket() {
    let _r = Renderer::new(false);
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap();
        s.write_all(&[0u8; 8]).unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        s.write_all(&[2, 0, 0]).unwrap();
        let frame = rx.recv().unwrap();
        s.write_all(&frame).unwrap();
        // hold the socket until the client has read
        thread::sleep(Duration::from_millis(200));
    });

    let _r = Renderer::new(false);

    let mut c = client();
    c.config.host = addr.ip().to_string();
    c.config.port = addr.port();
    c.login("bob", "pw", false).unwrap();
    let mut isaac = c.random_in.clone().expect("inbound Isaac after login");
    let opcode = (ServerProt::REBUILD_NORMAL.wrapping_add(isaac.next_int()) & 0xff) as u8;
    let frame = vec![opcode, 0, 50, 0, 50];
    tx.send(frame).unwrap();

    let mut got = false;
    for _ in 0..100 {
        if c.tcp_in() {
            got = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(got, "tcp_in did not consume the framed REBUILD_NORMAL");
    assert_eq!(c.map_build_base_x, (50 - 6) * 8);
    assert_eq!(c.map_build_base_z, (50 - 6) * 8);
    server.join().unwrap();
}

/// Task 8: REBUILD_NORMAL paints the loading splash into `area_game` (and
/// `draw_area` when `draw`) before the map data streams in. Fonts may be
/// missing without a title jag, in which case the splash still cls's the
/// pre-filled `area_game` black and only the text pixels are absent.
#[test]
fn rebuild_normal_paints_loading_splash_when_draw() {
    let _r = Renderer::new(false);
    let mut r = Renderer::new(false);
    let mut c = client();
    c.set_draw(true);
    // fonts may be missing without a title jag; the splash keeps the frozen
    // frame and only overlays "Loading - please wait." when a font exists.
    if r.area_game.is_none() {
        r.area_game = Some(PixMap::new(512, 334));
    }
    // pre-fill so the frozen frame (no cls) and any text overlay can be seen
    r.area_game.as_mut().unwrap().fill(0x123456);
    let mut payload = Packet::alloc(0);
    payload.p2(50);
    payload.p2(50);
    payload.pos = 0;
    c.handle_packet(ServerProt::REBUILD_NORMAL, &mut payload);
    assert_eq!(c.scene_state, 1);
    let ag = r.area_game.as_ref().expect("area_game");
    if r.media.p12.is_none() {
        // no font: the frame stays frozen — no cls, no text overlay
        assert!(
            ag.pixels.iter().all(|&p| p == 0x123456),
            "area_game not frozen"
        );
    } else {
        // font present: the text overwrites the frozen frame
        assert!(ag.pixels.iter().any(|&p| p != 0x123456), "no splash pixels");
    }
}

/// `check_scene` with every map square present (the tutorial-skip pattern:
/// files -1 so nothing is awaited, no data) builds the scene and emits
/// MAP_BUILD_COMPLETE (214). `map_build` writes NO_TIMEOUT frames first, so
/// the completion opcode is the last byte (unencrypted here: no
/// `out.random`).
#[test]
fn check_scene_ready_sets_state_2_and_map_build_complete() {
    let mut c = client();
    c.ingame = true;
    c.awaiting_player_info = false;
    c.scene_state = 1;
    // one region, files -1 so no wait (tutorial skip pattern)
    c.map_build_index = vec![0];
    c.map_build_ground_file = vec![-1];
    c.map_build_location_file = vec![-1];
    c.map_build_ground_data = vec![None];
    c.map_build_location_data = vec![None];
    let status = c.check_scene();
    assert_eq!(status, 0);
    assert_eq!(c.scene_state, 2);
    assert_eq!(
        c.out.data()[c.out.pos - 1],
        ClientProt::MAP_BUILD_COMPLETE.id as u8
    );
}

/// Lowmem same-zone level change (a ladder) re-enters `scene_state = 1`
/// and rebuilds from the parked land/loc bytes. Clearing those after the
/// first `map_build` made `check_scene` return -1000 forever (live hang).
#[test]
fn lowmem_level_change_rebuilds_from_parked_map_bytes() {
    let mut c = client();
    c.config.lowmem = true;
    c.ingame = true;
    c.awaiting_player_info = false;
    c.scene_state = 1;
    c.map_build_index = vec![0];
    c.map_build_ground_file = vec![-1];
    c.map_build_location_file = vec![-1];
    c.map_build_ground_data = vec![None];
    c.map_build_location_data = vec![None];
    assert_eq!(c.check_scene(), 0);
    assert_eq!(c.scene_state, 2);

    c.minusedlevel = 1;
    c.game_loop();
    assert_eq!(
        c.scene_state, 2,
        "lowmem level change must rebuild, not hang in scene_state=1"
    );
}

/// The headless scene build (task-2b fix round 1): `game_loop` runs the
/// sim half of `checkMinimap` unconditionally — `draw=false` must not
/// skip `check_scene` → `map_build`, so a headless slot still emits
/// `MAP_BUILD_COMPLETE` and reaches `scene_state == 2`.
#[test]
fn game_loop_builds_scene_headless_with_draw_off() {
    let mut c = client();
    c.ingame = true;
    c.awaiting_player_info = false;
    c.scene_state = 1;
    assert!(!c.draw, "headless default");
    c.map_build_index = vec![0];
    c.map_build_ground_file = vec![-1];
    c.map_build_location_file = vec![-1];
    c.map_build_ground_data = vec![None];
    c.map_build_location_data = vec![None];
    c.game_loop();
    assert_eq!(
        c.scene_state, 2,
        "game_loop must build the scene with draw=false"
    );
    assert_eq!(
        c.out.data()[c.out.pos - 1],
        ClientProt::MAP_BUILD_COMPLETE.id as u8
    );
}

/// `map_build` must mirror the decoded heights into `World.groundh`: Java
/// hands `World` the one `groundh` array Client writes, so the render pass
/// (`render_quick_ground`, which reads `World.groundh`) sees the same
/// heights the camera reads (`get_av_h` on `Client.groundh`). Without the
/// copy the world reads zeros and the outdoor ground renders a checkerboard.
#[test]
fn world_groundh_matches_client_after_load_ground() {
    let mut c = client();
    c.ingame = true;
    c.awaiting_player_info = false;
    c.scene_state = 1;
    // a 64x64x4 opcode-0 terrain square (the perlin fixture) parked as the
    // loaded ground data for the single requested region; files -1 so
    // check_scene does not wait on the on-demand queue.
    let mut map = Packet::alloc(2);
    for _level in 0..4 {
        for _x in 0..64 {
            for _z in 0..64 {
                map.p1(0);
            }
        }
    }
    c.map_build_index = vec![0];
    c.map_build_ground_file = vec![-1];
    c.map_build_location_file = vec![-1];
    c.map_build_ground_data = vec![Some(map.data().to_vec())];
    c.map_build_location_data = vec![None];
    c.map_build_centre_zone_x = 50;
    c.map_build_centre_zone_z = 50;
    c.map_build_base_x = 0;
    c.map_build_base_z = 0;

    let status = c.check_scene();
    assert_eq!(status, 0);

    // the perlin terrain gives interior level-0 heights well away from 0;
    // the world must read the same heights the camera reads.
    assert_ne!(c.groundh[0][20][20], 0);
    assert_eq!(c.world.groundh_at(0, 20, 20), c.groundh[0][20][20]);
}
