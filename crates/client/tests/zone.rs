use client::config::{Cache, SeqType, SpotType};
use client::dash3d::{ClientObj, LocChange};
use client::datastruct::{LinkList, LinkableTrait};

#[test]
fn loc_change_defaults_end_time_minus_one() {
    let loc = LocChange::default();
    assert_eq!(loc.end_time, -1);
    assert_eq!(loc.new_type, 0);
}

#[test]
fn client_obj_roundtrips_in_link_list() {
    let mut list = LinkList::new();
    list.push(ClientObj::new(42, 5));
    assert_eq!(list.head().unwrap().id, 42);
    assert_eq!(list.head().unwrap().count, 5);
}

use client::dash3d::{ClientProj, MapSpotAnim, Model};
use client::graphics::Pix3D;

#[test]
fn rotate_x_axis_90_swaps_y_and_z() {
    let mut m = Model::default();
    m.num_points = 1;
    m.point_x = Some(vec![0]);
    m.point_y = Some(vec![128]);
    m.point_z = Some(vec![0]);
    m.rotate_x_axis(512); // 90° in 2048-circle; sin≈65536, cos≈0
    let y = m.point_y.as_ref().unwrap()[0];
    let z = m.point_z.as_ref().unwrap()[0];
    assert_eq!(y, 0);
    assert_eq!(z, 128);
}

#[test]
fn client_proj_set_target_places_startpos_along_delta() {
    let mut p = ClientProj::new(0, 0, 0, 100, 0, 0, 10, 0, 64, 0, 0);
    p.set_target(128.0, 100.0, 0.0, 0);
    // d=128, startpos=64 → x = 0 + 128*64/128 = 64
    assert!((p.x - 64.0).abs() < 1e-6);
    assert!((p.z - 0.0).abs() < 1e-6);
    assert!((p.y - 100.0).abs() < 1e-6);
}

#[test]
fn map_spot_anim_start_cycle_is_cycle_plus_delay() {
    let s = MapSpotAnim::new(0, 0, 64, 64, 0, 10, 5);
    assert_eq!(s.start_cycle, 15);
    assert!(!s.anim_complete);
}

#[test]
fn client_proj_move_by_uses_bound_seq_delays() {
    let cache = Cache {
        seqs: vec![
            SeqType::default(),
            SeqType::default(),
            SeqType::default(),
            SeqType { num_frames: 2, frames: Some(vec![0, 1]), iframes: Some(vec![0, 1]), delay: Some(vec![10, 5]), ..SeqType::default() },
        ],
        spots: vec![
            SpotType::default(),
            SpotType { id: 1, seq: Some(3), ..SpotType::default() },
        ],
        ..Cache::default()
    };

    let mut p = ClientProj::new(1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    p.bind_seq(&cache);
    p.anim_cycle = 20;
    p.move_by(1);
    // 21 > 10 → cycle 10, frame 1; 10 > 5 → cycle 4, frame 2 → wrap to 0
    assert_eq!(p.anim_frame, 0);
    assert_eq!(p.anim_cycle, 4);

    // Unbound seq leaves the anim loop skipped.
    let mut q = ClientProj::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    q.anim_cycle = 20;
    q.move_by(1);
    assert_eq!(q.anim_frame, 0);
    assert_eq!(q.anim_cycle, 20);
}
