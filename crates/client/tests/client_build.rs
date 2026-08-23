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
use std::sync::{Arc, Mutex};

use client::client::{Client, ClientBuild, ClientConfig};
use client::config::{Cache, FloType, LocType};
use client::dash3d::model::ModelProvider;
use client::dash3d::{BuildArea, CollisionFlag, LocAngle, LocShape, MapFlag, Model, SceneModel, TerrainOverlayShape};
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
    // model None but the ground decor is stored typecode-only so draw=false
    // clients can read the loc
    assert!(c.world.get_wall(0, 2, 2).is_none());
    let gd = c.world.get_gd(0, 2, 2).expect("ground decor placed");
    assert!(gd.model.is_none());
}

#[test]
fn skip_loc_models_places_typecode_without_mesh() {
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
        model: Some(vec![60000]),
        active: true,
        blockwalk: true,
        forcedecor: true,
        ..LocType::default()
    });
    let mut build = ClientBuild::new();
    build.skip_loc_models = true;
    // same src as the ground-decor tests: loc 0 at (2,2), shape 22
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
    // skip_loc_models must still place the typecode and block walking; only
    // the mesh decode is dropped (the loc's 60000 model is never requested)
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::WR_GRND, 0);
    let gd = c.world.get_gd(0, 2, 2).expect("ground decor placed");
    assert!(gd.model.is_none());
}

#[test]
fn skip_loc_models_places_centrepiece_typecode_without_mesh() {
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
        model: Some(vec![60000]),
        active: true,
        blockwalk: true,
        ..LocType::default()
    });
    let mut build = ClientBuild::new();
    build.skip_loc_models = true;
    // deltaId 1 (loc 0); deltaPos 131 -> locPos 130 (x=2, z=2, level=0);
    // info = shape 10 (CENTREPIECE_STRAIGHT) << 2, rotation 0; end pos; end id
    let src = [
        0x01,
        0x80,
        0x83,
        (LocShape::CENTREPIECE_STRAIGHT << 2) as u8,
        0x00,
        0x00,
    ];
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
    // skip_loc_models must still place the centrepiece typecode and block
    // walking; only the mesh decode is dropped (the 60000 model is never
    // requested)
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::WALK_SCENERY, 0);
    let sprite = c.world.get_scene(0, 2, 2).expect("centrepiece sprite placed");
    assert_eq!(sprite.typecode, 0x4000_0102);
    assert!(sprite.model.is_none());
}

/// `map_build` with `skip_loc_models` (draw off, the null build) must not
/// pre-fill the base level: loc setters create `Box<Square>` on demand, so
/// the 104×104 tile grid stays sparse until real ground/locs land.
#[test]
fn skip_loc_models_does_not_prefill_base_level() {
    let mut c = client();
    c.ingame = true;
    c.awaiting_player_info = false;
    c.scene_state = 1;
    // one region, files -1 so no wait (tutorial skip pattern); draw is off
    // by default so `map_build` builds with `skip_loc_models`
    c.map_build_index = vec![0];
    c.map_build_ground_file = vec![-1];
    c.map_build_location_file = vec![-1];
    c.map_build_ground_data = vec![None];
    c.map_build_location_data = vec![None];
    assert_eq!(c.check_scene(), 0);
    assert_eq!(c.scene_state, 2);
    let occupied = (0..104)
        .flat_map(|x| (0..104).map(move |z| (x, z)))
        .filter(|&(x, z)| c.world.square(0, x, z).is_some())
        .count();
    assert!(occupied < 104 * 104, "base level must stay sparse, occupied={occupied}");
}

#[test]
fn load_locations_low_mem_skips_force_high_detail() {
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
    // the wall is stored typecode-only even without a model so draw=false
    // clients can read the loc; it still blocks walking (TS 924-928)
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::W_W, 0);
    let wall = c.world.get_wall(0, 2, 2).expect("wall placed");
    assert!(wall.model1.is_none());
}

#[test]
fn load_locations_skips_out_of_area_tiles() {
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
    let mut c = client();
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
    build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);

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

#[test]
fn finish_build_blocks_map_flag_block_tiles() {
    let mut c = client();
    c.mapl[0][2][2] = MapFlag::BLOCK as u8;
    let mut build = ClientBuild::new();
    build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::WR_GRND, 0);
}

#[test]
fn finish_build_link_below_blocks_lower_level() {
    let mut c = client();
    // a level-1 Block with LinkBelow lands on level 0's collision grid
    // (TS 79-87: trueLevel = level - 1)
    c.mapl[1][2][2] = (MapFlag::BLOCK | MapFlag::LINK_BELOW) as u8;
    let mut build = ClientBuild::new();
    build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    assert_ne!(c.collision[0].flags[2][2] & CollisionFlag::WR_GRND, 0);
    assert_eq!(c.collision[1].flags[2][2] & CollisionFlag::WR_GRND, 0);
}

