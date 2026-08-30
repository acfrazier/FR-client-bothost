// `LocType.checkModelAll` + `ClientBuild.checkLocations`/`prefetchLocations`
// (client-ts `LocType.ts` 296-323, `ClientBuild.ts` 638-715), and
// `loadLocations`/`addLoc` (TS 718-1137). Model ids in these tests are chosen
// far outside anything a real pack could load, so `Model::requestDownload`
// can never see them as ready; planted locs have `model: None` so `getModel`
// yields nothing and only the collision side-effects are observable. The
// provider test wires a recording `ModelProvider` (as `maininit` does) to
// prove the not-ready path queues archive-0 requests.
//
// The finishBuild tests (TS 75-541) drive real ground data: the src packets
// carry opcode-0 tiles (perlin fallback heights) plus one planted floor
// tile, so the blend pass has something to lay down.
use client::render::{RenderWorld, Renderer};
use std::mem::size_of;
use std::sync::{Arc, Mutex};

use client::client::{Client, ClientBuild, ClientConfig};
use client::config::{Cache, FloType, LocType};
use client::dash3d::model::ModelProvider;
use client::dash3d::{
    BuildArea, CollisionFlag, Ground, LocAngle, MapFlag, Model, SceneModel, TerrainOverlayShape,
};
use client::graphics::Colour;
use client::io::{OnDemand, Packet};

/// Records the model ids `Model.requestDownload` routes to it, standing in
/// for the `OnDemand` handle `maininit` wires via `Model.init`.
struct RecordingProvider {
    requested: Arc<Mutex<Vec<i32>>>,
}

impl ModelProvider for RecordingProvider {
    fn request_model(&mut self, id: i32) {
        self.requested.lock().unwrap().push(id);
    }
}

#[test]
fn check_model_all_requests_through_wired_provider() {
let _r = Renderer::new(false);
    // Tests in this binary share the process-wide `Model` store and may run
    // in parallel, so the assertions tolerate extra 60000 requests from the
    // sibling not-ready tests instead of pinning the exact request vector.
    let requested = Arc::new(Mutex::new(Vec::<i32>::new()));
    Model::init(70000, Box::new(RecordingProvider { requested: requested.clone() }));

    // missing models are not ready and each id is queued to the provider
    let loc = LocType {
        model: Some(vec![60000, 60001]),
        ..LocType::default()
    };
    assert!(!loc.check_model_all());
    {
        let got = requested.lock().unwrap();
        assert!(got.contains(&60000) && got.contains(&60001));
    }

    // unpacked models report ready and are not re-requested (id 5: no
    // other test in this binary unpacks it, so a request for it would
    // prove the ready path really skipped the provider)
    Model::unpack(5, None);
    let loc = LocType {
        model: Some(vec![5]),
        ..LocType::default()
    };
    assert!(loc.check_model_all());
    assert!(!requested.lock().unwrap().contains(&5));

    // request_download routes unknown ids through the provider as well
    assert!(!Model::request_download(60002));
    assert!(requested.lock().unwrap().contains(&60002));
}

#[test]
fn check_model_all_none_models_is_ready() {
let _r = Renderer::new(false);
    let loc = LocType::default();
    assert!(loc.check_model_all());
}

#[test]
fn check_model_all_missing_model_is_not_ready() {
let _r = Renderer::new(false);
    let loc = LocType {
        model: Some(vec![60000]),
        ..LocType::default()
    };
    assert!(!loc.check_model_all());
}

#[test]
fn check_locations_empty_packet_is_ready() {
let _r = Renderer::new(false);
    let cache = Cache::default();
    // gsmart 0 ends the loc-id loop immediately
    assert!(ClientBuild::new().check_locations(&cache, &[0u8], 0, 0));
}

#[test]
fn check_locations_requests_missing_models_in_area() {
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
    let cache = Cache::default();
    let mut od = OnDemand::new_unconnected();
    ClientBuild::prefetch_locations(&cache, &mut Packet::new(vec![0]), &mut od);
    assert_eq!(od.remaining(), 0);
}

