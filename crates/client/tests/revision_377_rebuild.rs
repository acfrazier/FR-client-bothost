//! Java-377 instance rebuilds through the production packet and scene paths.
//! The corpus frame is synthetic output of the original engine encoder, not a capture.
use client::client::{Client, ClientConfig, ClientNpc, ClientPlayer, ClientRevision};
use client::config::Cache;
use client::dash3d::{ClientObj, LocChange};
use client::datastruct::LinkList;
use client::io::{OnDemand, Packet};
use client::render::Renderer;
use std::sync::Arc;

fn client() -> Client {
    let mut c = Client::from_shared_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir: std::env::var("ISOHOME_DIR")
                .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").into()),
            members: true,
            lowmem: false,
        },
        Arc::new(Cache::default()),
        Arc::new(Vec::new()),
        Vec::new(),
        ClientRevision::R377,
    );
    c.ingame = true;
    c.scene_state = 2;
    c.on_demand = Some(OnDemand::new_unconnected());
    c
}

fn engine_fixture() -> Vec<u8> {
    // RebuildRegionEncoder.ts:9-46; Client.java:8087-8119.
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/revision_377/packets.json")).unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "RebuildRegion")
        .unwrap();
    assert_eq!(case["origin"], "original-engine-encoder");
    case["payload_hex"]
        .as_str()
        .unwrap()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn dispatch(c: &mut Client, opcode: i32, bytes: Vec<u8>) {
    let mut packet = Packet::new(bytes);
    packet.set_frame_end(packet.length());
    c.psize = packet.length() as i32;
    c.handle_packet(opcode, &mut packet);
    assert!(
        c.ingame,
        "packet {opcode} rejected: {:?}",
        c.take_session_exit_reason()
    );
    assert_eq!(packet.pos, packet.length());
    assert_eq!(c.ptype, -1);
}

#[test]
fn packet53_dispatches_instance_rebuild_and_shifts_java_state() {
    let _renderer = Renderer::new(false);
    let mut c = client();
    c.map_build_prev_base_x = 3144;
    c.map_build_prev_base_z = 3168;
    c.cinema_cam = true;
    let mut local = ClientPlayer::at(40, 50);
    let mut npc = ClientNpc::at(40, 50);
    for index in 0..10 {
        local.route_x[index] = 40 + index as i32;
        local.route_z[index] = 50 + index as i32;
        npc.route_x[index] = local.route_x[index];
        npc.route_z[index] = local.route_z[index];
    }
    local.x = 40 * 128;
    local.z = 50 * 128;
    npc.x = local.x;
    npc.z = local.z;
    c.players[17] = Some(Box::new(local.clone()));
    c.local_player = Some(local);
    c.npc[0] = Some(Box::new(npc));
    let mut stack = LinkList::new();
    stack.push(ClientObj::new(0, 7));
    c.ground_obj[0][20][20] = Some(Box::new(stack));
    c.loc_changes.push(LocChange {
        x: 20,
        z: 20,
        ..Default::default()
    });
    c.loc_changes.push(LocChange {
        x: 2,
        z: 20,
        ..Default::default()
    });
    c.minimap_flag_x = 40;
    c.minimap_flag_z = 50;
    c.try_move_path = vec![(40, 50)];
    let before = c.gens;

    dispatch(&mut c, 53, engine_fixture());

    assert_eq!(
        (c.map_build_centre_zone_x, c.map_build_centre_zone_z),
        (400, 401)
    );
    assert_eq!((c.map_build_base_x, c.map_build_base_z), (3152, 3160));
    assert_eq!(
        (c.map_build_prev_base_x, c.map_build_prev_base_z),
        (3152, 3160)
    );
    assert_eq!(c.map_build_index, [12900]); // Three templates, one source square.
    assert_eq!(c.map_build_ground_file, [-1]);
    assert_eq!(c.map_build_location_file, [-1]);
    assert_eq!(c.scene_state, 1);
    assert!(c.awaiting_player_info);
    assert!(!c.cinema_cam);
    assert!(!c.within_tutorial_island);
    let local = c.local_player.as_ref().unwrap();
    for index in 0..10 {
        assert_eq!(local.route_x[index], 32 + index as i32);
        assert_eq!(local.route_z[index], 58 + index as i32);
        assert_eq!(
            c.players[17].as_ref().unwrap().route_x[index],
            local.route_x[index]
        );
        assert_eq!(
            c.npc[0].as_ref().unwrap().route_z[index],
            local.route_z[index]
        );
    }
    assert_eq!((local.x, local.z), (32 * 128, 58 * 128));
    assert_eq!(
        (c.npc[0].as_ref().unwrap().x, c.npc[0].as_ref().unwrap().z),
        (local.x, local.z)
    );
    assert!(c.ground_obj[0][20][20].is_none());
    assert_eq!(
        c.ground_obj[0][12][28]
            .as_mut()
            .unwrap()
            .head()
            .unwrap()
            .count,
        7
    );
    let pending = c.loc_changes.head().unwrap();
    assert_eq!((pending.x, pending.z), (12, 28));
    assert!(c.loc_changes.next_node().is_none());
    assert_eq!((c.minimap_flag_x, c.minimap_flag_z), (32, 58));
    assert_eq!(c.try_move_path, [(32, 58)]);
    assert_eq!(c.gens.scene, before.scene + 1);
    assert_eq!(c.gens.camera, before.camera + 1);
}

fn template(plane: i32, zone_x: i32, zone_z: i32, rotation: i32) -> i32 {
    (plane << 24) | (zone_x << 14) | (zone_z << 3) | (rotation << 1)
}

fn put_bits(bytes: &mut Vec<u8>, bit: &mut usize, value: i32, count: usize) {
    for shift in (0..count).rev() {
        if *bit / 8 == bytes.len() {
            bytes.push(0);
        }
        bytes[*bit / 8] |= (((value >> shift) & 1) as u8) << (7 - *bit % 8);
        *bit += 1;
    }
}

// Original engine RebuildRegionEncoder.ts:15-46, including the byte alignment.
fn frame(zone_x: i32, zone_z: i32, slots: &[(usize, usize, usize, i32)]) -> Vec<u8> {
    let mut bytes = vec![(zone_x >> 8) as u8, (zone_x as u8).wrapping_add(128)];
    let mut bit = 16;
    for plane in 0..4 {
        for x in 0..13 {
            for z in 0..13 {
                let value = slots
                    .iter()
                    .find(|&&(p, sx, sz, _)| (p, sx, sz) == (plane, x, z));
                put_bits(&mut bytes, &mut bit, i32::from(value.is_some()), 1);
                if let Some(&(_, _, _, packed)) = value {
                    put_bits(&mut bytes, &mut bit, packed, 26);
                }
            }
        }
    }
    bytes.extend_from_slice(&[(zone_z >> 8) as u8, (zone_z as u8).wrapping_add(128)]);
    bytes
}

fn normal(zone_x: i32, zone_z: i32) -> Vec<u8> {
    vec![
        (zone_z >> 8) as u8,
        zone_z as u8,
        (zone_x as u8).wrapping_add(128),
        (zone_x >> 8) as u8,
    ]
}

fn land(flags: u8, floors: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    for plane in 0..4 {
        for x in 0..64 {
            for z in 0..64 {
                let selected = plane == 2 && x == 2 && z == 5;
                if selected {
                    if floors {
                        bytes.extend_from_slice(&[7, 3, 82]);
                    }
                    if flags != 0 {
                        bytes.push(49 + flags);
                    }
                }
                bytes.extend_from_slice(&[1, if selected { 7 } else { 10 }]);
            }
        }
    }
    bytes
}

fn smart(bytes: &mut Vec<u8>, value: i32) {
    if value < 128 {
        bytes.push(value as u8);
    } else {
        bytes.extend_from_slice(&((value + 32768) as u16).to_be_bytes());
    }
}

fn locations(positions: &[(i32, i32, i32)]) -> Vec<u8> {
    let mut bytes = vec![1]; // Location 0.
    let mut previous = 0;
    for &(plane, x, z) in positions {
        let position = (plane << 12) | (x << 6) | z;
        smart(&mut bytes, position - previous + 1);
        bytes.push((10 << 2) | 1); // Non-square footprint, own rotation 1.
        previous = position;
    }
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn synchronize_player(c: &mut Client) {
    c.local_player
        .get_or_insert_with(|| ClientPlayer::at(50, 50));
    // Java local no-update bit, zero old visibles, 2047 addition terminator.
    dispatch(c, 90, vec![0, 0x7f, 0xf0]);
    assert!(!c.awaiting_player_info);
}

#[test]
fn synthetic_engine_encoder_frame_and_decode_match_existing_corpus() {
    use client::client::packet_377::{decode_packet_377, Operation377};
    let bytes = frame(
        400,
        401,
        &[
            (0, 0, 0, template(2, 401, 802, 3)),
            (1, 6, 6, template(1, 403, 806, 1)),
            (3, 12, 12, template(3, 401, 802, 2)),
        ],
    );
    assert_eq!(bytes, engine_fixture());
    let c = client();
    let decoded = decode_packet_377(53, &bytes).unwrap().validate(&c).unwrap();
    let Operation377::Rebuild(region) = decoded.operation() else {
        panic!("rebuild")
    };
    assert_eq!(region.zone_center(), (400, 401));
    assert_eq!(region.scene_base_tile(), (3152, 3160));
    assert_eq!(region.template_slots().unwrap().len(), 676);
    assert_eq!(region.unique_map_squares().collect::<Vec<_>>(), [12900]);
}

#[test]
fn unique_source_squares_keep_first_template_order_and_do_not_skip_tutorial_sources() {
    let _renderer = Renderer::new(false);
    let mut c = client();
    dispatch(
        &mut c,
        53,
        frame(
            400,
            401,
            &[
                (0, 0, 0, template(2, 80, 160, 0)),
                (0, 0, 1, template(0, 16, 24, 2)),
                (1, 0, 0, template(3, 81, 161, 3)),
                (3, 12, 12, template(1, 400, 392, 1)),
            ],
        ),
    );
    assert_eq!(
        c.map_build_index,
        [(10 << 8) + 20, (2 << 8) + 3, (50 << 8) + 49]
    );
    assert_eq!(c.map_build_ground_file, [-1; 3]);
    assert_eq!(c.map_build_location_file, [-1; 3]);
}

#[test]
fn java_rotated_land_and_non_square_locations_build_at_destination_planes() {
    use client::config::{FloType, LocType};
    use client::dash3d::CollisionFlag;
    let _renderer = Renderer::new(false);
    let mut c = client();
    let cache = Arc::get_mut(&mut c.cache).unwrap();
    cache.locs.push(LocType {
        width: 2,
        length: 3,
        ..Default::default()
    });
    cache.flos.resize_with(3, || FloType {
        chroma: 10,
        ..Default::default()
    });
    let mut slots = Vec::new();
    for rotation in 0..4 {
        slots.push((0, 2 + rotation as usize, 3, template(2, 320, 640, rotation)));
    }
    slots.push((3, 8, 8, template(2, 320, 640, 0)));
    dispatch(&mut c, 53, frame(400, 401, &slots));
    c.map_build_ground_data[0] = Some(land(1, true));
    // Wrong source plane and wrong source chunk must not be placed.
    c.map_build_location_data[0] = Some(locations(&[(1, 2, 5), (2, 2, 5), (2, 20, 20)]));
    synchronize_player(&mut c);
    c.out.pos = 0;
    assert_eq!(c.check_scene(), 0);
    assert_eq!(c.scene_state, 2);
    assert_eq!(&c.out.data()[..c.out.pos], &[40, 40, 40, 40, 6]);
    // Executed unmodified Java World.method16 + WorldRegion (java-oracle.log).
    for (rotation, (terrain, loc)) in [
        ((18, 29), (18, 29)),
        ((29, 29), (29, 27)),
        ((37, 26), (35, 25)),
        ((42, 26), (41, 26)),
    ]
    .into_iter()
    .enumerate()
    {
        let (x, z) = terrain;
        assert_eq!(c.groundh[0][x][z], -56);
        assert_eq!(c.world.groundh_at(0, x as i32, z as i32), -56);
        assert_eq!(c.mapl[0][x][z], 1);
        let (width, length) = if rotation & 1 == 0 { (3, 2) } else { (2, 3) };
        for x in loc.0..loc.0 + width {
            for z in loc.1..loc.1 + length {
                assert_ne!(
                    c.collision[0].flags[x][z] & 0x100,
                    0,
                    "rotation {rotation} at {x},{z}"
                );
            }
        }
        assert_eq!(c.collision[0].flags[loc.0 - 1][loc.1] & 0x100, 0);
        assert_eq!(c.collision[0].flags[loc.0 + width][loc.1] & 0x100, 0);
    }
    assert_eq!(c.groundh[3][66][69], -56); // Destination, not source plane 2.
    assert_eq!(c.groundh[2][66][69], 0);
    assert_ne!(c.collision[3].flags[66][69] & 0x100, 0);
    assert_ne!(c.collision[0].flags[18][29] & CollisionFlag::WR_GRND, 0);
    assert_eq!(c.collision[0].flags[4][5] & 0x100, 0);
    // Parked bytes and instance templates remain usable for a lowmem rebuild.
    c.scene_state = 1;
    assert_eq!(c.check_scene(), 0);
    assert_eq!(c.groundh[0][18][29], -56);
}

#[test]
fn bridge_flags_route_instance_location_collision_to_the_plane_below() {
    use client::config::LocType;
    let _renderer = Renderer::new(false);
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
        width: 2,
        length: 3,
        ..Default::default()
    });
    dispatch(
        &mut c,
        53,
        frame(400, 401, &[(1, 2, 3, template(2, 320, 640, 0))]),
    );
    c.map_build_ground_data[0] = Some(land(2, false));
    c.map_build_location_data[0] = Some(locations(&[(2, 2, 5)]));
    synchronize_player(&mut c);
    assert_eq!(c.check_scene(), 0);
    assert_ne!(c.collision[0].flags[18][29] & 0x100, 0);
    assert_eq!(c.collision[1].flags[18][29] & 0x100, 0);
}

