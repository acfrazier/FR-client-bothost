//! The rasterizer seam (plan Interfaces, task 4): a frame is
//! `begin` → `scene` → `composite_scene` → `chrome` → `finish`.
//! `CpuBackend` keeps the pixel-faithful Pix3D/Pix2D path; the wgpu backend
//! (task 7) overrides `scene`/`composite_scene`/`finish` (2D chrome stays
//! CPU that slice).

pub mod cpu;
pub mod gpu;

use crate::client::client::Client;
use crate::graphics::PixMap;
use crate::render::Renderer;

/// Which backend rasterized the frame (the selection tests and the window
/// driver log it; `CpuBackend` is the default and the fallback).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendKind {
    /// The software Pix3D/Pix2D path — the fidelity path and the default.
    Cpu,
    /// The wgpu scene rasterizer (task 7); 2D chrome still Pix2D.
    Gpu,
}

/// A wgpu texture handle the GPU backend can return. The seam accepts a GPU
/// output (`FrameOutput::Texture`); this slice's `GpuBackend` composites
/// its scene texture back into the CPU `draw_area` so the window present
/// path stays untouched, so the handle stays an opaque marker for the
/// host-owned texture handoff (slice 2).
pub struct TextureHandle;

/// The output of a rendered frame, owned by the backend that produced it.
/// `CpuBackend` returns the composited `draw_area` as an owned `PixMap`
/// (the pixel-faithful path); the wgpu backend (task 7) can return a
/// texture handle instead. `finish` must be able to return either, so the
/// seam does not assume the renderer owns the framebuffer.
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
/// The composite seam (task 7): `scene` produces the 3D scene content
/// (`CpuBackend` into `area_game`, `GpuBackend` into a wgpu texture), and
/// `composite_scene` lands it in `draw_area` at the (4, 4) blit point
/// ahead of the 2D chrome. `CpuBackend` blits `area_game`; `GpuBackend`
/// reads its texture back and blits it (with the `area_game` overlay
/// content merged) at the same point. The default is a no-op so test
/// backends and the title path (which has no 3D scene) need nothing.
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
    /// Assemble + rasterize the 3D scene. `CpuBackend` renders into
    /// `area_game`; `GpuBackend` rasterizes the scene graph into a wgpu
    /// texture. A no-op on the login screen and while the scene is not
    /// built (`scene_state != 2`).
    fn scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind);
    /// Land the backend's scene in `draw_area` at the (4, 4) blit point,
    /// ahead of the 2D chrome (task 7 composite seam). `CpuBackend` blits
    /// `area_game`; `GpuBackend` reads its scene texture back and blits it
    /// with the `area_game` overlay content merged. Default: no-op.
    fn composite_scene(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    /// 2D chrome over the scene: ifaces/HUD, side/chat/icons, minimap and
    /// the in-game redraw-frame compositing, or the title compositing.
    fn chrome(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind);
    /// The composited frame, owned by the backend: `FrameOutput::PixMap`
    /// for `CpuBackend`, `FrameOutput::Texture` for the wgpu backend.
    fn finish(&mut self, r: &mut Renderer) -> FrameOutput;
    /// Which backend this is (the selection/fallback tests and the window
    /// driver use it); defaults to `Cpu`.
    fn kind(&self) -> BackendKind {
        BackendKind::Cpu
    }
}

pub use cpu::CpuBackend;
pub use gpu::GpuBackend;