#[test]
fn prefetch_locations_decodes_loc_id_loop() {
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
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
let _r = Renderer::new(false);
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
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
let _r = Renderer::new(false);
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
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
let _r = Renderer::new(false);
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
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
let _r = Renderer::new(false);
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
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
let _r = Renderer::new(false);
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
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

#[test]
fn load_locations_places_at_offset_tiles() {
let _r = Renderer::new(false);
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
        blockwalk: true,
        ..LocType::default()
    });
    let mut build = ClientBuild::new();
    // deltaId 1 (loc 0); deltaPos 131 -> locPos 130 (raw x=2, z=2); info
    // 0x00 -> shape 0 (WALL_STRAIGHT), rotation 0 (WEST)
    let src = [0x01, 0x80, 0x83, 0x00, 0x00, 0x00];
    // stx = x + 8 = 10: addLoc must receive the offset tile coords, not the
    // raw packet x/z (TS `this.addLoc(level, stx, stz, ...)` at 758)
    build.load_locations(
        &c.cache,
        &mut c.world,
        &mut c.collision,
        &c.groundh,
        &c.mapl,
        &src,
        8,
        0,
        0,
    );
    assert_ne!(c.collision[0].flags[10][2] & CollisionFlag::W_W, 0);
    assert_eq!(c.collision[0].flags[2][2] & CollisionFlag::W_W, 0);
}

// --- finishBuild (TS 75-497) / fadeAdjacent (TS 543-565) ---

/// Build a ground packet: every tile is a single opcode-0 byte (perlin
/// fallback on level 0, `groundh[level-1] - 240` above), except `tiles`,
/// whose extra bytes (`82` = floort1 = 1, or `2, v` = floort2 = v with
/// shape 0) precede the terminating 0.
fn ground_src(tiles: &[(i32, i32, i32, &[u8])]) -> Vec<u8> {
    let mut src = Vec::new();
    for level in 0..BuildArea::LEVELS {
        for x in 0..64 {
            for z in 0..64 {
                if let Some((_, _, _, extra)) = tiles
                    .iter()
                    .find(|(l, tx, tz, _)| (*l, *tx, *tz) == (level, x, z))
                {
                    src.extend_from_slice(extra);
                }
                src.push(0);
            }
        }
    }
    src
}

/// The brief's Step-1 scenario: one level-0 floor tile (opcode 82, floort1
/// = 1) with a configured flo, so `finishBuild`'s blend pass calls
/// `setGround` for it. The all-zero src from the brief lays down no floors
/// and no ground (TS `setGround` only fires inside `t1 > 0 || t2 > 0`), so
/// the planted tile is what makes "at least one tile" true.
#[test]
fn finish_build_sets_quick_ground_after_load_ground() {
    let _r = Renderer::new(false);
    let mut c = client();
    c.set_draw(true);
    Arc::get_mut(&mut c.cache).unwrap().flos.push(FloType {
        chroma: 10,
        underlay_hue: 5,
        saturation: 100,
        lightness: 128,
        ..FloType::default()
    });
    let mut build = ClientBuild::new();
    let src = ground_src(&[(0, 2, 2, &[82])]);
    build.load_ground(&mut c.groundh, &mut c.mapl, &src, 0, 0, 0, 0);
    c.world.fill_base_level(0);
    build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);

    // the floor tile gets a PLAIN quick ground (t2 == 0 path, TS 254-273)
    let sq = c.world.square(0, 2, 2).expect("floor tile square");
    assert!(sq.quick_ground.is_some(), "finishBuild must setGround at least one tile");

    let mut any = false;
    for level in 0..BuildArea::LEVELS {
        for x in 0..BuildArea::SIZE {
            for z in 0..BuildArea::SIZE {
                if let Some(sq) = c.world.square(level, x, z) {
                    if sq.quick_ground.is_some() || sq.ground.is_some() {
                        any = true;
                    }
                }
            }
        }
    }
    assert!(any, "finishBuild must setGround at least one tile");
}

