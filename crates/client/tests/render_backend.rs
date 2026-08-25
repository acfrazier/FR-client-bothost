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
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default())).ok()?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("r274 seam-test frame"),
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
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

// Task 7: the backend selection. `Renderer` prefers the wgpu backend when
// the driver asked for a GPU (`set_prefer_gpu`); a wgpu init failure
// (no adapter, or the `R274_TEST_FORCE_NO_GPU` test hook) must fall back to
// `CpuBackend` — logged, never fatal — and the renderer still produces a
// frame. The fallback is once-per-process: the first failed init is cached
// by the shared GPU context, so the test pins the *selection* contract, not
// the adapter state.
#[test]
fn gpu_init_failure_falls_back_to_cpu() {
    // Force the process-wide wgpu init to fail (no adapter) via the test
    // hook env var, then ask for a GPU backend: the renderer must land on
    // `CpuBackend` and still produce a frame.
    std::env::set_var("R274_TEST_FORCE_NO_GPU", "1");
    Renderer::set_prefer_gpu(true);
    let mut r = Renderer::new(false);
    assert_eq!(
        r.backend_kind(),
        BackendKind::Cpu,
        "a failed wgpu init must fall back to the CPU backend, never panic"
    );

    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    let output = r.game_draw(&mut c);
    assert!(
        matches!(output, FrameOutput::PixMap(_)),
        "the fallback renderer must still produce a frame"
    );
    let FrameOutput::PixMap(frame) = output else { unreachable!() };
    assert!(!frame.pixels.is_empty(), "the fallback frame must be painted");

    Renderer::set_prefer_gpu(false);
    std::env::remove_var("R274_TEST_FORCE_NO_GPU");
}