#[test]
fn empty_and_missing_source_templates_clear_stale_terrain_and_finish_loading() {
    let _renderer = Renderer::new(false);
    for slots in [vec![], vec![(0, 4, 4, template(2, 320, 640, 0))]] {
        let mut c = client();
        for level in 0..4 {
            for x in 0..104 {
                c.mapl[level][x].fill(7);
                c.groundh[level][x].fill(-123);
            }
        }
        dispatch(&mut c, 53, frame(400, 401, &slots));
        assert_eq!(c.check_scene(), -4);
        synchronize_player(&mut c);
        c.out.pos = 0;
        assert_eq!(c.check_scene(), 0);
        assert_eq!(c.groundh[0][33][33], 0);
        assert_eq!(c.groundh[3][33][33], 0);
        assert_eq!(c.mapl[0][33][33], 0);
        assert_eq!(&c.out.data()[..c.out.pos], &[40, 40, 40, 40, 6]);
    }
}

#[test]
fn instance_waits_for_land_locations_models_and_production_player_sync() {
    use client::config::LocType;
    let _renderer = Renderer::new(false);
    let mut c = client();
    Arc::get_mut(&mut c.cache).unwrap().locs.push(LocType {
        model: Some(vec![60000]),
        ..Default::default()
    });
    dispatch(
        &mut c,
        53,
        frame(400, 401, &[(0, 2, 3, template(2, 320, 640, 0))]),
    );
    c.map_build_ground_file[0] = 1;
    c.map_build_location_file[0] = 2;
    assert_eq!(c.check_scene(), -1);
    c.map_build_ground_data[0] = Some(land(0, false));
    assert_eq!(c.check_scene(), -2);
    c.map_build_location_data[0] = Some(locations(&[(2, 2, 5)]));
    // Source map coordinates are far outside the destination scene. Java's
    // instance model gate must check at (10,10), not normal world offsets.
    assert_eq!(c.check_scene(), -3);
    Arc::get_mut(&mut c.cache).unwrap().locs[0].model = None;
    assert_eq!(c.check_scene(), -4);
    synchronize_player(&mut c);
    assert_eq!(c.check_scene(), 0);
}

