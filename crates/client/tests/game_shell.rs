use client::client::GameShell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn deltime_default_is_20() {
    let sh = GameShell::new();
    assert_eq!(sh.deltime, 20);
}

#[test]
fn set_framerate_1_is_error_screen_only() {
    let mut sh = GameShell::new();
    sh.set_framerate(1);
    assert_eq!(sh.deltime, 1000);
}

#[test]
fn run_increments_loop_not_server_tick() {
    let loops = Arc::new(AtomicU32::new(0));
    let mut sh = GameShell::new();
    sh.mindel = 0;
    // drive three mainloop iterations then stop — must not sleep 600ms
    let start = std::time::Instant::now();
    while loops.load(Ordering::SeqCst) < 3 {
        sh.pump_one(|shell| {
            let n = loops.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= 3 {
                shell.state = -1;
            }
        });
        if start.elapsed() > Duration::from_millis(400) {
            panic!("GameShell is not the 20ms machine");
        }
    }
    assert!(start.elapsed() < Duration::from_millis(400));
    assert_eq!(sh.state, -1);
}
