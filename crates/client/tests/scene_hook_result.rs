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

use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision};
use client::dash3d::TerrainOverlayShape;
use client::graphics::{PixFont, PixMap};
use client::render::backend::cpu::set_cpu_host_overlay_hook;
use client::render::backend::gpu::{
    current_scene_submit_result, set_scene_admission_hook, set_scene_workspace_hook,
    test_hooks::{self, SceneTestPoint, ScopedSceneTestHook},
    SceneAdmission, SceneSubmit,
};
use client::render::backend::{FrameOutput, SceneProgress};
use client::render::media::Media;
use client::render::Renderer;

static HOOKS: LazyLock<()> = LazyLock::new(|| {
    set_scene_admission_hook(admission_dispatcher);
    set_scene_workspace_hook(workspace_dispatcher);
    set_cpu_host_overlay_hook(host_overlays_dispatcher);
});

thread_local! {
    static ADMISSION: Cell<SceneAdmission> = const { Cell::new(SceneAdmission::Standalone) };
    static ACTIVE_RUN: RefCell<Option<Rc<RunContext>>> = const { RefCell::new(None) };
    static HOST_OVERLAYS: Cell<bool> = const { Cell::new(false) };
}

fn host_overlays_dispatcher() -> bool {
    HOST_OVERLAYS.get()
}

struct HostOverlayScope {
    previous_active: bool,
}

impl HostOverlayScope {
    fn enter(active: bool) -> Self {
        Self {
            previous_active: HOST_OVERLAYS.replace(active),
        }
    }
}

impl Drop for HostOverlayScope {
    fn drop(&mut self) {
        HOST_OVERLAYS.set(self.previous_active);
    }
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
    overlay_gate: Mutex<Option<(Sender<()>, Receiver<()>)>>,
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
        let overlay_gate = observation
            .overlay_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some((entered, resume)) = overlay_gate {
            let _ = entered.send(());
            let _ = resume.recv_timeout(Duration::from_secs(30));
        }

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

fn empty_client() -> Client {
    let cache_dir = format!(
        "{}/../../target/scene-progress-absent-cache-{}",
        env!("CARGO_MANIFEST_DIR"),
        std::process::id()
    );
    assert!(!std::path::Path::new(&cache_dir).exists());
    Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir,
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    )
}

fn submit_run_context() -> Rc<RunContext> {
    Rc::new(RunContext {
        observation: Arc::new(Observation::default()),
        workspace: WorkspaceMode::Submit,
    })
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
    let (host_submit_tx, host_submit_rx) = mpsc::channel();
    let (host_resume_tx, host_resume_rx) = mpsc::channel();
    let (host_ready_tx, host_ready_rx) = mpsc::sync_channel(1);
    let (host_finished_tx, host_finished_rx) = mpsc::sync_channel(1);
    let host_worker_observation = Arc::clone(&host_observation);
    let host = thread::spawn(move || {
        let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
            let _ = host_ready_tx.send(false);
            return;
        };
        let context = Rc::new(RunContext {
            observation: host_worker_observation,
            workspace: WorkspaceMode::SubmitThenPause {
                submitted: host_submit_tx,
                resume: host_resume_rx,
            },
        });
        let _run = RunScope::enter(SceneAdmission::Rebuild, context);
        let _events = test_hooks::install_for_thread(record_scene_point);
        let _ = host_ready_tx.send(true);
        let _ = renderer.game_draw(&mut client);
        let _ = host_finished_tx.send(());
    });

    match host_ready_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(true) => {}
        Ok(false) => {
            drop(scene_lock);
            host.join().expect("unavailable GPU host worker must exit");
            eprintln!("no adapter; invocation-local submit test skips");
            return;
        }
        Err(_) => {
            drop(scene_lock);
            let _ = host_resume_tx.send(());
            let _ = host.join();
            panic!("the GPU host worker must initialize its scene");
        }
    }

    if host_submit_rx
        .recv_timeout(Duration::from_secs(30))
        .is_err()
    {
        drop(scene_lock);
        let _ = host_resume_tx.send(());
        let _ = host.join();
        panic!("the Rebuild callback must reach its result pause while the scene lock is held");
    }

    // The host has stored LockContended and now waits before reading it.
    drop(scene_lock);
    let standalone_observation = Arc::new(Observation::default());
    let (standalone_submit_tx, standalone_submit_rx) = mpsc::channel();
    let (standalone_resume_tx, standalone_resume_rx) = mpsc::channel();
    let (standalone_ready_tx, standalone_ready_rx) = mpsc::sync_channel(1);
    let (standalone_finished_tx, standalone_finished_rx) = mpsc::sync_channel(1);
    let standalone_worker_observation = Arc::clone(&standalone_observation);
    let standalone = thread::spawn(move || {
        let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
            let _ = standalone_ready_tx.send(false);
            return;
        };
        let context = Rc::new(RunContext {
            observation: standalone_worker_observation,
            workspace: WorkspaceMode::SubmitThenPause {
                submitted: standalone_submit_tx,
                resume: standalone_resume_rx,
            },
        });
        let _run = RunScope::enter(SceneAdmission::Standalone, context);
        let _events = test_hooks::install_for_thread(record_scene_point);
        let _ = standalone_ready_tx.send(true);
        let _ = renderer.game_draw(&mut client);
        let _ = standalone_finished_tx.send(());
    });

    match standalone_ready_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(true) => {}
        Ok(false) => {
            let _ = host_resume_tx.send(());
            host.join().expect("the host worker must finish");
            standalone
                .join()
                .expect("unavailable standalone worker must exit");
            panic!("the second GPU scene must reuse the available adapter");
        }
        Err(_) => {
            let _ = standalone_resume_tx.send(());
            let _ = host_resume_tx.send(());
            let _ = standalone.join();
            let _ = host.join();
            panic!("the standalone GPU worker must initialize its scene");
        }
    }

    if standalone_submit_rx
        .recv_timeout(Duration::from_secs(30))
        .is_err()
    {
        let _ = standalone_resume_tx.send(());
        let _ = host_resume_tx.send(());
        let _ = standalone.join();
        let _ = host.join();
        panic!("the standalone callback must submit and pause before it reads its result");
    }

    // The standalone callback now waits after its successful submit. Resume
    // the host and read LockContended while that invocation is still active.
    host_resume_tx
        .send(())
        .expect("the host callback must still be waiting to read its result");
    host_finished_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the host scene must finish after its result is read");
    host.join().expect("the host scene worker must not panic");

    standalone_resume_tx
        .send(())
        .expect("the standalone callback must still be paused");
    standalone_finished_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the standalone scene must finish after its callback resumes");
    standalone
        .join()
        .expect("the standalone scene worker must not panic");

    drain_lock_probes(&standalone_observation);
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
    assert_eq!(
        *standalone_observation
            .terminal_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        [SceneSubmit::Submitted]
    );
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