/// Task 5 rule 6: an unheaded build (`overlay_mesh == false`, what
/// `Client::map_build` mirrors from `draw == false`) keeps the sim data —
/// typecodes, heights, collision — but writes no overlay
/// `Ground`/`QuickGround` meshes; the first headed paint materializes
/// them from the stamps.
#[test]
fn unheaded_build_keeps_stamp_but_no_mesh_until_materialize() {
    let _r = Renderer::new(false);
    let mut c = client();
    c.world.overlay_mesh = false;
    Arc::get_mut(&mut c.cache).unwrap().flos.push(FloType {
        chroma: 10,
        underlay_hue: 5,
        saturation: 100,
        lightness: 128,
        ..FloType::default()
    });
    let mut build = ClientBuild::new();
    let src = ground_src(&[(0, 2, 2, &[82])]);
    build.load_ground(&mut c.groundh, &mut c.mapl, &src, 0, 0, 0, 0);
    // load_ground zeroes mapl per tile; plant the block flag after it
    c.mapl[0][5][5] = MapFlag::BLOCK as u8;
    c.world.fill_base_level(0);
    build.finish_build(
        &c.cache,
        &c.tex_average,
        &mut c.world,
        &mut c.collision,
        &c.groundh,
        &c.mapl,
    );

    // the sim kept the overlay typecodes ...
    let sq = c.world.square(0, 2, 2).expect("floor tile square");
    assert!(
        sq.overlay_stamp.is_some(),
        "unheaded finishBuild must record the overlay stamp"
    );
    // ... but no square holds an overlay mesh
    let mut any_mesh = false;
    for level in 0..BuildArea::LEVELS {
        for x in 0..BuildArea::SIZE {
            for z in 0..BuildArea::SIZE {
                if let Some(sq) = c.world.square(level, x, z) {
                    if sq.quick_ground.is_some() || sq.ground.is_some() {
                        any_mesh = true;
                    }
                }
            }
        }
    }
    assert!(
        !any_mesh,
        "unheaded finishBuild must not write overlay meshes"
    );

    // collision is still stamped as usual (mapl BLOCK -> WR_GRND)
    assert_ne!(
        c.collision[0].flags[5][5] & CollisionFlag::WR_GRND,
        0,
        "unheaded build must still block collision"
    );

    // the renderer's first paint after the build consumes the pending
    // flag and materializes the mesh from the stamp
    assert!(
        c.world.take_overlay_pending(),
        "finishBuild must flag the overlay materialize"
    );
    c.world.materialize_overlay();
    let sq = c.world.square(0, 2, 2).expect("floor tile square");
    assert!(
        sq.quick_ground.is_some(),
        "materialize_overlay must build the quick ground"
    );
}

/// Live attach: `map_build` copies `draw` into `overlay_mesh`. If `set_draw(true)`
/// does not flip that flag, later `set_ground` (zone/rebuild while headed)
/// keeps writing stamps only and the scene looks empty/broken.
#[test]
fn set_draw_true_enables_overlay_mesh_for_later_set_ground() {
    let mut c = client();
    c.set_draw(false);
    c.world.overlay_mesh = c.draw;
    assert!(!c.world.overlay_mesh);
    c.set_draw(true);
    assert!(
        c.world.overlay_mesh,
        "headed attach must flip overlay_mesh so live set_ground writes verts"
    );
}

