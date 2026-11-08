//! Java 377 camera packets through the production handler, then logic/render frames.
use client::client::{Client, ClientConfig, ClientRevision};
use client::config::Cache;
use client::io::{Packet, SERVER_PROT_SIZES_377};
use client::render::Renderer;
use client::util::JavaRandom;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

struct Counting;
thread_local! {
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}
// SAFETY: Forward every allocation unchanged to System. The thread-local
// counter records allocation events only while the test enables it.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: Forward the caller's valid layout unchanged.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: Forward the caller's valid layout unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: Forward the caller's allocation and layouts unchanged.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Forward the caller's allocation and layout unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn count() {
    COUNT.with(|count| {
        if let Some(n) = count.get() {
            count.set(Some(n + 1));
        }
    });
}
fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|count| count.set(Some(0)));
    f();
    COUNT.with(|count| count.replace(None).unwrap())
}

fn client() -> Client {
    let mut c = Client::from_shared_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir: std::env::temp_dir().to_str().unwrap().into(),
            members: true,
            lowmem: true,
        },
        Arc::new(Cache::default()),
        Arc::new(Vec::new()),
        Vec::new(),
        ClientRevision::R377,
    );
    c.ingame = true;
    c.scene_state = 2;
    c
}
fn dispatch(c: &mut Client, opcode: i32, packet: &mut Packet) {
    packet.pos = 0;
    packet.clear_frame_end();
    packet.set_frame_end(packet.length());
    c.psize = packet.length() as i32;
    c.handle_packet(opcode, packet);
    assert!(
        c.ingame,
        "camera {opcode} rejected: {:?}",
        c.take_session_exit_reason()
    );
    assert_eq!(packet.pos, packet.length());
    assert_eq!(c.ptype, -1);
}
fn send(c: &mut Client, opcode: i32, bytes: &[u8]) {
    dispatch(c, opcode, &mut Packet::new(bytes.to_vec()));
}
fn eye(c: &Client) -> [i32; 5] {
    [c.cam_x, c.cam_y, c.cam_z, c.cam_pitch, c.cam_yaw]
}

#[test]
fn java_opcodes_immediate_threshold_bridge_height_and_reset() {
    let mut c = client();
    for (opcode, length) in [(3, 6), (167, 6), (67, 4), (148, 0)] {
        assert_eq!(SERVER_PROT_SIZES_377[opcode], length);
    }
    assert_eq!(
        SERVER_PROT_SIZES_377[135], -1,
        "135 is MESSAGE_PRIVATE, not shake"
    );
    // Java getHeightmapY: the link-below bit selects the next plane, and
    // interpolation at the tile centre averages its four corners to 1500.
    c.mapl[1][4][5] = 2;
    c.groundh[1][4][5] = 1200;
    c.groundh[1][5][5] = 1400;
    c.groundh[1][4][6] = 1600;
    c.groundh[1][5][6] = 1800;
    send(&mut c, 3, &[4, 5, 0, 200, 10, 99]);
    assert!(c.cinema_cam);
    assert_eq!(&eye(&c)[..3], &[0, 0, 0]);
    send(&mut c, 3, &[4, 5, 0, 200, 10, 100]);
    assert_eq!(&eye(&c)[..3], &[576, 1300, 704]);
    c.cam_pitch = 250;
    c.cam_yaw = 999;
    send(&mut c, 167, &[4, 6, 0, 20, 10, 99]);
    assert_eq!(&eye(&c)[3..], &[250, 999]);
    // Downward target: atan2(-1320,128) masks into >383, then clamps.
    send(&mut c, 167, &[4, 6, 0, 20, 10, 100]);
    assert_eq!(&eye(&c)[3..], &[383, 0]);
    c.cam_y = -10000;
    send(&mut c, 167, &[4, 5, 0, 20, 0, 100]);
    assert_eq!(c.cam_pitch, 383, "vertical upward look clamps at 383");
    c.cam_y = 0;
    send(&mut c, 167, &[4, 6, 0, 0, 0, 100]);
    assert_eq!(c.cam_pitch, 128, "horizontal look clamps at 128");
    for axis in 0..5 {
        send(&mut c, 67, &[axis, 7, 40, 25]);
    }
    let before = eye(&c);
    send(&mut c, 148, &[]);
    assert!(!c.cinema_cam);
    assert_eq!(c.cam_shake, [false; 5]);
    c.game_loop();
    assert_eq!(
        eye(&c),
        before,
        "reset stops interpolation without clearing the eye"
    );
    assert_eq!(c.gens.camera, 12);
}