#[test]
fn prepared_submit_failure_removes_sprites_and_restores_camera() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; LockContended rollback needs a GPU scene");
        return;
    }
    ensure_hooks();

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
        "the fixture must change the camera during scene preparation"
    );

    let scene_lock = test_hooks::hold_scene_lock();
    let observation = Arc::new(Observation::default());
    let (overlay_entered_tx, overlay_entered_rx) = mpsc::channel();
    let (overlay_resume_tx, overlay_resume_rx) = mpsc::sync_channel(1);
    *observation
        .overlay_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some((overlay_entered_tx, overlay_resume_rx));

    let worker_observation = Arc::clone(&observation);
    let camera_before = [1234, 2345, 3456, 456, 567];
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
            let _ = ready_tx.send(false);
            return;
        };
        make_ready_local_player(&mut client);
        client.cinema_cam = false;
        [
            client.cam_x,
            client.cam_y,
            client.cam_z,
            client.cam_pitch,
            client.cam_yaw,
        ] = camera_before;
        client.pick_count = 3;
        client.pick_typecodes[..3].copy_from_slice(&[701, 702, 703]);
        arm_ground_click(&mut client);
        client
            .world
            .add_dynamic(0, 50 * 128 + 64, 0, 50 * 128 + 64, 0, 0, 60, false)
            .expect("fixture dynamic sprite must fit the build area");
        assert_eq!(client.world.dynamic_count(), 1);

        let context = Rc::new(RunContext {
            observation: Arc::clone(&worker_observation),
            workspace: WorkspaceMode::Submit,
        });
        let _run = RunScope::enter(SceneAdmission::Rebuild, context);
        let _events = test_hooks::install_for_thread(record_scene_point);
        let _ = ready_tx.send(true);
        let _ = renderer.game_draw(&mut client);
        drain_lock_probes(&worker_observation);

        let snapshot = (
            [
                client.cam_x,
                client.cam_y,
                client.cam_z,
                client.cam_pitch,
                client.cam_yaw,
            ],
            (
                client.world.ground_x,
                client.world.ground_z,
                client.world.click,
            ),
            client.pick_count,
            [
                client.pick_typecodes[0],
                client.pick_typecodes[1],
                client.pick_typecodes[2],
            ],
            renderer.pix3d.capture.is_none(),
            client.world.dynamic_count(),
            current_scene_submit_result(),
        );
        let _ = finished_tx.send(snapshot);
    });

    match ready_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(true) => {}
        Ok(false) => {
            drop(scene_lock);
            let _ = overlay_resume_tx.send(());
            worker.join().expect("unavailable GPU worker must exit");
            eprintln!("no adapter; LockContended rollback test skips");
            return;
        }
        Err(_) => {
            drop(scene_lock);
            let _ = overlay_resume_tx.send(());
            let _ = worker.join();
            panic!("the LockContended worker must initialize its GPU fixture");
        }
    }

    if overlay_entered_rx
        .recv_timeout(Duration::from_secs(30))
        .is_err()
    {
        drop(scene_lock);
        let _ = overlay_resume_tx.send(());
        let _ = worker.join();
        panic!("LockContended cleanup must reach overlay entry while the lock is held");
    }
    drop(scene_lock);
    overlay_resume_tx
        .send(())
        .expect("the overlay hook must wait for the lock release");
    let snapshot = finished_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the LockContended paint must finish after overlay entry");
    worker
        .join()
        .expect("the LockContended worker must not panic");

    assert_eq!(observation.workspace_calls.load(Ordering::Relaxed), 1);
    assert_eq!(observation.submit_calls.load(Ordering::Relaxed), 1);
    assert_eq!(observation.terminal_calls.load(Ordering::Relaxed), 1);
    assert_eq!(observation.resource_faults.load(Ordering::Relaxed), 0);
    assert_eq!(
        *observation
            .terminal_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        [SceneSubmit::LockContended]
    );
    assert!(!observation.lease_live.load(Ordering::Acquire));
    assert!(!observation.overlay_saw_live_lease.load(Ordering::Acquire));
    assert_eq!(
        *observation
            .overlay_lock_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        [true],
        "the scene lock must be available at overlay entry after the holder releases it"
    );
    assert_eq!(
        snapshot.0, camera_before,
        "failed preparation restores camera"
    );
    assert_eq!(
        snapshot.1,
        (-1, -1, true),
        "failed preparation restores the armed ground click"
    );
    assert_eq!(snapshot.2, 3);
    assert_eq!(snapshot.3, [701, 702, 703]);
    assert!(snapshot.4, "failed preparation clears the capture state");
    assert_eq!(
        snapshot.5, 0,
        "failed preparation removes temporary sprites"
    );
    assert_eq!(snapshot.6, SceneSubmit::NotRun);
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
    client.orbit_camera_x = player.x;
    client.orbit_camera_z = player.z;
    client.orbit_camera_pitch = 512;
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
        "the failed Rebuild must restore camera before overlay entry"
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
fn prepare_cpu_scene(client: &mut Client) {
    client.ingame = true;
    client.draw = true;
    client.scene_state = 2;
    client.local_player = Some(ClientPlayer::at(50, 50));
    client.cinema_cam = true;
    client.cam_x = 50 * 128 + 64;
    client.cam_z = 50 * 128 + 64;
    client.cam_y = 0;
    client.cam_pitch = 512;
    client.cam_yaw = 0;
    client.world = client::core::World::new(vec![vec![vec![2000; 105]; 105]; 4], 104, 4, 104);
    client.world.fill_base_level(0);
    for x in 49..=51 {
        for z in 49..=51 {
            client.world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                2000,
                2000,
                2000,
                2000,
                25700,
                25700,
                25700,
                25700,
                25700,
                25700,
                25700,
                25700,
                0,
                0,
            );
        }
    }
}