/// Task 5: the materialized mesh must be exactly what a headed build
/// wrote at `set_ground` time, for a non-quick overlay shape too.
#[test]
fn ground_stamp_materializes_the_mesh_a_headed_build_writes() {
    let _r = Renderer::new(false);
    let mut unheaded = client();
    unheaded.world.overlay_mesh = false;
    let mut headed = client();
    headed.set_draw(true);

    for (x, z) in [(2, 2), (3, 3)] {
        let args = (
            0,
            x,
            z,
            TerrainOverlayShape::TRAPEZIUM,
            1,
            7,
            1000,
            1004,
            1010,
            1006,
            0x111111,
            0x222222,
            0x333333,
            0x444444,
            0x555555,
            0x666666,
            0x777777,
            0x888888,
            0x99,
            0xaa,
        );
        unheaded.world.set_ground(
            args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7, args.8, args.9,
            args.10, args.11, args.12, args.13, args.14, args.15, args.16, args.17, args.18,
            args.19,
        );
        headed.world.set_ground(
            args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7, args.8, args.9,
            args.10, args.11, args.12, args.13, args.14, args.15, args.16, args.17, args.18,
            args.19,
        );
    }

    assert!(unheaded
        .world
        .square(0, 2, 2)
        .expect("stamp tile")
        .ground
        .is_none());
    unheaded.world.materialize_overlay();

    for (x, z) in [(2, 2), (3, 3)] {
        let got = unheaded
            .world
            .square(0, x, z)
            .expect("materialized tile")
            .ground
            .as_deref()
            .expect("overlay mesh");
        let want = headed
            .world
            .square(0, x, z)
            .expect("headed tile")
            .ground
            .as_deref()
            .expect("overlay mesh");
        assert_eq!(got.vertex_count, want.vertex_count);
        assert_eq!(got.vertex_x, want.vertex_x);
        assert_eq!(got.vertex_y, want.vertex_y);
        assert_eq!(got.vertex_z, want.vertex_z);
        assert_eq!(got.face_count, want.face_count);
        assert_eq!(got.face_vertex_a, want.face_vertex_a);
        assert_eq!(got.face_vertex_b, want.face_vertex_b);
        assert_eq!(got.face_vertex_c, want.face_vertex_c);
        assert_eq!(got.face_colour_a, want.face_colour_a);
        assert_eq!(got.face_colour_b, want.face_colour_b);
        assert_eq!(got.face_colour_c, want.face_colour_c);
        assert_eq!(got.face_texture, want.face_texture);
        assert_eq!(got.flat, want.flat);
        assert_eq!(got.minimap_overlay, want.minimap_overlay);
        assert_eq!(got.minimap_underlay, want.minimap_underlay);
        assert_eq!(got.overlay_shape, want.overlay_shape);
        assert_eq!(got.overlay_rotation, want.overlay_rotation);
    }
}

/// Task 5 spec contract: `Option<Box<Ground>>` is 8 bytes — the
/// occupied-tile hole for a headed world whose overlay is present.
#[test]
fn ground_hole_is_one_fat_pointer() {
    assert_eq!(size_of::<Option<Box<Ground>>>(), 8);
}

#[test]
fn finish_build_blocks_map_flag_block_tiles() {
let _r = Renderer::new(false);
    let _r = Renderer::new(false);
    let mut c = client();
    c.mapl[0][2][2] = MapFlag::BLOCK as u8;
    let mut build = ClientBuild::new();
    build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::WR_GRND, 0);
}

#[test]
fn finish_build_link_below_blocks_lower_level() {
let _r = Renderer::new(false);
    let _r = Renderer::new(false);
    let mut c = client();
    // a level-1 Block with LinkBelow lands on level 0's collision grid
    // (TS 79-87: trueLevel = level - 1)
    c.mapl[1][2][2] = (MapFlag::BLOCK | MapFlag::LINK_BELOW) as u8;
    let mut build = ClientBuild::new();
    build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::WR_GRND, 0);
    assert_eq!(c.collision[1].flags[2][2] & CollisionFlag::WR_GRND, 0);
}

#[test]
fn finish_build_clamps_hue_and_lig_off() {
let _r = Renderer::new(false);
    let _r = Renderer::new(false);
    let mut c = client();
    let mut build = ClientBuild::new();
    assert!((-8..=8).contains(&build.hue_off), "hue_off {}", build.hue_off);
    assert!((-16..=16).contains(&build.lig_off), "lig_off {}", build.lig_off);
    for _ in 0..200 {
        build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    }
    assert!((-8..=8).contains(&build.hue_off), "hue_off {}", build.hue_off);
    assert!((-16..=16).contains(&build.lig_off), "lig_off {}", build.lig_off);
}

