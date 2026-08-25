// Task 4 (render backend seam): a frame routes through the renderer's
// backend as `begin` → `scene` → `chrome` → `finish`, in order, on both
// frame paths (`game_draw` in-game, `title_screen_draw` on the login
// screen). The stub below never touches the client or renderer state, so a
// run also proves the renderer drives its frame purely through the backend
// once the CpuBackend path owns the rasterization. `CpuBackend` itself is
// the fidelity path the rest of the suite exercises.
use std::cell::RefCell;
use std::rc::Rc;

use client::client::{Client, ClientConfig};
use client::render::backend::{FrameKind, FrameOutput, RenderBackend};
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
    fn finish<'a>(&mut self, _r: &'a mut Renderer) -> FrameOutput<'a> {
        self.log.0.borrow_mut().push("finish");
        FrameOutput::Pix(&_r.draw_area)
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
    r.game_draw(&mut c);
    assert_eq!(*log.0.borrow(), ["begin", "scene", "chrome", "finish"]);

    // Title frame: `mainredraw`'s `title_screen_draw` path routes the
    // same four stages.
    let log = CallLog::default();
    let mut r = Renderer::with_backend(Box::new(StubBackend { log: log.clone() }), false);
    let mut c = client();
    c.set_draw(true);
    r.title_screen_draw(&mut c);
    assert_eq!(*log.0.borrow(), ["begin", "scene", "chrome", "finish"]);
}