fn output_pixel(output: FrameOutput, x: usize, y: usize) -> i32 {
    let FrameOutput::Texture(handle) = output else {
        panic!("the GPU scene fixture must return a texture frame");
    };
    let width = handle.width() as usize;
    handle.read_back()[y * width + x]
}

#[test]
fn cpu_scene_progress_tracks_actual_scene_state() {
    let _gpu_serial = gpu_serial::lock();
    let mut client = empty_client();
    prepare_cpu_scene(&mut client);
    let mut renderer = Renderer::new(false);
    renderer.area_game = Some(PixMap::new(512, 334));

    let _ = renderer.game_draw(&mut client);
    assert_eq!(renderer.scene_progress(), SceneProgress::Rebuilt);
    assert!(renderer.has_valid_scene());

    client.scene_state = 1;
    let _ = renderer.game_draw(&mut client);
    assert_eq!(renderer.scene_progress(), SceneProgress::Retained);
    assert!(renderer.has_valid_scene());

    client.scene_state = 0;
    let _ = renderer.game_draw(&mut client);
    assert_eq!(renderer.scene_progress(), SceneProgress::NoScene);
    assert!(!renderer.has_valid_scene());

    client.scene_state = 2;
    let _ = renderer.game_draw(&mut client);
    assert_eq!(renderer.scene_progress(), SceneProgress::Rebuilt);
    renderer.title_screen_draw(&mut client);
    assert_eq!(renderer.scene_progress(), SceneProgress::NoScene);
    assert!(!renderer.has_valid_scene());
}

