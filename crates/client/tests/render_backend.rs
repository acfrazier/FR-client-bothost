#[path = "common/gpu_serial.rs"]
mod gpu_serial;
#[cfg(feature = "render-test-hooks")]
#[path = "common/scene_fixture.rs"]
mod scene_fixture;

// Task 4 (render backend seam): a frame routes through the renderer's
// backend as `begin` → `scene` → `chrome` → `finish`, in order, on both
// frame paths (`game_draw` in-game, `title_screen_draw` on the login
// screen). The stub below never touches the client or renderer state, so a
// run also proves the renderer drives its frame purely through the backend
// once the CpuBackend path owns the rasterization. `CpuBackend` itself is
// the fidelity path the rest of the suite exercises.
//
// Fix round 1 (texture-capable seam): `finish` returns a backend-owned
// `FrameOutput` — `PixMap` on the CPU path, `Texture` for the GPU path —
// so `cpu_backend_returns_owned_pixmap_frame` and
// `texture_backend_surfaces_owned_texture` pin both variants.
use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use client::client::{Client, ClientConfig};
use client::graphics::PixMap;
use client::render::backend::{BackendKind, FrameKind, FrameOutput, RenderBackend, TextureHandle};
use client::render::Renderer;
use std::process::Command;

#[cfg(feature = "render-test-hooks")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "render-test-hooks")]
use std::sync::mpsc;
#[cfg(feature = "render-test-hooks")]
use std::thread;
#[cfg(feature = "render-test-hooks")]
use std::time::Duration;

#[cfg(feature = "render-test-hooks")]
static GPU_WORKSPACE_CALLS: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "render-test-hooks")]
static GPU_SUBMIT_CALLS: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "render-test-hooks")]
fn count_workspace_submit(submit: &mut dyn FnMut()) {
    GPU_WORKSPACE_CALLS.fetch_add(1, Ordering::Relaxed);
    GPU_SUBMIT_CALLS.fetch_add(1, Ordering::Relaxed);
    submit();
}