#[test]
fn same_center_replaces_templates_before_early_return_and_normal_clears_instance_mode() {
    let _renderer = Renderer::new(false);
    let mut c = client();
    dispatch(
        &mut c,
        53,
        frame(400, 401, &[(0, 2, 3, template(2, 320, 640, 0))]),
    );
    c.map_build_ground_data[0] = Some(land(0, false));
    c.scene_state = 2;
    c.cinema_cam = true;
    let indices = c.map_build_index.clone();
    dispatch(
        &mut c,
        53,
        frame(400, 401, &[(0, 5, 3, template(2, 320, 640, 3))]),
    );
    assert!(c.cinema_cam); // Same-center loaded scene: no shift/camera reset.
    assert_eq!(c.scene_state, 2);
    assert_eq!(c.map_build_index, indices);
    synchronize_player(&mut c);
    c.scene_state = 1;
    assert_eq!(c.check_scene(), 0);
    assert_eq!(c.groundh[0][42][26], -56); // New grid, not the old rotation-0 grid.

    // Same-center 222 must clear instance mode even though it returns early.
    dispatch(&mut c, 222, normal(400, 401));
    c.map_build_index.clear();
    c.map_build_ground_data.clear();
    c.map_build_location_data.clear();
    assert_eq!(c.check_scene(), -1000);
    dispatch(&mut c, 222, normal(401, 401));
    assert!(!c.cinema_cam);
    for data in &mut c.map_build_ground_data {
        *data = Some(land(0, false));
    }
    synchronize_player(&mut c);
    assert_eq!(c.check_scene(), 0);
    assert_eq!(c.groundh[0][42][26], -80); // Normal loading uses source plane 0.
}