#[test]
fn finish_build_push_down_link_below() {
let _r = Renderer::new(false);
    let _r = Renderer::new(false);
    let mut c = client();
    c.set_draw(true);
    c.world.fill_base_level(0);
    // a level-1 PLAIN tile at (2,2); LinkBelow pushes it down to level 0
    // (TS 333-339 + World.pushDown)
    c.world.set_ground(
        1,
        2,
        2,
        TerrainOverlayShape::PLAIN,
        LocAngle::WEST,
        -1,
        0,
        0,
        0,
        0,
        1,
        1,
        1,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
    );
    c.mapl[1][2][2] = MapFlag::LINK_BELOW as u8;
    let mut build = ClientBuild::new();
    build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    let sq = c.world.square(0, 2, 2).expect("pushed-down tile");
    assert_eq!(sq.level, 0);
    assert!(sq.quick_ground.is_some());
}

#[test]
fn finish_build_clears_flat_floor_occluder_bits() {
let _r = Renderer::new(false);
    let _r = Renderer::new(false);
    let mut c = client();
    let mut build = ClientBuild::new();
    // a 1x5 flat-floor run (bit 0x4) at level 0: area 5 >= 4 so it becomes
    // a `setOcclude(level, 4, ...)` box and the bits are cleared (TS 472-496)
    for z in 8..=12 {
        build.mapo[0][10][z] |= 0x4;
    }
    build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    for z in 8..=12 {
        assert_eq!(build.mapo[0][10][z] & 0x4, 0, "floor bit cleared at z={z}");
    }
}

#[test]
fn finish_build_hooks_share_light() {
let _r = Renderer::new(false);
    // Task 3b: the models live on the render side, so `finishBuild` only
    // flags the share-light pass (`World.shareLight(64, 768, -50, -10, -50)`
    // from the TS 331 hook) instead of running it over sim-side models.
    // The render-side pass consumes a model's point normals and lights its
    // vertices exactly as the old `finishBuild` hook did.
    let mut c = client();
    c.world.set_wall(0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0);

    let mut build = ClientBuild::new();
    build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);

    assert!(c.world.take_share_light_pending(), "finishBuild must flag the render-side share_light");

    let mut model = Model::default();
    model.num_points = 3;
    model.point_x = Some(vec![-50, 50, 50]);
    model.point_y = Some(vec![-50, -50, 50]);
    model.point_z = Some(vec![0, 0, 0]);
    model.num_faces = 1;
    model.face_vertex_a = Some(vec![0]);
    model.face_vertex_b = Some(vec![2]);
    model.face_vertex_c = Some(vec![1]);
    model.face_colour = Some(vec![Colour::CYAN]);
    model.calc_bounding_cylinder();
    model.calculate_normals(64, 768, -50, -10, -50, false);
    let mut rw = RenderWorld::new();
    rw.set_wall_model(&c.world, 0, 2, 2, Some(SceneModel::Model(model)), None);
    rw.share_light(&mut c.world, 64, 768, -50, -10, -50);

    let SceneModel::Model(m) = rw.wall_model1(&c.world, &c.cache, 0, 0, 2, 2).expect("wall model1")
    else {
        panic!("wall model1 must be a Model")
    };
    assert!(m.point_normal.is_none(), "share_light must consume point normals");
    assert!(m.shared_point_normal.is_none(), "share_light must consume shared normals");
    let lit = m.face_colour_a.as_ref().expect("lit colour")[0];
    assert_ne!(lit, 0, "share_light must light wall vertices");
}

#[test]
fn finish_build_clears_wall_occluder_bits() {
let _r = Renderer::new(false);
    let _r = Renderer::new(false);
    let mut c = client();
    let mut build = ClientBuild::new();
    // an 8-tile wall0 run (bit 0x1) along z at level 0: area 8 >= 8 so it
    // becomes a `setOcclude(level, 1, ...)` box (TS 343-405)
    for z in 2..=9 {
        build.mapo[0][10][z] |= 0x1;
    }
    build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    for z in 2..=9 {
        assert_eq!(build.mapo[0][10][z] & 0x1, 0, "wall bit cleared at z={z}");
    }
}