#[cfg(feature = "render-test-hooks")]
static GPU_OVERLAY_ENTRIES: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "render-test-hooks")]
fn count_overlay_entry(point: client::render::backend::gpu::test_hooks::SceneTestPoint) {
    use client::render::backend::gpu::test_hooks::SceneTestPoint;

    if point == SceneTestPoint::OverlayEntry {
        GPU_OVERLAY_ENTRIES.fetch_add(1, Ordering::Relaxed);
    }
}

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// Test-only backend: records the frame-stage call order.
#[derive(Clone, Default)]
struct CallLog(Rc<RefCell<Vec<&'static str>>>);

#[derive(Default)]
struct StubBackend {
    log: CallLog,
}

impl RenderBackend for StubBackend {
    fn begin(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {
        self.log.0.borrow_mut().push("begin");
    }
    fn scene(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {
        self.log.0.borrow_mut().push("scene");
    }
    fn chrome(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {
        self.log.0.borrow_mut().push("chrome");
    }
    fn finish(&mut self, _r: &mut Renderer) -> FrameOutput {
        self.log.0.borrow_mut().push("finish");
        FrameOutput::PixMap(PixMap::new(1, 1))
    }
}

#[test]
fn renderer_backend_selection() {
    // In-game frame: `mainredraw`'s `game_draw` path.
    let log = CallLog::default();
    let mut r = Renderer::with_backend(Box::new(StubBackend { log: log.clone() }), false);
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    let output = r.game_draw(&mut c);
    assert_eq!(*log.0.borrow(), ["begin", "scene", "chrome", "finish"]);
    // The stub's own (backend-owned) output comes back untouched.
    assert!(matches!(output, FrameOutput::PixMap(_)));

    // Title frame: `mainredraw`'s `title_screen_draw` path routes the
    // same four stages.
    let log = CallLog::default();
    let mut r = Renderer::with_backend(Box::new(StubBackend { log: log.clone() }), false);
    let mut c = client();
    c.set_draw(true);
    let output = r.title_screen_draw(&mut c);
    assert_eq!(*log.0.borrow(), ["begin", "scene", "chrome", "finish"]);
    assert!(matches!(output, FrameOutput::PixMap(_)));
}

#[test]
fn cpu_backend_returns_owned_pixmap_frame() {
    let mut r = Renderer::new(false); // default CpuBackend
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    let output = r.game_draw(&mut c);
    let FrameOutput::PixMap(frame) = output else {
        panic!("CpuBackend must return the composited frame as an owned PixMap");
    };
    assert_eq!(frame.width, r.draw_area.width);
    assert_eq!(frame.height, r.draw_area.height);
    assert_eq!(frame.pixels, r.draw_area.pixels);
    // The renderer's framebuffer still holds the frame for the tests and
    // the `window` blit.
    assert!(!r.draw_area.pixels.is_empty());
}

/// Test-only GPU-shaped backend: produces a texture without writing into a
/// renderer-owned PixMap, proving the seam accepts a GPU output. The
/// handle needs a real wgpu device; the test skips on machines without an
/// adapter.
struct TextureBackend {
    handle: Option<TextureHandle>,
}

impl TextureBackend {
    fn new() -> Self {
        TextureBackend {
            handle: dummy_handle(),
        }
    }
}

/// A real 1×1 frame handle on this machine's adapter (None = no adapter).
fn dummy_handle() -> Option<TextureHandle> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).ok()?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("r274 seam-test frame"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    Some(TextureHandle {
        device,
        queue,
        view: texture.create_view(&Default::default()),
        width: 1,
        height: 1,
    })
}

impl RenderBackend for TextureBackend {
    fn begin(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    fn scene(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    fn chrome(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    fn finish(&mut self, _r: &mut Renderer) -> FrameOutput {
        FrameOutput::Texture(self.handle.take().expect("adapter-gated test"))
    }
}

#[test]
fn texture_backend_surfaces_owned_texture() {
    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; texture output test skips");
        return;
    }

    let backend = TextureBackend::new();
    if backend.handle.is_none() {
        eprintln!("no adapter on this machine; the texture seam test skips");
        return;
    }
    let mut r = Renderer::with_backend(Box::new(backend), false);
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    let output = r.game_draw(&mut c);
    assert!(
        matches!(output, FrameOutput::Texture(_)),
        "a texture backend must surface its output through FrameOutput::Texture"
    );
    let FrameOutput::Texture(handle) = output else {
        unreachable!()
    };
    assert_eq!(handle.width, 1);
    assert_eq!(handle.height, 1);
    assert_eq!(handle.read_back(), vec![0]);
}

/// Test-only backend whose `scene` panics on the first frame only.
struct PanicOnce {
    panicked: bool,
}

impl RenderBackend for PanicOnce {
    fn begin(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    fn scene(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {
        if !self.panicked {
            self.panicked = true;
            panic!("first-frame scene panic");
        }
    }
    fn chrome(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    fn finish(&mut self, _r: &mut Renderer) -> FrameOutput {
        FrameOutput::PixMap(PixMap::new(1, 1))
    }
}

#[test]
fn stage_panic_reinstalls_backend() {
    let mut r = Renderer::with_backend(Box::new(PanicOnce { panicked: false }), false);
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = r.game_draw(&mut c);
    }));
    assert!(result.is_err(), "the stage panic must propagate");
    // The Drop guard reinstalled the backend: a second frame runs without
    // hitting the "render backend present" expect (which would panic if the
    // backend had been left `None` by the unwind).
    let output = r.game_draw(&mut c);
    assert!(matches!(output, FrameOutput::PixMap(_)));
}

// Keep the forced-init-failure assertion in a child invocation. A failed
// `GpuBackend::try_new` is cached in the process-wide OnceLock and must not
// poison the real-GPU scene tests in this executable.
#[test]
fn gpu_init_failure_falls_back_to_cpu() {
    let status = Command::new(std::env::current_exe().expect("test executable path"))
        .args([
            "--exact",
            "gpu_init_failure_in_child_process",
            "--nocapture",
        ])
        .env("R274_RENDER_BACKEND_GPU_FAILURE_CHILD", "1")
        .env_remove("R274_TEST_FORCE_NO_GPU")
        .status()
        .expect("spawn isolated GPU fallback assertion");
    assert!(status.success(), "isolated fallback assertion must pass");
}

#[test]
fn gpu_init_failure_in_child_process() {
    if std::env::var("R274_RENDER_BACKEND_GPU_FAILURE_CHILD").as_deref() != Ok("1") {
        return;
    }
    std::env::set_var("R274_TEST_FORCE_NO_GPU", "1");
    Renderer::set_prefer_gpu(true);
    let mut renderer = Renderer::new(false);
    assert_eq!(
        renderer.backend_kind(),
        BackendKind::Cpu,
        "a failed wgpu init must fall back to the CPU backend, never panic"
    );

    let mut client = client();
    client.set_draw(true);
    client.ingame = true;
    let output = renderer.game_draw(&mut client);
    assert!(
        matches!(output, FrameOutput::PixMap(_)),
        "the fallback renderer must still produce a frame"
    );
    let FrameOutput::PixMap(frame) = output else {
        unreachable!()
    };
    assert!(
        !frame.pixels.is_empty(),
        "the fallback frame must be painted"
    );

    Renderer::set_prefer_gpu(false);
    std::env::remove_var("R274_TEST_FORCE_NO_GPU");
}

#[cfg(feature = "render-test-hooks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScenePhase {
    InitialRebuild,
    Freeze,
    RebuildAfterFreeze,
}

#[cfg(feature = "render-test-hooks")]
#[derive(Debug)]
struct SceneFrame {
    phase: ScenePhase,
    terrain_pixel: i32,
    scene_cycles: i32,
    render_count: i32,
    workspace_calls: usize,
    submit_calls: usize,
    overlay_entries: usize,
}

#[cfg(feature = "render-test-hooks")]
enum SceneMessage {
    Unavailable,
    Painted(SceneFrame),
}

#[cfg(feature = "render-test-hooks")]
enum SceneCommand {
    Freeze,
    Rebuild,
}

#[cfg(feature = "render-test-hooks")]
fn scene_frame(phase: ScenePhase, output: FrameOutput, renderer: &Renderer) -> SceneFrame {
    SceneFrame {
        phase,
        terrain_pixel: scene_fixture::terrain_pixel(output),
        scene_cycles: renderer.scene_cycle,
        render_count: renderer.world.render_count(),
        workspace_calls: GPU_WORKSPACE_CALLS.load(Ordering::Relaxed),
        submit_calls: GPU_SUBMIT_CALLS.load(Ordering::Relaxed),
        overlay_entries: GPU_OVERLAY_ENTRIES.load(Ordering::Relaxed),
    }
}

/// No admission hook is installed: Standalone must preserve the old state-2
/// submit and state-1 last-scene freeze, even with the legacy workspace hook.
#[cfg(feature = "render-test-hooks")]
#[test]
fn unused_admission_keeps_rebuild_and_freeze_behavior() {
    use client::render::backend::gpu::test_hooks;

    let _gpu_serial = gpu_serial::lock();
    if gpu_serial::skipped() {
        eprintln!("SKIP_GPU=1; unused-admission behavior needs a GPU scene");
        return;
    }

    GPU_WORKSPACE_CALLS.store(0, Ordering::Relaxed);
    GPU_SUBMIT_CALLS.store(0, Ordering::Relaxed);
    GPU_OVERLAY_ENTRIES.store(0, Ordering::Relaxed);
    let (command_tx, command_rx) = mpsc::channel();
    let (frame_tx, frame_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let Some((mut client, mut renderer)) = scene_fixture::gpu_scene() else {
            let _ = frame_tx.send(SceneMessage::Unavailable);
            return;
        };
        // This is the existing workspace hook, not an admission hook. The
        // test pins that leaving admission unset keeps Standalone behavior.
        client::render::backend::gpu::set_scene_workspace_hook(count_workspace_submit);
        let _overlay = test_hooks::install_for_thread(count_overlay_entry);

        let output = renderer.game_draw(&mut client);
        if frame_tx
            .send(SceneMessage::Painted(scene_frame(
                ScenePhase::InitialRebuild,
                output,
                &renderer,
            )))
            .is_err()
        {
            return;
        }

        if let Ok(SceneCommand::Freeze) = command_rx.recv() {
            client.scene_state = 1;
            let output = renderer.game_draw(&mut client);
            if frame_tx
                .send(SceneMessage::Painted(scene_frame(
                    ScenePhase::Freeze,
                    output,
                    &renderer,
                )))
                .is_err()
            {
                return;
            }
        }

        if let Ok(SceneCommand::Rebuild) = command_rx.recv() {
            client.scene_state = 2;
            let output = renderer.game_draw(&mut client);
            let _ = frame_tx.send(SceneMessage::Painted(scene_frame(
                ScenePhase::RebuildAfterFreeze,
                output,
                &renderer,
            )));
        }
    });

    let initial = frame_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("GPU fixture must initialize or report an unavailable adapter");
    let SceneMessage::Painted(initial) = initial else {
        worker.join().expect("unavailable GPU worker must exit");
        eprintln!("no adapter; unused-admission behavior test skips");
        return;
    };
    assert_eq!(initial.phase, ScenePhase::InitialRebuild);
    assert_ne!(
        initial.terrain_pixel, 0,
        "state 2 must submit the real terrain scene"
    );
    assert_eq!(initial.workspace_calls, 1);
    assert_eq!(initial.submit_calls, 1);
    assert_eq!(initial.scene_cycles, 1);
    assert_eq!(initial.render_count, 1);
    assert_eq!(initial.overlay_entries, 1);

    // If state 1 touches the production scene mutex, this paint cannot
    // finish until the holder is dropped. It must preserve the last scene,
    // draw overlays, and skip the workspace entirely.
    let scene_lock = test_hooks::hold_scene_lock();
    command_tx
        .send(SceneCommand::Freeze)
        .expect("scene worker must wait for the freeze command");
    let freeze_while_locked = frame_rx.recv_timeout(Duration::from_secs(10));
    let returned_while_locked = freeze_while_locked.is_ok();
    drop(scene_lock);
    let freeze = freeze_while_locked.unwrap_or_else(|_| {
        frame_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("freeze paint")
    });
    let SceneMessage::Painted(freeze) = freeze else {
        panic!("the state-1 paint must return a frame");
    };

    command_tx
        .send(SceneCommand::Rebuild)
        .expect("scene worker must wait for the rebuild command");
    let rebuilt = frame_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("state-2 paint must rebuild after the freeze");
    worker.join().expect("scene fixture worker must not panic");
    let SceneMessage::Painted(rebuilt) = rebuilt else {
        panic!("the second state-2 paint must return a frame");
    };

    assert!(
        returned_while_locked,
        "state 1 must not acquire the scene lock"
    );
    assert_eq!(freeze.phase, ScenePhase::Freeze);
    assert_ne!(
        freeze.terrain_pixel, 0,
        "state 1 must preserve the last FBO"
    );
    assert_eq!(freeze.terrain_pixel, initial.terrain_pixel);
    assert_eq!(
        freeze.workspace_calls, 1,
        "state 1 must not enter the workspace"
    );
    assert_eq!(freeze.submit_calls, 1, "state 1 must not submit a scene");
    assert_eq!(freeze.scene_cycles, 1, "state 1 must not rebuild the scene");
    assert_eq!(freeze.render_count, 1);
    assert_eq!(freeze.overlay_entries, 2, "freeze overlays remain live");

    assert_eq!(rebuilt.phase, ScenePhase::RebuildAfterFreeze);
    assert_ne!(
        rebuilt.terrain_pixel, 0,
        "state 2 must submit terrain again"
    );
    assert_eq!(rebuilt.workspace_calls, 2);
    assert_eq!(rebuilt.submit_calls, 2);
    assert_eq!(rebuilt.scene_cycles, 2);
    assert_eq!(rebuilt.render_count, 2);
    assert_eq!(rebuilt.overlay_entries, 3);
}