struct Counting;
thread_local! {
    static ALLOCATIONS: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
}

fn record_allocation(size: usize) {
    ALLOCATIONS.with(|count| {
        if let Some((events, bytes)) = count.get() {
            count.set(Some((events + 1, bytes + size)));
        }
    });
}

// SAFETY: Every operation forwards the caller's allocation and layout to System.
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        record_allocation(layout.size());
        // SAFETY: Forward the caller's valid layout unchanged.
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        record_allocation(layout.size());
        // SAFETY: Forward the caller's valid layout unchanged.
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        record_allocation(size);
        // SAFETY: Forward the caller's allocation and layouts unchanged.
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: Forward the caller's allocation and layout unchanged.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn same_center_packet53_moves_the_decoded_grid_without_copying_or_extra_allocations() {
    let _renderer = Renderer::new(false);
    let mut c = client();
    let mut packet = Packet::new(engine_fixture());
    packet.set_frame_end(packet.length());
    c.psize = packet.length() as i32;
    c.handle_packet(53, &mut packet);
    assert!(c.ingame);
    c.scene_state = 2;
    ALLOCATIONS.with(|count| count.set(Some((0, 0))));
    for _ in 0..1000 {
        packet.pos = 0;
        c.handle_packet(53, &mut packet);
        assert!(c.ingame);
    }
    let measured = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    assert_eq!(measured, (1000, 1000 * 676 * 4));
    println!("Client inline bytes: {}", std::mem::size_of::<Client>());
}