fn loading_font() -> (PixFont, usize, usize) {
    let text = "Loading - please wait.";
    let mut font = PixFont::new();
    font.height = 1;
    for byte in text.bytes() {
        let code = byte as usize;
        font.char_advance[code] = 2;
        if byte != b' ' {
            font.char_mask[code] = vec![1];
            font.char_mask_width[code] = 1;
            font.char_mask_height[code] = 1;
        }
    }
    let x = 4 + (256 - font.string_wid(Some(text)) / 2) as usize;
    let y = 4 + (150 - font.height) as usize;
    (font, x, y)
}

#[test]
fn host_loading_overlay_requires_a_valid_scene_and_coverage() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; host loading overlay needs a GPU scene");
        return;
    }
    ensure_hooks();
    let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
        eprintln!("no adapter; host loading overlay test skips");
        return;
    };
    // First-game preparation reloads the shared media. Install the synthetic
    // font only after that setup, without creating a valid scene.
    client.scene_state = 0;
    let _ = renderer.game_draw(&mut client);
    let (font, splash_x, splash_y) = loading_font();
    let mut media = Media::empty();
    media.p12 = Some(font);
    renderer.media = Arc::new(media);

    client.scene_state = 1;
    let pending = {
        let _host = HostOverlayScope::enter(true);
        let _run = RunScope::enter(SceneAdmission::Rebuild, submit_run_context());
        renderer.game_draw(&mut client)
    };
    assert_eq!(renderer.scene_progress(), SceneProgress::NoScene);
    assert!(!renderer.has_valid_scene());
    assert_eq!(
        output_pixel(pending, splash_x, splash_y),
        0x00ff_ffff,
        "the no-scene host loading splash must remain visible"
    );

    client.scene_state = 2;
    let rebuilt = {
        let _host = HostOverlayScope::enter(true);
        let _run = RunScope::enter(SceneAdmission::Rebuild, submit_run_context());
        renderer.game_draw(&mut client)
    };
    assert_eq!(renderer.scene_progress(), SceneProgress::Rebuilt);
    assert!(renderer.has_valid_scene());
    let scene_pixel = output_pixel(rebuilt, splash_x, splash_y);

    client.scene_state = 1;
    let retained = {
        let _host = HostOverlayScope::enter(true);
        let _run = RunScope::enter(SceneAdmission::Retained, submit_run_context());
        renderer.game_draw(&mut client)
    };
    assert_eq!(renderer.scene_progress(), SceneProgress::Retained);
    assert!(renderer.has_valid_scene());
    assert_eq!(
        output_pixel(retained, splash_x, splash_y),
        0x00ff_ffff,
        "the loading splash must be present inside retained-scene coverage"
    );

    let standalone = {
        let _host = HostOverlayScope::enter(false);
        let _run = RunScope::enter(SceneAdmission::Standalone, submit_run_context());
        renderer.game_draw(&mut client)
    };
    assert_eq!(renderer.scene_progress(), SceneProgress::Retained);
    assert_eq!(
        output_pixel(standalone, splash_x, splash_y),
        scene_pixel,
        "standalone state-1 rendering must preserve the FBO without host overlays"
    );

    client.scene_state = 0;
    let _ = {
        let _host = HostOverlayScope::enter(true);
        let _run = RunScope::enter(SceneAdmission::Rebuild, submit_run_context());
        renderer.game_draw(&mut client)
    };
    assert_eq!(renderer.scene_progress(), SceneProgress::NoScene);
    assert!(!renderer.has_valid_scene());

    client.scene_state = 2;
    let _ = {
        let _host = HostOverlayScope::enter(true);
        let _run = RunScope::enter(SceneAdmission::Rebuild, submit_run_context());
        renderer.game_draw(&mut client)
    };
    assert_eq!(renderer.scene_progress(), SceneProgress::Rebuilt);
    let _ = {
        let _host = HostOverlayScope::enter(true);
        renderer.title_screen_draw(&mut client)
    };
    assert_eq!(renderer.scene_progress(), SceneProgress::NoScene);
    assert!(!renderer.has_valid_scene());
}

