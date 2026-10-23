#![cfg(feature = "render-test-hooks")]

#[path = "common/gpu_serial.rs"]
mod gpu_serial;
#[path = "common/scene_fixture.rs"]
mod scene_fixture;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use client::render::backend::gpu::{
    current_scene_submit_result, set_scene_admission_hook, set_scene_workspace_hook,
    test_hooks::{self, SceneTestPoint},
    SceneAdmission, SceneSubmit,
};

static HOOKS: OnceLock<()> = OnceLock::new();

thread_local! {
    static ACTIVE_OBSERVATION: RefCell<Option<Arc<Observation>>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct Observation {
    workspace_calls: AtomicUsize,
    submit_calls: AtomicUsize,
    callback_result_was_not_run: AtomicBool,
    lease_live: AtomicBool,
    overlay_calls: AtomicUsize,
    overlay_saw_live_lease: AtomicBool,
}

fn admission_dispatcher() -> SceneAdmission {
    if let Some(observation) = active_observation() {
        observation.lease_live.store(true, Ordering::Release);
    }
    SceneAdmission::Rebuild
}

fn omitted_workspace(_submit: &mut dyn FnMut()) {
    if let Some(observation) = active_observation() {
        observation.workspace_calls.fetch_add(1, Ordering::Relaxed);
        observation.callback_result_was_not_run.store(
            current_scene_submit_result() == SceneSubmit::NotRun,
            Ordering::Release,
        );
        // This host callback deliberately declines to invoke the client
        // closure. A Rebuild must not add its own fallback after it returns.
        observation.lease_live.store(false, Ordering::Release);
    }
}

fn active_observation() -> Option<Arc<Observation>> {
    ACTIVE_OBSERVATION.with(|slot| slot.borrow().clone())
}

fn observe_overlay(point: SceneTestPoint) {
    if point != SceneTestPoint::OverlayEntry {
        return;
    }
    if let Some(observation) = active_observation() {
        observation.overlay_calls.fetch_add(1, Ordering::Relaxed);
        if observation.lease_live.load(Ordering::Acquire) {
            observation
                .overlay_saw_live_lease
                .store(true, Ordering::Release);
        }
    }
}

fn ensure_hooks() {
    HOOKS.get_or_init(|| {
        set_scene_admission_hook(admission_dispatcher);
        set_scene_workspace_hook(omitted_workspace);
    });
}

#[test]
fn omitted_host_submit_never_falls_back() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; omitted-host-submit test needs a GPU scene");
        return;
    }
    ensure_hooks();

    // Hold the real production scene mutex on this thread. A legacy fallback
    // tries the blocking standalone lock after the callback returns; the
    // correct Rebuild returns from this omitted callback while it is held.
    let scene_lock = test_hooks::hold_scene_lock();
    let observation = Arc::new(Observation::default());
    let worker_observation = Arc::clone(&observation);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
            let _ = ready_tx.send(false);
            return;
        };
        ACTIVE_OBSERVATION.with(|slot| {
            slot.replace(Some(worker_observation));
        });
        let _events = test_hooks::install_for_thread(observe_overlay);
        let _ = ready_tx.send(true);
        let frame = renderer.game_draw(&mut client);
        let _ = finished_tx.send(scene_fixture::terrain_pixel(frame));
    });

    let gpu_available = ready_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("GPU fixture must initialize or report an unavailable adapter");
    if !gpu_available {
        drop(scene_lock);
        worker.join().expect("unavailable GPU worker must exit");
        eprintln!("no adapter; omitted-host-submit test skips");
        return;
    }

    let pixel = finished_rx.recv_timeout(Duration::from_secs(10));
    let returned_while_locked = pixel.is_ok();
    drop(scene_lock);
    let pixel = pixel.unwrap_or_else(|_| {
        // Let a legacy blocking fallback finish after the lock is released,
        // so the assertion does not strand a render thread.
        finished_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the scene worker must finish after the lock is released")
    });
    worker.join().expect("omitted-submit worker must not panic");

    assert!(
        returned_while_locked,
        "Rebuild must not submit after the host callback omits its closure"
    );
    assert_eq!(
        pixel & 0xffffff,
        0,
        "an omitted Rebuild must not paint terrain"
    );
    assert_eq!(observation.workspace_calls.load(Ordering::Relaxed), 1);
    assert_eq!(observation.submit_calls.load(Ordering::Relaxed), 0);
    assert!(observation
        .callback_result_was_not_run
        .load(Ordering::Acquire));
    assert_eq!(observation.overlay_calls.load(Ordering::Relaxed), 1);
    assert!(!observation.lease_live.load(Ordering::Acquire));
    assert!(!observation.overlay_saw_live_lease.load(Ordering::Acquire));
    assert_eq!(current_scene_submit_result(), SceneSubmit::NotRun);
}
