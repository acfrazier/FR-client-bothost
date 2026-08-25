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
use client::render::backend::{FrameKind, FrameOutput, RenderBackend, TextureHandle};
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
/// renderer-owned PixMap, proving the seam accepts a GPU output.
struct TextureBackend;

impl RenderBackend for TextureBackend {
    fn begin(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    fn scene(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    fn chrome(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    fn finish(&mut self, _r: &mut Renderer) -> FrameOutput {
        FrameOutput::Texture(TextureHandle)
    }
}

#[test]
fn texture_backend_surfaces_owned_texture() {
    let mut r = Renderer::with_backend(Box::new(TextureBackend), false);
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    let output = r.game_draw(&mut c);
    assert!(
        matches!(output, FrameOutput::Texture(TextureHandle)),
        "a texture backend must surface its output through FrameOutput::Texture"
    );
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
