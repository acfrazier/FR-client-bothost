//! The rasterizer seam (plan Interfaces, task 4): a frame is
//! `begin` → `scene` → `chrome` → `finish`. `CpuBackend` keeps the
//! pixel-faithful Pix3D/Pix2D path; the wgpu backend (task 7) overrides
//! `scene`/`finish` (2D chrome stays CPU that slice).

pub mod cpu;

use crate::client::client::Client;
use crate::graphics::PixMap;
use crate::render::Renderer;

/// A wgpu texture handle the GPU backend will return (task 7 fills in the
/// real type; the variant and ownership shape exist now so the seam
/// accepts a GPU path).
pub struct TextureHandle;

/// The output of a rendered frame, owned by the backend that produced it.
/// `CpuBackend` returns the composited `draw_area` as an owned `PixMap`
/// (the pixel-faithful path); the wgpu backend (task 7) returns a texture
/// handle instead. `finish` must be able to return either, so the seam
/// does not assume the renderer owns the framebuffer.
pub enum FrameOutput {
    /// CpuBackend: the composited 765×503 frame.
    PixMap(PixMap),
    /// GpuBackend: a wgpu texture handle (task 7).
    Texture(TextureHandle),
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
/// take `&mut Client` plus the renderer. `Renderer` takes the backend out
/// of the struct for the duration of a frame (`FrameBackend`, see
/// `renderer.rs`) so the stages can borrow the renderer's state.
///
/// Output is backend-owned: `finish` returns a `FrameOutput` the backend
/// produced, never a borrow of the renderer's framebuffer. A GPU backend
/// rasterizes into its own texture during `scene` and returns
/// `FrameOutput::Texture`; `r` is the draw state the CPU path composites
/// from and is unused by a texture backend.
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
    /// The composited frame, owned by the backend: `FrameOutput::PixMap`
    /// for `CpuBackend`, `FrameOutput::Texture` for the wgpu backend.
    fn finish(&mut self, r: &mut Renderer) -> FrameOutput;
}

pub use cpu::CpuBackend;
