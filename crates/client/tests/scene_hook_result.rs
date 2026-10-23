#![cfg(feature = "render-test-hooks")]

#[path = "common/gpu_serial.rs"]
mod gpu_serial;
#[path = "common/scene_fixture.rs"]
mod scene_fixture;

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use client::client::{Client, ClientPlayer};
use client::render::backend::gpu::{
    current_scene_submit_result, set_scene_admission_hook, set_scene_workspace_hook,
    test_hooks::{self, SceneTestPoint, ScopedSceneTestHook},
    SceneAdmission, SceneSubmit,
};

static HOOKS: LazyLock<()> = LazyLock::new(|| {
    set_scene_admission_hook(admission_dispatcher);
    set_scene_workspace_hook(workspace_dispatcher);
});

thread_local! {
    static ADMISSION: Cell<SceneAdmission> = const { Cell::new(SceneAdmission::Standalone) };
    static ACTIVE_RUN: RefCell<Option<Rc<RunContext>>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct Observation {
    workspace_calls: AtomicUsize,
    submit_calls: AtomicUsize,
    terminal_calls: AtomicUsize,
    resource_faults: AtomicUsize,
    terminal_results: Mutex<Vec<SceneSubmit>>,
    points: Mutex<Vec<SceneTestPoint>>,
    lease_live: AtomicBool,
    overlay_saw_live_lease: AtomicBool,
    overlay_lock_results: Mutex<Vec<bool>>,
    lock_probes: Mutex<Vec<JoinHandle<()>>>,
}

enum WorkspaceMode {
    Submit,
    PanicAfterSubmit,
    SubmitThenPause {
        submitted: Sender<()>,
        resume: Receiver<()>,
    },
}

struct RunContext {
    observation: Arc<Observation>,
    workspace: WorkspaceMode,
}

struct RunScope {
    previous_admission: SceneAdmission,
    previous_run: Option<Rc<RunContext>>,
}

impl RunScope {
    fn enter(admission: SceneAdmission, context: Rc<RunContext>) -> Self {
        let previous_admission = ADMISSION.replace(admission);
        let previous_run = ACTIVE_RUN.with(|slot| slot.replace(Some(context)));
        Self {
            previous_admission,
            previous_run,
        }
    }
}

impl Drop for RunScope {
    fn drop(&mut self) {
        ACTIVE_RUN.with(|slot| {
            slot.replace(self.previous_run.take());
        });
        ADMISSION.set(self.previous_admission);
    }
}

fn active_run() -> Option<Rc<RunContext>> {
    ACTIVE_RUN.with(|slot| slot.borrow().clone())
}

fn admission_dispatcher() -> SceneAdmission {
    let admission = ADMISSION.get();
    if admission == SceneAdmission::Rebuild {
        if let Some(run) = active_run() {
            run.observation.lease_live.store(true, Ordering::Release);
        }
    }
    admission
}

fn workspace_dispatcher(submit: &mut dyn FnMut()) {
    let Some(run) = active_run() else {
        submit();
        return;
    };
    let observation = &run.observation;
    observation.workspace_calls.fetch_add(1, Ordering::Relaxed);

    match &run.workspace {
        WorkspaceMode::Submit => {
            observation.submit_calls.fetch_add(1, Ordering::Relaxed);
            submit();
        }
        WorkspaceMode::SubmitThenPause { submitted, resume } => {
            observation.submit_calls.fetch_add(1, Ordering::Relaxed);
            submit();
            let _ = submitted.send(());
            let _ = resume.recv();
        }
        WorkspaceMode::PanicAfterSubmit => {
            submit();
            assert_eq!(current_scene_submit_result(), SceneSubmit::Submitted);
            panic!("intentional workspace unwind after a successful submit");
        }
    }

    let result = current_scene_submit_result();
    observation.terminal_calls.fetch_add(1, Ordering::Relaxed);
    if result == SceneSubmit::ResourceLimited {
        observation.resource_faults.fetch_add(1, Ordering::Relaxed);
    }
    observation
        .terminal_results
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(result);
    observation.lease_live.store(false, Ordering::Release);
}

fn record_scene_point(point: SceneTestPoint) {
    let Some(run) = active_run() else {
        return;
    };
    let observation = &run.observation;
    observation
        .points
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(point);

    if point == SceneTestPoint::OverlayEntry {
        if observation.lease_live.load(Ordering::Acquire) {
            observation
                .overlay_saw_live_lease
                .store(true, Ordering::Release);
        }

        // Probe the actual production mutex from another thread. A bounded
        // receive lets the scene continue if a regression still holds it
        // through overlay entry; the probe then finishes when the paint ends.
        let (acquired_tx, acquired_rx) = mpsc::sync_channel(1);
        let probe = thread::spawn(move || {
            let _guard = test_hooks::hold_scene_lock();
            let _ = acquired_tx.send(());
        });
        let acquired = acquired_rx.recv_timeout(Duration::from_millis(250)).is_ok();
        observation
            .overlay_lock_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(acquired);
        observation
            .lock_probes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(probe);
    }
}

fn panic_at_overlay(point: SceneTestPoint) {
    if point == SceneTestPoint::OverlayEntry {
        panic!("intentional scene-hook unwind");
    }
}

fn count_outer_hook(point: SceneTestPoint) {
    if let Some(run) = active_run() {
        run.observation
            .points
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(point);
    }
}

fn ensure_hooks() {
    let _ = &*HOOKS;
}

fn drain_lock_probes(observation: &Observation) {
    let probes = std::mem::take(
        &mut *observation
            .lock_probes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    );
    for probe in probes {
        probe.join().expect("overlay lock probe must finish");
    }
}

fn assert_single_resource_delivery(observation: &Observation) {
    assert_eq!(observation.workspace_calls.load(Ordering::Relaxed), 1);
    assert_eq!(observation.submit_calls.load(Ordering::Relaxed), 1);
    assert_eq!(observation.terminal_calls.load(Ordering::Relaxed), 1);
    assert_eq!(observation.resource_faults.load(Ordering::Relaxed), 1);
    assert_eq!(
        *observation
            .terminal_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        [SceneSubmit::ResourceLimited]
    );
    assert!(!observation.lease_live.load(Ordering::Acquire));
    assert!(!observation.overlay_saw_live_lease.load(Ordering::Acquire));
    let overlay_lock_results = observation
        .overlay_lock_results
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(overlay_lock_results.len(), 1);
    assert!(
        overlay_lock_results[0],
        "the real GPU scene lock must be available at overlay entry"
    );
}

#[test]
fn submit_result_is_local_to_the_invocation() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; invocation-local submit result needs a GPU scene");
        return;
    }
    ensure_hooks();

    let scene_lock = test_hooks::hold_scene_lock();
    let host_observation = Arc::new(Observation::default());
    let (submit_tx, submit_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);
    let worker_observation = Arc::clone(&host_observation);
    let host = thread::spawn(move || {
        let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
            let _ = ready_tx.send(false);
            return;
        };
        let context = Rc::new(RunContext {
            observation: worker_observation,
            workspace: WorkspaceMode::SubmitThenPause {
                submitted: submit_tx,
                resume: resume_rx,
            },
        });
        let _run = RunScope::enter(SceneAdmission::Rebuild, context);
        let _events = test_hooks::install_for_thread(record_scene_point);
        let _ = ready_tx.send(true);
        let _ = renderer.game_draw(&mut client);
        let _ = finished_tx.send(());
    });

    let gpu_available = ready_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("GPU fixture must initialize or report an unavailable adapter");
    if !gpu_available {
        drop(scene_lock);
        host.join().expect("unavailable GPU worker must exit");
        eprintln!("no adapter; invocation-local submit test skips");
        return;
    }

    let callback_ready = submit_rx.recv_timeout(Duration::from_secs(30));
    if callback_ready.is_err() {
        drop(scene_lock);
        let _ = resume_tx.send(());
        let _ = host.join();
        panic!("the Rebuild submit callback must return while the scene lock is held");
    }

    // The host callback has run the real Rebuild's try-lock path and is now
    // paused before it reads the result. Release its lock so a standalone
    // scene can successfully submit before that host read.
    drop(scene_lock);
    let standalone_observation = Arc::new(Observation::default());
    let Some((mut standalone_client, mut standalone_renderer)) = scene_fixture::gpu_scene() else {
        let _ = resume_tx.send(());
        let _ = host.join();
        panic!("the second GPU scene must reuse the available adapter");
    };
    let standalone_context = Rc::new(RunContext {
        observation: Arc::clone(&standalone_observation),
        workspace: WorkspaceMode::Submit,
    });
    let _standalone_run = RunScope::enter(SceneAdmission::Standalone, standalone_context);
    let _standalone_events = test_hooks::install_for_thread(record_scene_point);
    let _ = standalone_renderer.game_draw(&mut standalone_client);
    drain_lock_probes(&standalone_observation);
    assert_eq!(
        *standalone_observation
            .terminal_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        [SceneSubmit::Submitted]
    );

    resume_tx
        .send(())
        .expect("the host callback must still be waiting to read its result");
    finished_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the host scene must finish after its result is read");
    host.join().expect("host scene worker must not panic");
    drain_lock_probes(&host_observation);

    assert_eq!(host_observation.workspace_calls.load(Ordering::Relaxed), 1);
    assert_eq!(host_observation.submit_calls.load(Ordering::Relaxed), 1);
    assert_eq!(host_observation.terminal_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        *host_observation
            .terminal_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        [SceneSubmit::LockContended],
        "the host must read its own LockContended result after the standalone submit"
    );
    assert!(!host_observation.lease_live.load(Ordering::Acquire));
    assert!(!host_observation
        .overlay_saw_live_lease
        .load(Ordering::Acquire));
    assert_eq!(current_scene_submit_result(), SceneSubmit::NotRun);
}