#[test]
fn java_interpolation_values_on_each_production_logic_frame() {
    let mut c = client();
    (c.cam_x, c.cam_y, c.cam_z, c.cam_pitch, c.cam_yaw) = (900, 100, 1000, 300, 2030);
    send(&mut c, 3, &[4, 5, 0, 200, 10, 50]);
    send(&mut c, 167, &[2, 7, 0, 50, 5, 75]);
    assert_eq!(eye(&c), [900, 100, 1000, 300, 2030]);
    // Computed with Java's integer division, atan2 mask/clamp, shortest-yaw
    // wrap and overshoot snap (Client.java:4701-4792), not the Rust routine.
    let java_frames = [
        [874, 75, 976, 311, 27],
        [850, 52, 953, 321, 68],
        [827, 30, 931, 330, 104],
        [805, 9, 910, 338, 137],
        [784, -11, 890, 346, 166],
        [764, -30, 871, 353, 192],
        [745, -48, 853, 360, 214],
        [727, -65, 836, 338, 234],
        [710, -81, 820, 318, 251],
        [694, -96, 805, 299, 265],
        [679, -111, 790, 282, 277],
        [664, -125, 776, 266, 287],
    ];
    for (index, expected) in java_frames.into_iter().enumerate() {
        c.game_loop();
        assert_eq!(eye(&c), expected, "Java logic frame {}", index + 1);
        assert_eq!(c.cam_shake_cycle, [(index + 1) as i32; 5]);
    }
    // Fixed-speed overshoot snaps on both signs, including yaw across zero.
    send(&mut c, 3, &[4, 5, 0, 200, 255, 0]);
    send(&mut c, 167, &[4, 6, 0, 200, 255, 0]);
    c.game_loop();
    assert_eq!(eye(&c), [576, -200, 704, 128, 32]);
    c.game_loop();
    assert_eq!(eye(&c), [576, -200, 704, 128, 0]);
}

#[test]
fn java_all_five_shake_axes_are_frame_local_and_seeded() {
    let mut c = client();
    let mut renderer = Renderer::new(false);
    renderer.rand = JavaRandom::new(42);
    (c.cam_x, c.cam_y, c.cam_z, c.cam_pitch, c.cam_yaw) = (10, 20, 30, 380, 2040);
    for axis in 0..5 {
        c.cam_shake_cycle[axis] = 99;
        send(&mut c, 67, &[axis as u8, 7, 40, 25]);
        assert_eq!(c.cam_shake_cycle[axis], 0);
    }
    assert!(!c.cinema_cam, "shake alone does not enter a cutscene");
    // Java drawScene: Random(42), jitter=7, amplitude=40, speed=25,
    // with one logic-cycle increment before each render. Order is x/y/z/yaw/pitch.
    let java_frames = [
        [23, 33, 37, 2047, 383],
        [35, 37, 46, 11, 383],
        [44, 46, 61, 18, 383],
        [45, 49, 69, 21, 383],
        [52, 59, 69, 34, 383],
        [48, 65, 69, 39, 383],
    ];
    for expected in java_frames {
        c.game_loop();
        let (x, y, z, pitch, yaw) = renderer.cam_shake_jitter(&mut c, 10, 20, 30, 380, 2040);
        assert_eq!([x, y, z, yaw, pitch], expected);
        assert_eq!(
            eye(&c),
            [10, 20, 30, 380, 2040],
            "shake must not accumulate"
        );
    }
    send(&mut c, 67, &[4, 0, 255, 100]);
    c.cam_shake_cycle[4] = 4; // sin(4) * 255 drives pitch below its floor.
    let (_, _, _, pitch, _) = renderer.cam_shake_jitter(&mut c, 0, 0, 0, 128, 0);
    assert_eq!(pitch, 128);
    send(&mut c, 148, &[]);
    assert_eq!(
        renderer.cam_shake_jitter(&mut c, 1, 2, 3, 128, 4),
        (1, 2, 3, 128, 4)
    );
}

#[test]
fn camera_dispatch_interpolation_and_shake_allocate_nothing() {
    // Existing 64-bit layout: this feature reuses the camera fields, not new
    // per-client state. Keep the measured baseline as a growth tripwire.
    #[cfg(target_pointer_width = "64")]
    assert!(std::mem::size_of::<Client>() <= 28_872);
    let mut c = client();
    let mut renderer = Renderer::new(false);
    let mut moveto = Packet::new(vec![4, 5, 0, 200, 10, 50]);
    let mut lookat = Packet::new(vec![2, 7, 0, 50, 5, 75]);
    let mut shake = Packet::new(vec![3, 7, 40, 25]);
    dispatch(&mut c, 3, &mut moveto);
    dispatch(&mut c, 167, &mut lookat);
    dispatch(&mut c, 67, &mut shake);
    c.game_loop();
    let count = allocations(|| {
        for _ in 0..1000 {
            dispatch(&mut c, 3, &mut moveto);
            dispatch(&mut c, 167, &mut lookat);
            dispatch(&mut c, 67, &mut shake);
            c.game_loop();
            let [x, y, z, pitch, yaw] = eye(&c);
            std::hint::black_box(renderer.cam_shake_jitter(&mut c, x, y, z, pitch, yaw));
        }
    });
    assert_eq!(
        count, 0,
        "camera packet/logic/render steady-state allocations"
    );
}

#[test]
fn malformed_camera_frames_fail_closed_without_camera_publication() {
    for (opcode, bytes) in [
        (3, vec![4, 5, 0, 20, 10]),
        (167, vec![4, 5, 0, 20, 10, 100, 1]),
        (67, vec![5, 7, 40, 25]),
    ] {
        let mut c = client();
        let mut p = Packet::new(bytes);
        c.psize = p.length() as i32;
        c.handle_packet(opcode, &mut p);
        assert!(!c.ingame);
        assert!(!c.cinema_cam);
        assert_eq!(c.cam_shake, [false; 5]);
        assert_eq!(eye(&c), [0; 5]);
        // Disconnect invalidates all slices once, but no apply publication occurs.
        assert_eq!(c.gens.camera, 1);
    }
}
