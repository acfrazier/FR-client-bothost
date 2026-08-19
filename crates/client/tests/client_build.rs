// `LocType.checkModelAll` + `ClientBuild.checkLocations`/`prefetchLocations`
// (client-ts `LocType.ts` 296-323, `ClientBuild.ts` 638-715), and
// `loadLocations`/`addLoc` (TS 718-1137). Model ids in these tests are chosen
// far outside anything a real pack could load, so `Model::requestDownload`
// can never see them as ready; planted locs have `model: None` so `getModel`
// yields nothing and only the collision side-effects are observable.
use client::client::{Client, ClientBuild, ClientConfig};
use client::config::{Cache, LocType};
use client::dash3d::{BuildArea, CollisionFlag, MapFlag};
use client::io::{OnDemand, Packet};

#[test]
fn check_model_all_none_models_is_ready() {
    let loc = LocType::default();
    assert!(loc.check_model_all());
}

#[test]
fn check_model_all_missing_model_is_not_ready() {
    let loc = LocType {
        model: Some(vec![60000]),
        ..LocType::default()
    };
    assert!(!loc.check_model_all());
}

#[test]
fn check_locations_empty_packet_is_ready() {
    let cache = Cache::default();
    // gsmart 0 ends the loc-id loop immediately
    assert!(ClientBuild::new().check_locations(&cache, &[0u8], 0, 0));
}

#[test]
fn check_locations_requests_missing_models_in_area() {
    let mut cache = Cache::default();
    cache.locs.push(LocType {
        model: Some(vec![60000]),
        ..LocType::default()
    });
    let build = ClientBuild::new();
    // deltaId 1 (loc 0), deltaPos 1 (tile 0,0), shape 24, end pos, end id
    let src = [0x01, 0x01, 0x60, 0x00, 0x00];
    assert!(!build.check_locations(&cache, &src, 1, 1));
}

#[test]
fn check_locations_skips_ground_decor_in_low_mem() {
    let mut cache = Cache::default();
    cache.locs.push(LocType {
        model: Some(vec![60000]),
        ..LocType::default()
    });
    let build = ClientBuild::new();
    // shape 22 (0x58 = 88, 88 >> 2): ground decor, skipped in low mem
    let src = [0x01, 0x01, 0x58, 0x00, 0x00];
    assert!(build.check_locations(&cache, &src, 1, 1));
}

#[test]
fn check_locations_checks_ground_decor_in_high_mem() {
    let mut cache = Cache::default();
    cache.locs.push(LocType {
        model: Some(vec![60000]),
        ..LocType::default()
    });
    let mut build = ClientBuild::new();
    build.low_mem = false;
    let src = [0x01, 0x01, 0x58, 0x00, 0x00];
    assert!(!build.check_locations(&cache, &src, 1, 1));
}

#[test]
fn prefetch_locations_empty_packet_is_noop() {
    let cache = Cache::default();
    let mut od = OnDemand::new_unconnected();
    ClientBuild::prefetch_locations(&cache, &mut Packet::new(vec![0]), &mut od);
    assert_eq!(od.remaining(), 0);
}

#[test]
fn prefetch_locations_decodes_loc_id_loop() {
    let mut cache = Cache::default();
    cache.locs.push(LocType {
        model: Some(vec![1, -1, 2]),
        ..LocType::default()
    });
    let mut od = OnDemand::new_unconnected();
    // deltaId 1 (loc 0), one pos entry, end pos, end id
    ClientBuild::prefetch_locations(
        &cache,
        &mut Packet::new(vec![0x01, 0x01, 0x00, 0x00, 0x00]),
        &mut od,
    );
    assert_eq!(od.remaining(), 0);
}

// --- loadLocations / addLoc (TS 718-1137) ---

/// A fresh client off the empty /tmp cache: no packs, so `Cache::default()`
/// and zeroed `groundh`/`mapl`, like the hud tests.
fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// gsmart byte layout (Packet.ts `gsmart`): values 1..127 are one byte,
/// values 128.. are `g2() - 0x8000`. `deltaPos = locPos + 1` (TS 735-737).
#[test]
fn load_locations_empty_is_noop() {
    let mut c = client();
    let mut build = ClientBuild::new();
    // gsmart 0 ends the loc-id loop immediately
    build.load_locations(
        &c.cache,
        &mut c.world,
        &mut c.collision,
        &c.groundh,
        &c.mapl,
        &[0],
        0,
        0,
        0,
    );
    for level in 0..BuildArea::LEVELS {
        for x in 0..BuildArea::SIZE {
            for z in 0..BuildArea::SIZE {
                assert!(c.world.get_wall(level, x, z).is_none(), "wall at {level},{x},{z}");
            }
        }
    }
}

