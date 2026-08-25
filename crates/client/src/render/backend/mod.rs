//! The rasterizer seam (plan Interfaces, task 4): a frame is
//! `begin` → `scene` → `chrome` → `finish`. `CpuBackend` keeps the
//! pixel-faithful Pix3D/Pix2D path; the wgpu backend (task 7) overrides
//! `scene`/`finish` (2D chrome stays CPU that slice).

pub mod cpu;

use crate::client::client::Client;
use crate::graphics::PixMap;
use crate::render::Renderer;

/// The output of a rendered frame. The CPU backend hands back its
/// composited `draw_area` PixMap (the pixel-faithful path); the wgpu
/// backend will hand back a texture instead.
pub enum FrameOutput<'a> {
    Pix(&'a PixMap),
}

/// Which frame the renderer is drawing. The old `mainredraw` in-game /
/// title split is carried through the backend because `game_draw` and
/// `title_screen_draw` are also called directly (the tests drive them on a
/// fresh client whose `ingame` flag is not set), so `core.ingame` alone is
/// not the dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameKind {
    Game,
    Title,
}

/// The frame rasterizer behind `Renderer`.
///
/// The plan's Interfaces sketch had `scene`/`chrome` reading `&ClientCore`
/// and no renderer argument; the actual call flow (the moved
/// `game_draw`/`title_screen_draw` bodies) mutates both the client (redraw
/// flags, scroll, camera) and the renderer's own draw state, so the stages
/// take `&mut Client` plus the renderer. `Renderer` holds the backend as
/// `Option<Box<dyn RenderBackend>>` and takes it out for the duration of a
/// frame so the stages can borrow the renderer's state.
pub trait RenderBackend {
    /// Frame start: the pre-draw setup of the current `game_draw`/
    /// `title_screen_draw` structure — scroll/brightness/`prepare_game`
    /// and the redraw-frame chrome blits for the in-game frame; the title
    /// teardown + `prepare_title` for the login screen.
    fn begin(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind);
    /// Assemble + rasterize the 3D scene into `area_game` and blit it at
    /// (4, 4). A no-op on the login screen and while the scene is not
    /// built (`scene_state != 2`).
    fn scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind);
    /// 2D chrome over the scene: ifaces/HUD, side/chat/icons, minimap and
    /// the in-game redraw-frame compositing, or the title compositing.
    fn chrome(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind);
    /// The composited frame, handed to `Present` (task 6).
    fn finish<'a>(&mut self, r: &'a mut Renderer) -> FrameOutput<'a>;
}

pub use cpu::CpuBackend;
