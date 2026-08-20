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