#[test]
fn load_locations_ground_decor_blocks_when_active() {
    let mut c = client();
    c.cache.locs.push(LocType {
        active: true,
        blockwalk: true,
        forcedecor: true,
        ..LocType::default()
    });
    let mut build = ClientBuild::new();
    // deltaId 1 (loc 0); deltaPos 131 -> locPos 130 (x=2, z=2, level=0);
    // info 0x58 -> shape 22 (GROUND_DECOR), rotation 0; end pos; end id
    let src = [0x01, 0x80, 0x83, 0x58, 0x00, 0x00];
    build.load_locations(
        &c.cache,
        &mut c.world,
        &mut c.collision,
        &c.groundh,
        &c.mapl,
        &src,
        0,
        0,
        0,
    );
    // active+blockwalk GROUND_DECOR: collision.blockGround even with no model
    // (TS 803-805)
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::WR_GRND, 0);
    // no scenery was placed (model None; low-mem active gate passed)
    assert!(c.world.get_wall(0, 2, 2).is_none());
    assert!(c.world.get_gd(0, 2, 2).is_none());
}

#[test]
fn load_locations_low_mem_skips_force_high_detail() {
    let mut c = client();
    c.cache.locs.push(LocType {
        active: true,
        blockwalk: true,
        ..LocType::default()
    });
    c.mapl[0][2][2] = MapFlag::FORCE_HIGH_DETAIL as u8;
    let mut build = ClientBuild::new();
    let src = [0x01, 0x80, 0x83, 0x58, 0x00, 0x00];
    build.load_locations(
        &c.cache,
        &mut c.world,
        &mut c.collision,
        &c.groundh,
        &c.mapl,
        &src,
        0,
        0,
        0,
    );
    // low_mem default true: ForceHighDetail tiles are skipped before any
    // collision work (TS 767-769)
    assert_eq!(c.collision[0].flags[2][2] & CollisionFlag::WR_GRND, 0);
}

#[test]
fn load_locations_low_mem_skips_wrong_vis_below() {
    let mut c = client();
    c.cache.locs.push(LocType {
        active: true,
        blockwalk: true,
        ..LocType::default()
    });
    let mut build = ClientBuild::new();
    build.minusedlevel = 1;
    let src = [0x01, 0x80, 0x83, 0x58, 0x00, 0x00];
    build.load_locations(
        &c.cache,
        &mut c.world,
        &mut c.collision,
        &c.groundh,
        &c.mapl,
        &src,
        0,
        0,
        0,
    );
    // getVisBelowLevel(0, 2, 2) is 0, not minusedlevel: skipped (TS 770-773)
    assert_eq!(c.collision[0].flags[2][2] & CollisionFlag::WR_GRND, 0);
}

#[test]
fn load_locations_wall_blocks_collision_without_model() {
    let mut c = client();
    c.cache.locs.push(LocType {
        blockwalk: true,
        ..LocType::default()
    });
    let mut build = ClientBuild::new();
    // info 0x00 -> shape 0 (WALL_STRAIGHT), rotation 0 (WEST)
    let src = [0x01, 0x80, 0x83, 0x00, 0x00, 0x00];
    build.load_locations(
        &c.cache,
        &mut c.world,
        &mut c.collision,
        &c.groundh,
        &c.mapl,
        &src,
        0,
        0,
        0,
    );
    // setWall no-ops on a missing model, but the wall still blocks walking
    // (TS 924-928)
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::W_W, 0);
    assert!(c.world.get_wall(0, 2, 2).is_none());
}

#[test]
fn load_locations_skips_out_of_area_tiles() {
    let mut c = client();
    c.cache.locs.push(LocType {
        blockwalk: true,
        ..LocType::default()
    });
    let mut build = ClientBuild::new();
    // locPos 2 -> x=0, z=2, level=0: stx=0 lies outside the 1..103 build
    // area, so the bytes are consumed but no loc is placed (TS 748)
    let src = [0x01, 0x03, 0x00, 0x00, 0x00];
    build.load_locations(
        &c.cache,
        &mut c.world,
        &mut c.collision,
        &c.groundh,
        &c.mapl,
        &src,
        0,
        0,
        0,
    );
    // the out-of-area wall would have stamped (0,2) and panicked on its
    // negative neighbour; interior tiles stay untouched
    assert_eq!(c.collision[0].flags[1][2] & CollisionFlag::W_W, 0);
}