#[test]
fn busy_click_and_trace_wait_for_a_real_scene() {
    const CHILD_MODE: &str = "R274_BUSY_HOST_TRACE_CHILD";

    if std::env::var_os(CHILD_MODE).is_none() {
        if gpu_serial::skipped() {
            eprintln!("SKIP_GPU=1; host trace test needs a GPU scene");
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "busy_click_and_trace_wait_for_a_real_scene",
                "--nocapture",
            ])
            .env(CHILD_MODE, "1")
            .env("CLIENT_289_GROUND_TRACE", "1")
            .output()
            .expect("spawn the isolated trace-enabled scene test");
        assert!(
            output.status.success(),
            "trace-enabled child failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let trace = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            trace.matches("stage=scene ").count(),
            1,
            "only the successful rebuild may consume the scene trace: {trace}"
        );
        assert_eq!(
            trace.matches("stage=post_render ").count(),
            1,
            "busy retained paints must not emit post-render trace events: {trace}"
        );
        assert!(
            !trace.contains("reason=event_cap"),
            "busy paints must not exhaust the trace event cap: {trace}"
        );
        return;
    }

    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        return;
    }
    ensure_hooks();
    let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
        eprintln!("no adapter; host trace child skips");
        return;
    };

    client.shell.apply_mouse_move(260, 138);
    {
        let _host = HostOverlayScope::enter(true);
        let _run = RunScope::enter(SceneAdmission::Rebuild, submit_run_context());
        let _ = renderer.game_draw(&mut client);
    }
    assert_eq!(renderer.scene_progress(), SceneProgress::Rebuilt);
    client.shell.apply_mouse_down(1, 260, 138);
    client.shell.latch_click();
    client.game_loop();
    assert!(client.world.click);
    assert_eq!((client.world.click_x, client.world.click_y), (256, 134));
    assert_eq!(client.world.ground_x, -1);

    let scene_cycle = renderer.scene_cycle;
    {
        let _host = HostOverlayScope::enter(true);
        for _ in 0..65 {
            let _run = RunScope::enter(SceneAdmission::Retained, submit_run_context());
            let _ = renderer.game_draw(&mut client);
            assert_eq!(renderer.scene_progress(), SceneProgress::Retained);
            assert_eq!(renderer.scene_cycle, scene_cycle);
            assert!(client.world.click);
            assert_eq!(client.world.ground_x, -1);
        }
    }

    {
        let _host = HostOverlayScope::enter(true);
        let _run = RunScope::enter(SceneAdmission::Rebuild, submit_run_context());
        let _ = renderer.game_draw(&mut client);
    }
    assert_eq!(renderer.scene_progress(), SceneProgress::Rebuilt);
}

fn cpu_frame(output: FrameOutput) -> PixMap {
    let FrameOutput::PixMap(frame) = output else {
        panic!("the CPU driver must return a complete pixel map");
    };
    frame
}

fn freeze_modal(client: &mut Client, x: i32, y: i32) {
    use client::config::if_type::{ComponentType, IfType, IfTypeMut};
    const ROOT: i32 = 700;
    const RECT: i32 = 701;
    client.set_iface(
        ROOT as usize,
        IfType {
            id: ROOT,
            r#type: ComponentType::TYPE_LAYER,
            width: 512,
            height: 334,
            children: Some(vec![RECT]),
            child_x: Some(vec![x]),
            child_y: Some(vec![y]),
            ..Default::default()
        },
    );
    client.set_iface(
        RECT as usize,
        IfType {
            id: RECT,
            r#type: ComponentType::TYPE_RECT,
            width: 8,
            height: 6,
            fill: true,
            ..Default::default()
        },
    );
    client.set_iface_mut(
        RECT as usize,
        IfTypeMut {
            colour: 0x0012_3456,
            ..Default::default()
        },
    );
    client.main_modal_id = ROOT;
}