#[test]
fn incomplete_or_trailing_instance_frames_disconnect_without_apply_publication() {
    for bytes in [
        engine_fixture()[..engine_fixture().len() - 1].to_vec(),
        {
            let mut bytes = engine_fixture();
            bytes.push(0);
            bytes
        },
        vec![0, 0],
    ] {
        let mut c = client();
        assert!(client::client::packet_377::decode_packet_377(53, &bytes).is_err());
        let mut packet = Packet::new(bytes);
        packet.set_frame_end(packet.length());
        c.psize = packet.length() as i32;
        c.handle_packet(53, &mut packet);
        assert!(!c.ingame);
        assert!(matches!(
            c.take_session_exit_reason(),
            Some(client::client::client::SessionExitReason::ProtocolError {
                class: "T2",
                last_opcode: 53,
                ..
            })
        ));
        // Disconnect invalidates slices once; no rebuild publication occurred.
        assert_eq!(c.gens.scene, 1);
        assert_eq!(c.gens.camera, 1);
    }
}

#[test]
fn logout_retires_the_instance_grid_before_a_new_session() {
    let mut c = client();
    dispatch(&mut c, 53, engine_fixture());
    c.logout();
    c.map_build_index.clear();
    c.map_build_ground_data.clear();
    c.map_build_location_data.clear();
    assert_eq!(c.check_scene(), -1000);
}