#[test]
fn pre_submit_resource_failures_reach_terminal_once_and_roll_back() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; resource-failure rollback needs a GPU scene");
        return;
    }
    ensure_hooks();

    // Pin that the shared fixture really changes its ground-pick state on a
    // normal paint; the failure case below must roll that mutation back.
    let (mut pick_control, mut pick_control_renderer) =
        scene_fixture::gpu_scene().expect("adapter checked above");
    make_ready_local_player(&mut pick_control);
    arm_ground_click(&mut pick_control);
    assert_ne!(
        scene_fixture::terrain_pixel(pick_control_renderer.game_draw(&mut pick_control)),
        0,
        "the real terrain must be rendered"
    );
    assert_eq!(
        (pick_control.world.ground_x, pick_control.world.ground_z),
        (50, 51)
    );
    assert!(!pick_control.world.click);

    run_resource_failure(SceneTestPoint::AfterAdmission, false, false);
    run_resource_failure(SceneTestPoint::BeforeTerminal, true, false);
    run_resource_failure(SceneTestPoint::BeforeTerminal, true, true);
}

/// Use the same deterministic terrain click as `ground_input_composed`.
fn arm_ground_click(client: &mut Client) {
    client.world.click = true;
    client.world.click_x = 256;
    client.world.click_y = 134;
    client.world.ground_x = -1;
    client.world.ground_z = -1;
}

