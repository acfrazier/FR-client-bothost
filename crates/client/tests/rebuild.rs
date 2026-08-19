//! Task 16: in-game read loop and REBUILD_NORMAL.
//!
//! `handle_packet` is the inner `ptype` switch, callable from tests without a
//! socket; `ClientBuild::load_ground` decodes a map square into `groundh`.
use client::client::{Client, ClientBuild, ClientConfig};
use client::io::{Packet, ServerProt};

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

#[test]
fn rebuild_normal_same_zone_scene_2_is_ignored() {
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
    build.load_ground(&mut c.groundh, map.data(), 352, 352, 0, 0);
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
    build.load_ground(&mut c.groundh, map.data(), 352, 352, 0, 0);
    assert_eq!(c.groundh[0][10][20], -7 * 8);
    assert_eq!(c.groundh[1][10][20], c.groundh[0][10][20] - 7 * 8);
    assert_eq!(c.groundh[2][10][20], c.groundh[1][10][20] - 7 * 8);
    assert_eq!(c.groundh[3][10][20], c.groundh[2][10][20] - 7 * 8);
}

/// Loopback `tcp_in`: Isaac-encode `REBUILD_NORMAL` on a listener, login,
/// then read the framed packet off the socket and assert bases.
#[test]
fn tcp_in_rebuild_normal_over_socket() {
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
