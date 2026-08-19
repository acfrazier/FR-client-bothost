// `LocType.checkModelAll` + `ClientBuild.checkLocations`/`prefetchLocations`
// (client-ts `LocType.ts` 296-323, `ClientBuild.ts` 638-715). Model ids in
// these tests are chosen far outside anything a real pack could load, so
// `Model::requestDownload` can never see them as ready.
use client::client::ClientBuild;
use client::config::{Cache, LocType};
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