/// Materialize a renderable local player at the fixture's centre tile.
fn make_ready_local_player(client: &mut Client) {
    let mut player = ClientPlayer::at(50, 50);
    player.ready = true;
    player.entity.teleport(&client.cache, true, 50, 50);
    client.local_player = Some(player);
}

fn run_resource_failure(
    point: SceneTestPoint,
    materialize_sprite: bool,
    move_camera_during_prepare: bool,
) {
    let (mut client, mut renderer) = scene_fixture::gpu_scene().expect("adapter checked above");
    if materialize_sprite || move_camera_during_prepare {
        make_ready_local_player(&mut client);
    }
    client.cinema_cam = !move_camera_during_prepare;

    let camera_before = [1234, 2345, 3456, 456, 567];
    [
        client.cam_x,
        client.cam_y,
        client.cam_z,
        client.cam_pitch,
        client.cam_yaw,
    ] = camera_before;
    client.pick_count = 3;
    client.pick_typecodes[..3].copy_from_slice(&[701, 702, 703]);
    client.world.ground_x = -1;
    client.world.ground_z = -1;
    client.world.click = true;
    client.world.click_x = 256;
    client.world.click_y = 134;

    if point == SceneTestPoint::BeforeTerminal {
        // Capture uses this real dynamic sprite arena and PreparedScene must
        // return it before overlay entry on the injected terminal failure.
        client
            .world
            .add_dynamic(0, 50 * 128 + 64, 0, 50 * 128 + 64, 0, 0, 60, false)
            .expect("fixture dynamic sprite must fit the build area");
        assert_eq!(client.world.dynamic_count(), 1);
    }

    let observation = Arc::new(Observation::default());
    let context = Rc::new(RunContext {
        observation: Arc::clone(&observation),
        workspace: WorkspaceMode::Submit,
    });
    let _run = RunScope::enter(SceneAdmission::Rebuild, context);
    let _events = test_hooks::install_for_thread(record_scene_point);
    let _failure = test_hooks::resource_failure_at(point);
    let _ = renderer.game_draw(&mut client);
    drain_lock_probes(&observation);

    assert_single_resource_delivery(&observation);
    assert!(observation
        .points
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains(&point));
    assert_eq!(
        [
            client.cam_x,
            client.cam_y,
            client.cam_z,
            client.cam_pitch,
            client.cam_yaw,
        ],
        camera_before,
        "a failed prepared scene must restore its camera"
    );
    assert_eq!(
        (
            client.world.ground_x,
            client.world.ground_z,
            client.world.click
        ),
        (-1, -1, true),
        "a failed scene must restore the armed ground click and prior pick"
    );
    assert_eq!(client.pick_count, 3);
    assert_eq!(&client.pick_typecodes[..3], &[701, 702, 703]);
    assert!(renderer.pix3d.capture.is_none());
    assert_eq!(client.world.dynamic_count(), 0);
    assert_eq!(current_scene_submit_result(), SceneSubmit::NotRun);
}