#[test]
fn finish_build_clamps_hue_and_lig_off() {
    let mut c = client();
    let mut build = ClientBuild::new();
    assert!((-8..=8).contains(&build.hue_off), "hue_off {}", build.hue_off);
    assert!((-16..=16).contains(&build.lig_off), "lig_off {}", build.lig_off);
    for _ in 0..200 {
        build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    }
    assert!((-8..=8).contains(&build.hue_off), "hue_off {}", build.hue_off);
    assert!((-16..=16).contains(&build.lig_off), "lig_off {}", build.lig_off);
}

#[test]
fn finish_build_push_down_link_below() {
    let mut c = client();
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
    build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    let sq = c.world.square(0, 2, 2).expect("pushed-down tile");
    assert_eq!(sq.level, 0);
    assert!(sq.quick_ground.is_some());
}

#[test]
fn finish_build_clears_flat_floor_occluder_bits() {
    let mut c = client();
    let mut build = ClientBuild::new();
    // a 1x5 flat-floor run (bit 0x4) at level 0: area 5 >= 4 so it becomes
    // a `setOcclude(level, 4, ...)` box and the bits are cleared (TS 472-496)
    for z in 8..=12 {
        build.mapo[0][10][z] |= 0x4;
    }
    build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    for z in 8..=12 {
        assert_eq!(build.mapo[0][10][z] & 0x4, 0, "floor bit cleared at z={z}");
    }
}

#[test]
fn finish_build_hooks_share_light() {
    // A wall model with computed point normals (as `addLoc` leaves one)
    // must be lit by the TS 331 hook: `finishBuild` calls
    // `world.shareLight(64, 768, -50, -10, -50)`, so after the pass the
    // model's normals are consumed and its vertices carry lit colours.
    let mut c = client();
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
    c.world.set_wall(0, 2, 2, 0, 0, 0, Some(SceneModel::Model(model)), None, 0, 0);

    let mut build = ClientBuild::new();
    build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);

    let wall = c.world.get_wall(0, 2, 2).expect("wall");
    let SceneModel::Model(m) = wall.model1.as_deref().unwrap() else {
        panic!("wall model1 must be a Model")
    };
    assert!(m.point_normal.is_none(), "finishBuild must call share_light");
    assert!(m.shared_point_normal.is_none(), "finishBuild must call share_light");
    let lit = m.face_colour_a.as_ref().expect("lit colour")[0];
    assert_ne!(lit, 0, "finishBuild's share_light must light wall vertices");
}

#[test]
fn finish_build_clears_wall_occluder_bits() {
    let mut c = client();
    let mut build = ClientBuild::new();
    // an 8-tile wall0 run (bit 0x1) along z at level 0: area 8 >= 8 so it
    // becomes a `setOcclude(level, 1, ...)` box (TS 343-405)
    for z in 2..=9 {
        build.mapo[0][10][z] |= 0x1;
    }
    build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    for z in 2..=9 {
        assert_eq!(build.mapo[0][10][z] & 0x1, 0, "wall bit cleared at z={z}");
    }
}

#[test]
fn finish_build_magenta_overlay_floor() {
    let mut c = client();
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
    build.finish_build(&c.cache, &mut c.pix3d, &mut c.world, &mut c.collision, &c.groundh, &c.mapl);
    let sq = c.world.square(0, 2, 2).expect("overlay tile square");
    assert!(sq.quick_ground.is_some());
}

// --- ground tile gating (Java ClientBuild.java:972-975) ---

#[test]
fn ground_tile_visible_gates_force_high_detail_in_low_mem() {
    let mut c = client();
    // low-mem: a ForceHighDetail tile's ground is culled by the finishBuild
    // gate; high-mem ignores the flag (Java `(mapl[...] & 0x10) == 0`).
    c.mapl[0][2][2] = MapFlag::FORCE_HIGH_DETAIL as u8;
    assert!(!ClientBuild::ground_tile_visible(&c.mapl, 0, 2, 2, true, 0));
    assert!(ClientBuild::ground_tile_visible(&c.mapl, 0, 2, 2, false, 0));
}

#[test]
fn ground_tile_visible_requires_matching_vis_below_level() {
    let c = client();
    // tile (2,2) on level 0 has no map flags: getVisBelowLevel is 0, so the
    // low-mem gate only opens when minusedlevel == 0.
    assert!(ClientBuild::ground_tile_visible(&c.mapl, 0, 2, 2, true, 0));
    assert!(!ClientBuild::ground_tile_visible(&c.mapl, 0, 2, 2, true, 1));
}

#[test]
fn fade_adjacent_level0_seam() {
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