#[test]
fn cpu_freeze_appear_move_remove_restores_clean_scene() {
    let _serial = gpu_serial::lock();
    ensure_hooks();
    let _host = HostOverlayScope::enter(true);
    let mut client = empty_client();
    prepare_cpu_scene(&mut client);
    client.advance_hosted_animations(false);
    let mut renderer = Renderer::new(false);
    let _ = cpu_frame(renderer.mainredraw(&mut client));
    assert!(renderer.has_valid_scene());
    assert_eq!(renderer.retained_storage_bytes(), 512 * 334 * 4);
    assert_eq!(renderer.retained_storage_allocations(), 1);

    let (font, splash_x, splash_y) = loading_font();
    let mut media = Media::empty();
    media.p12 = Some(font);
    renderer.media = Arc::new(media);
    client.scene_state = 1;
    let baseline = cpu_frame(renderer.mainredraw(&mut client));
    assert_eq!(
        baseline.pixels[splash_y * baseline.width as usize + splash_x],
        0x00ff_ffff
    );
    assert_eq!(renderer.scene_progress(), SceneProgress::Retained);
    let clean_loading = renderer.area_game.as_ref().unwrap().pixels.clone();
    let scene_cycle = renderer.scene_cycle;
    let camera = [121, 222, 323, 224, 525];
    [
        client.cam_x,
        client.cam_y,
        client.cam_z,
        client.cam_pitch,
        client.cam_yaw,
    ] = camera;

    freeze_modal(&mut client, 10, 20);
    let visible = cpu_frame(renderer.mainredraw(&mut client));
    assert_eq!(
        visible.pixels[24 * visible.width as usize + 14] & 0xffffff,
        0x0012_3456
    );
    freeze_modal(&mut client, 30, 20);
    let moved = cpu_frame(renderer.mainredraw(&mut client));
    assert_eq!(
        moved.pixels[24 * moved.width as usize + 14],
        baseline.pixels[24 * baseline.width as usize + 14]
    );
    assert_eq!(
        moved.pixels[24 * moved.width as usize + 34] & 0xffffff,
        0x0012_3456
    );
    client.main_modal_id = -1;
    let removed = cpu_frame(renderer.mainredraw(&mut client));
    assert_eq!(renderer.area_game.as_ref().unwrap().pixels, clean_loading);
    assert_eq!(
        removed.pixels[24 * removed.width as usize + 34],
        baseline.pixels[24 * baseline.width as usize + 34]
    );
    assert_eq!(
        [
            client.cam_x,
            client.cam_y,
            client.cam_z,
            client.cam_pitch,
            client.cam_yaw
        ],
        camera,
        "overlay projection must restore the current simulation camera"
    );
    assert_eq!(
        renderer.scene_cycle, scene_cycle,
        "loading must not prepare a new 3D scene"
    );
    assert_eq!(renderer.retained_storage_allocations(), 1);
    assert_eq!(renderer.retained_storage_bytes(), 684_032);
}

#[test]
fn cpu_title_invalidates_scene_without_reallocating_storage() {
    let _serial = gpu_serial::lock();
    ensure_hooks();
    let _host = HostOverlayScope::enter(true);
    let mut client = empty_client();
    prepare_cpu_scene(&mut client);
    client.scene_state = 0;
    let mut renderer = Renderer::new(false);
    let _ = cpu_frame(renderer.game_draw(&mut client));
    renderer
        .area_game
        .as_mut()
        .unwrap()
        .pixels
        .fill(0x0033_ff10);
    let (font, splash_x, splash_y) = loading_font();
    let mut media = Media::empty();
    media.p12 = Some(font);
    renderer.media = Arc::new(media);
    client.scene_state = 1;
    let pending = cpu_frame(renderer.mainredraw(&mut client));
    assert_eq!(
        pending.pixels[splash_y * pending.width as usize + splash_x],
        0x00ff_ffff
    );
    assert_eq!(renderer.area_game.as_ref().unwrap().pixels[0], 0);
    assert!(!renderer.has_valid_scene());
    assert_eq!(renderer.retained_storage_allocations(), 0);
    assert_eq!(renderer.retained_storage_bytes(), 0);

    client.scene_state = 2;
    let _ = cpu_frame(renderer.mainredraw(&mut client));
    assert!(renderer.has_valid_scene());
    assert_eq!(renderer.retained_storage_allocations(), 1);
    let old_area = renderer.area_game.as_ref().unwrap().pixels.clone();
    client.scene_state = 0;
    let _ = cpu_frame(renderer.game_draw(&mut client));
    assert!(!renderer.has_valid_scene());
    assert_eq!(renderer.area_game.as_ref().unwrap().pixels, old_area);
    assert_eq!(renderer.retained_storage_allocations(), 1);
    let _ = renderer.title_screen_draw(&mut client);
    assert!(!renderer.has_valid_scene());
    assert_eq!(renderer.retained_storage_bytes(), 684_032);
    assert_eq!(renderer.retained_storage_allocations(), 1);

    client.scene_state = 0;
    let _ = cpu_frame(renderer.game_draw(&mut client));
    let (font, splash_x, splash_y) = loading_font();
    let mut media = Media::empty();
    media.p12 = Some(font);
    renderer.media = Arc::new(media);
    renderer
        .area_game
        .as_mut()
        .unwrap()
        .pixels
        .fill(0x0033_ff10);
    client.scene_state = 1;
    let pending = cpu_frame(renderer.mainredraw(&mut client));
    assert!(!renderer.has_valid_scene());
    assert_eq!(renderer.area_game.as_ref().unwrap().pixels[0], 0);
    assert_eq!(
        pending.pixels[splash_y * pending.width as usize + splash_x],
        0x00ff_ffff
    );
    assert_eq!(renderer.retained_storage_allocations(), 1);
    client.scene_state = 2;
    let _ = cpu_frame(renderer.mainredraw(&mut client));
    assert!(renderer.has_valid_scene());
    assert_eq!(renderer.retained_storage_allocations(), 1);
}