#[test]
fn finish_build_magenta_overlay_floor() {
let _r = Renderer::new(false);
    let _r = Renderer::new(false);
    let mut c = client();
    c.set_draw(true);
    Arc::get_mut(&mut c.cache).unwrap().flos.push(FloType {
        colour: Colour::MAGENTA,
        ..FloType::default()
    });
    let mut build = ClientBuild::new();
    // opcode 2: floort2 = 1 (the g1b), floors = 0 -> DIAGONAL overlay, so
    // the t2 > 0 branch runs the magenta path (TS 280-318)
    let src = ground_src(&[(0, 2, 2, &[2, 1])]);
    build.load_ground(&mut c.groundh, &mut c.mapl, &src, 0, 0, 0, 0);
    c.world.fill_base_level(0);
    build.finish_build(&c.cache, &c.tex_average, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    let sq = c.world.square(0, 2, 2).expect("overlay tile square");
    assert!(sq.quick_ground.is_some());
}

// --- ground tile gating (Java ClientBuild.java:972-975) ---

#[test]
fn ground_tile_visible_gates_force_high_detail_in_low_mem() {
let _r = Renderer::new(false);
    let mut c = client();
    // low-mem: a ForceHighDetail tile's ground is culled by the finishBuild
    // gate; high-mem ignores the flag (Java `(mapl[...] & 0x10) == 0`).
    c.mapl[0][2][2] = MapFlag::FORCE_HIGH_DETAIL as u8;
    assert!(!ClientBuild::ground_tile_visible(&c.mapl, 0, 2, 2, true, 0));
    assert!(ClientBuild::ground_tile_visible(&c.mapl, 0, 2, 2, false, 0));
}

#[test]
fn ground_tile_visible_requires_matching_vis_below_level() {
let _r = Renderer::new(false);
    let c = client();
    // tile (2,2) on level 0 has no map flags: getVisBelowLevel is 0, so the
    // low-mem gate only opens when minusedlevel == 0.
    assert!(ClientBuild::ground_tile_visible(&c.mapl, 0, 2, 2, true, 0));
    assert!(!ClientBuild::ground_tile_visible(&c.mapl, 0, 2, 2, true, 1));
}

#[test]
fn fade_adjacent_level0_seam() {
let _r = Renderer::new(false);
    let mut c = client();
    c.groundh[0][64][5] = 100;
    c.groundh[0][65][5] = 50;
    c.groundh[0][5][64] = 100;
    c.groundh[0][5][65] = 50;
    let mut build = ClientBuild::new();
    // the TS mapBuild call is fadeAdjacent(z, x, 64, 64)
    build.fade_adjacent(&mut c.groundh, 0, 0, 64, 64);
    assert_eq!(c.groundh[0][64][5], 50, "east seam inherits x + 1");
    assert_eq!(c.groundh[0][5][64], 50, "south seam inherits z + 1");
    assert_eq!(build.shadow[0][10][10], 127);
    // the far edge keeps its own value
    assert_eq!(c.groundh[0][65][5], 50);
}

#[test]
fn scene_model_ids_protects_placed_loc_models() {
let _r = Renderer::new(false);
    let mut c = client();
    let mut cache = Cache::default();
    cache.locs.push(LocType {
        model: Some(vec![42]),
        ..LocType::default()
    });
    cache.locs.push(LocType::default());
    c.cache = Arc::new(cache);
    // A wall whose typecode decodes to loc id 0 (`x + z<<7 + 0x40000000`,
    // the `addLoc` layout). Its model 42 must survive lowmem's unload.
    c.world.set_wall(0, 2, 2, 0, 0, 0, 2 + (2 << 7) + 0x40000000, 0, 0, 0, 0, 0);
    let used = c.scene_model_ids(64);
    assert!(used[42], "a placed wall's model id is protected from unload");
    assert!(!used[43], "an unreferenced model id stays unprotected");
}