#[test]
fn scoped_scene_hooks_restore_after_unwind() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; scoped hook unwind needs a GPU scene");
        return;
    }
    ensure_hooks();
    // The fixture's ordinary scene must change the camera so the failed
    // Rebuild below proves rollback rather than an unexercised code path.
    let (mut camera_control, mut camera_control_renderer) =
        scene_fixture::gpu_scene().expect("adapter checked above");
    make_ready_local_player(&mut camera_control);
    camera_control.cinema_cam = false;
    let camera_control_before = [
        camera_control.cam_x,
        camera_control.cam_y,
        camera_control.cam_z,
        camera_control.cam_pitch,
        camera_control.cam_yaw,
    ];
    let _ = camera_control_renderer.game_draw(&mut camera_control);
    assert_ne!(
        [
            camera_control.cam_x,
            camera_control.cam_y,
            camera_control.cam_z,
            camera_control.cam_pitch,
            camera_control.cam_yaw,
        ],
        camera_control_before,
        "the local-player fixture must exercise camera preparation"
    );

    let (mut client, mut renderer) = scene_fixture::gpu_scene().expect("adapter checked above");
    make_ready_local_player(&mut client);
    client.cinema_cam = false;
    let camera_before = [
        client.cam_x,
        client.cam_y,
        client.cam_z,
        client.cam_pitch,
        client.cam_yaw,
    ];

    let observation = Arc::new(Observation::default());
    let context = Rc::new(RunContext {
        observation: Arc::clone(&observation),
        workspace: WorkspaceMode::Submit,
    });
    let _run = RunScope::enter(SceneAdmission::Rebuild, context);
    let _outer_hook: ScopedSceneTestHook = test_hooks::install_for_thread(count_outer_hook);
    let outer_failure = test_hooks::resource_failure_at(SceneTestPoint::AfterAdmission);

    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _inner_hook = test_hooks::install_for_thread(panic_at_overlay);
        let _inner_failure = test_hooks::resource_failure_at(SceneTestPoint::BeforeTerminal);
        let _ = renderer.game_draw(&mut client);
    }));
    assert!(
        unwind.is_err(),
        "the inner overlay hook must unwind the paint"
    );
    assert_eq!(
        [
            client.cam_x,
            client.cam_y,
            client.cam_z,
            client.cam_pitch,
            client.cam_yaw
        ],
        camera_before,
        "the failed Rebuild must restore camera state while unwinding"
    );

    // The outer hook and AfterAdmission failure scope must both have been
    // restored when the inner scopes unwind. The outer fault is delivered
    // once, then dropping it permits a normal Rebuild submit.
    let _ = renderer.game_draw(&mut client);
    assert_eq!(
        observation
            .terminal_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last(),
        Some(&SceneSubmit::ResourceLimited)
    );
    drop(outer_failure);
    let _ = renderer.game_draw(&mut client);
    assert_eq!(
        observation
            .terminal_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last(),
        Some(&SceneSubmit::Submitted),
        "dropping the outer failure scope must restore ordinary scene submission"
    );
    assert_eq!(current_scene_submit_result(), SceneSubmit::NotRun);
    drain_lock_probes(&observation);
}

#[test]
fn submit_result_restores_after_workspace_unwind() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; result-scope unwind needs a GPU scene");
        return;
    }
    ensure_hooks();
    let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
        eprintln!("no adapter; result-scope unwind test skips");
        return;
    };
    let observation = Arc::new(Observation::default());
    let context = Rc::new(RunContext {
        observation: Arc::clone(&observation),
        workspace: WorkspaceMode::PanicAfterSubmit,
    });
    let _run = RunScope::enter(SceneAdmission::Standalone, context);
    let _events = test_hooks::install_for_thread(record_scene_point);
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _ = renderer.game_draw(&mut client);
    }));
    assert!(unwind.is_err());
    assert_eq!(current_scene_submit_result(), SceneSubmit::NotRun);
    assert!(
        observation
            .points
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty(),
        "unwind must not enter overlays"
    );
}

#[test]
fn scoped_hook_guards_unwind_without_gpu() {
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _hook = test_hooks::install_for_thread(count_outer_hook);
        let _failure = test_hooks::resource_failure_at(SceneTestPoint::AfterAdmission);
        panic!("exercise scoped hook destructors without initializing wgpu");
    }));
    assert!(unwind.is_err());

    // A later scope can still be installed on this thread after the unwind.
    let _hook = test_hooks::install_for_thread(count_outer_hook);
    let _failure = test_hooks::resource_failure_at(SceneTestPoint::BeforeTerminal);
}