#[test]
fn standalone_cpu_keeps_legacy_freeze_without_clean_copy() {
    let _serial = gpu_serial::lock();
    ensure_hooks();
    let _standalone = HostOverlayScope::enter(false);
    let mut client = empty_client();
    prepare_cpu_scene(&mut client);
    let mut renderer = Renderer::new_prefer(false, false);
    let live = cpu_frame(renderer.mainredraw(&mut client));
    assert!(renderer.has_valid_scene());
    assert_eq!(renderer.retained_storage_allocations(), 0);
    assert_eq!(renderer.retained_storage_bytes(), 0);
    renderer
        .area_game
        .as_mut()
        .unwrap()
        .pixels
        .fill(0x0044_6611);
    client.scene_state = 1;
    let frozen = cpu_frame(renderer.game_draw(&mut client));
    assert_eq!(
        frozen.pixels, live.pixels,
        "standalone loading must keep the legacy last-frame blit policy"
    );
    assert!(renderer
        .area_game
        .as_ref()
        .unwrap()
        .pixels
        .iter()
        .all(|&pixel| pixel == 0x0044_6611));
    assert_eq!(renderer.retained_storage_allocations(), 0);
    assert_eq!(renderer.retained_storage_bytes(), 0);
}

#[test]
fn cpu_distinct_heads_retain_independent_clean_scenes() {
    let _serial = gpu_serial::lock();
    ensure_hooks();
    let _host = HostOverlayScope::enter(true);
    let mut first_client = empty_client();
    let mut second_client = empty_client();
    prepare_cpu_scene(&mut first_client);
    prepare_cpu_scene(&mut second_client);
    for x in 49..=51 {
        for z in 49..=51 {
            second_client.world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                2000,
                2000,
                2000,
                2000,
                40300,
                40300,
                40300,
                40300,
                40300,
                40300,
                40300,
                40300,
                0,
                0,
            );
        }
    }
    let mut first = Renderer::new_prefer(false, false);
    let mut second = Renderer::new_prefer(false, false);
    for _ in 0..3 {
        let _ = cpu_frame(first.mainredraw(&mut first_client));
        let _ = cpu_frame(second.mainredraw(&mut second_client));
        assert_eq!(first.retained_storage_allocations(), 1);
        assert_eq!(second.retained_storage_allocations(), 1);
    }
    let first_clean = first.area_game.as_ref().unwrap().pixels.clone();
    let second_clean = second.area_game.as_ref().unwrap().pixels.clone();
    assert!(first_clean.iter().any(|&pixel| pixel != 0));
    assert!(second_clean.iter().any(|&pixel| pixel != 0));
    assert_ne!(
        first_clean, second_clean,
        "different live terrain must produce distinct retained heads"
    );
    first.media = Arc::new(Media::empty());
    second.media = Arc::new(Media::empty());
    first_client.scene_state = 1;
    second_client.scene_state = 1;
    let first_cycle = first.scene_cycle;
    let second_cycle = second.scene_cycle;
    for _ in 0..3 {
        first.area_game.as_mut().unwrap().pixels.fill(0x0055_7711);
        second.area_game.as_mut().unwrap().pixels.fill(0x0011_5577);
        let _ = cpu_frame(first.mainredraw(&mut first_client));
        let _ = cpu_frame(second.mainredraw(&mut second_client));
        assert_eq!(first.area_game.as_ref().unwrap().pixels, first_clean);
        assert_eq!(second.area_game.as_ref().unwrap().pixels, second_clean);
        assert_eq!(first.retained_storage_allocations(), 1);
        assert_eq!(second.retained_storage_allocations(), 1);
        assert_eq!(first.scene_cycle, first_cycle);
        assert_eq!(second.scene_cycle, second_cycle);
    }
}

