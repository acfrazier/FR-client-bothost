//! Delay-only animation reads must preserve publication semantics without
//! cloning the transform data needed by model animation.
use client::{config::SeqType, dash3d::AnimFrame};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;
thread_local! {
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|count| {
            if let Some(n) = count.get() {
                count.set(Some(n + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn publish(delay: u8) {
    // Frame 30001: one translated group with x=1 and one base label.
    AnimFrame::unpack(&[
        0, 1, 0x75, 0x31, 1,  // head
        1,  // transform x present
        65, // signed smart x=1
        delay, 1, 1, 1, 0, // base size, type, label count, label
        0, 3, 0, 1, 0, 1, 0, 1, // section lengths
    ]);
}

#[test]
fn delay_lookup_preserves_fallbacks_and_publication_without_allocating() {
    publish(2);
    let mut seq = SeqType {
        frames: Some(vec![30001, 30002, -1]),
        delay: Some(vec![0, 0, 0]),
        ..SeqType::default()
    };
    assert_eq!(seq.get_delay(0), 2);
    assert_eq!(seq.get_delay(1), 1);
    assert_eq!(seq.get_delay(2), 1);
    assert_eq!(seq.get_delay(-1), 1);
    assert_eq!(seq.get_delay(3), 1);
    let mut owned = AnimFrame::get(30001).unwrap();
    assert_eq!(owned.tx.as_deref(), Some([1].as_slice()));
    owned.tx.as_mut().unwrap()[0] = 99;
    assert_eq!(AnimFrame::get(30001).unwrap().tx.unwrap(), vec![1]);
    AnimFrame::init(16); // a later client's initialization preserves data
    assert_eq!(seq.get_delay(0), 2);
    publish(0);
    // Java `SeqType.getDelay` (274 `SeqType.java` 83-85; 289 `Class27`
    // 104-106): a frame delay that resolves to 0 is played as 1.
    assert_eq!(seq.get_delay(0), 1);
    assert_eq!(owned.delay, 2);
    publish(7);
    assert_eq!(seq.get_delay(0), 7); // no stale memoized delay
    seq.delay = Some(vec![5]);
    assert_eq!(seq.get_delay(0), 5);
    seq.delay = Some(vec![0]);
    seq.frames = Some(vec![]);
    assert_eq!(seq.get_delay(0), 1);
    seq.frames = None;
    assert_eq!(seq.get_delay(0), 0);
    seq.frames = Some(vec![30001]);
    seq.delay = None;
    assert_eq!(seq.get_delay(0), 0);
    seq.delay = Some(vec![0]);
    COUNT.with(|count| count.set(Some(0)));
    for _ in 0..100 {
        std::hint::black_box(seq.get_delay(std::hint::black_box(0)));
    }
    let allocations = COUNT.with(|count| count.replace(None).unwrap());
    assert_eq!(
        allocations, 0,
        "delay-only reads must not clone frame storage"
    );
}