#[test]
fn cpu_resource_failure_publishes_clean_scene_with_current_overlays_and_fault() {
    use client::render::backend::RenderFault;
    use client::render::budget::{RenderBudget, RenderBudgetLimits, RenderResource};

    let _serial = gpu_serial::lock();
    ensure_hooks();
    let _host = HostOverlayScope::enter(true);
    let mut client = empty_client();
    prepare_cpu_scene(&mut client);
    let mut renderer = Renderer::new_prefer(false, false);
    let baseline = cpu_frame(renderer.mainredraw(&mut client));
    assert!(renderer.has_valid_scene());
    assert_eq!(renderer.render_fault(), None);
    let retained_bytes = renderer.retained_storage_bytes();
    let retained_allocations = renderer.retained_storage_allocations();

    // Existing scene buffers must be admitted before any new head-misc work.
    // A zero fixture ceiling rejects the next real CPU scene preparation.
    let unlimited = usize::MAX;
    let budget = RenderBudget::fixture(RenderBudgetLimits::new(
        unlimited, unlimited, unlimited, unlimited, unlimited, unlimited, 0, unlimited, unlimited,
        unlimited, unlimited, unlimited, unlimited,
    ));
    let _budget_scope = budget.scope();
    for x in [10, 30] {
        freeze_modal(&mut client, x, 20);
        renderer.area_backbase1 = Some(PixMap::new(479, 50));
        renderer.area_backbase1.as_mut().unwrap().pixels[0] = 0x0055_aa33;
        client.redraw_chat_mode = true;
        let recovered = cpu_frame(renderer.mainredraw(&mut client));
        assert_eq!(renderer.scene_progress(), SceneProgress::Retained);
        assert!(renderer.has_valid_scene());
        assert!(matches!(
            renderer.render_fault(),
            Some(RenderFault::Budget(limit)) if limit.resource() == RenderResource::HeadMisc
        ));
        assert_eq!(
            recovered.pixels[24 * recovered.width as usize + (x + 4) as usize] & 0xffffff,
            0x0012_3456,
            "a resource fault must not hide the current viewport overlay",
        );
        assert!(!client.redraw_chat_mode, "recovery must still run chrome");
        assert_eq!(
            recovered.pixels[453 * recovered.width as usize],
            0x0055_aa33,
            "chrome changes must reach the usable recovered output",
        );
        if x == 30 {
            assert_eq!(
                recovered.pixels[24 * recovered.width as usize + 14],
                baseline.pixels[24 * baseline.width as usize + 14],
                "recovery must erase the previous overlay with the clean scene",
            );
        }
        assert_eq!(renderer.retained_storage_bytes(), retained_bytes);
        assert_eq!(
            renderer.retained_storage_allocations(),
            retained_allocations
        );
    }
}

#[test]
fn cpu_nav_input_refusal_keeps_current_overlays_on_the_retained_scene() {
    use client::render::backend::RenderFault;
    use client::render::budget::{RenderLimit, RenderResource};

    let _serial = gpu_serial::lock();
    ensure_hooks();
    let _host = HostOverlayScope::enter(true);
    let mut client = empty_client();
    prepare_cpu_scene(&mut client);
    let mut renderer = Renderer::new_prefer(false, false);
    let _ = cpu_frame(renderer.mainredraw(&mut client));
    let scene_cycle = renderer.scene_cycle;
    let fault = RenderLimit::new(RenderResource::NavInput, 2, 1);
    assert_eq!(client.set_budgeted_nav_debug_paint(Err(fault)), Err(fault));
    freeze_modal(&mut client, 10, 20);
    let recovered = cpu_frame(renderer.mainredraw(&mut client));
    assert_eq!(renderer.render_fault(), Some(RenderFault::Budget(fault)));
    assert_eq!(renderer.scene_progress(), SceneProgress::Retained);
    assert_eq!(renderer.scene_cycle, scene_cycle);
    assert_eq!(
        recovered.pixels[24 * recovered.width as usize + 14] & 0xffffff,
        0x0012_3456,
    );
    assert_eq!(renderer.retained_storage_allocations(), 1);
}

#[test]
fn cpu_nav_input_refusal_without_a_scene_publishes_pending_scene_and_overlays() {
    use client::render::backend::RenderFault;
    use client::render::budget::{RenderLimit, RenderResource};

    let _serial = gpu_serial::lock();
    ensure_hooks();
    let _host = HostOverlayScope::enter(true);
    let mut client = empty_client();
    prepare_cpu_scene(&mut client);
    client.scene_state = 0;
    let mut renderer = Renderer::new_prefer(false, false);
    let _ = cpu_frame(renderer.mainredraw(&mut client));
    let (font, splash_x, splash_y) = loading_font();
    let mut media = Media::empty();
    media.p12 = Some(font);
    renderer.media = Arc::new(media);
    renderer
        .area_game
        .as_mut()
        .unwrap()
        .pixels
        .fill(0x0055_aa33);
    client.scene_state = 2;
    let fault = RenderLimit::new(RenderResource::NavInput, 2, 1);
    assert_eq!(client.set_budgeted_nav_debug_paint(Err(fault)), Err(fault));
    freeze_modal(&mut client, 10, 20);
    let pending = cpu_frame(renderer.mainredraw(&mut client));
    assert_eq!(renderer.render_fault(), Some(RenderFault::Budget(fault)));
    assert_eq!(renderer.scene_progress(), SceneProgress::NoScene);
    assert!(!renderer.has_valid_scene());
    assert_eq!(renderer.area_game.as_ref().unwrap().pixels[0], 0);
    assert_eq!(
        pending.pixels[splash_y * pending.width as usize + splash_x],
        0x00ff_ffff
    );
    assert_eq!(
        pending.pixels[24 * pending.width as usize + 14] & 0xffffff,
        0x0012_3456
    );
    assert_eq!(renderer.retained_storage_allocations(), 0);
}
