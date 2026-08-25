//! The rasterizer seam (plan Interfaces, task 4): a frame is
//! `begin` → `scene` → `composite_scene` → `chrome` → `finish`.
//! `CpuBackend` keeps the pixel-faithful Pix3D/Pix2D path; the wgpu backend
//! (task 7) overrides `scene`/`composite_scene`/`finish` (2D chrome stays
//! CPU that slice).

pub mod cpu;
pub mod gpu;
pub mod gpu_atlas;

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

/// A real wgpu texture the GPU backend returns: the full-frame 765×503
/// texture view (scene + chrome composited on the GPU) plus the owning
/// device/queue, so a consumer that must read the pixels back (the
/// `window` present path, which has no wgpu surface) can copy and map it.
/// The host-owned `Textures` path binds the view directly.
pub struct TextureHandle {
    /// The device the view was created on (a consumer read-back needs it).
    pub device: wgpu::Device,
    /// The queue the backend submits on (same reason).
    pub queue: wgpu::Queue,
    /// The composited frame texture view.
    pub view: wgpu::TextureView,
    /// Texture width in pixels (the 765×503 applet frame).
    pub width: u32,
    /// Texture height in pixels.
    pub height: u32,
}

impl TextureHandle {
    /// Copy the frame texture back to the CPU as `0x00RRGGBB` pixels (the
    /// `window` present path and the pixel-assertion tests; the host
    /// texture path never reads back). Synchronous: poll until the map
    /// completes.
    pub fn read_back(&self) -> Vec<i32> {
        // `COPY_BYTES_PER_ROW_ALIGNMENT` 256: pad the row stride.
        let bytes_per_row = ((self.width * 4 + 255) / 256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 frame readback"),
            size: (bytes_per_row as u64) * (self.height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("r274 frame readback encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: self.view.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let map_result = loop {
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            if let Ok(result) = rx.recv_timeout(std::time::Duration::from_millis(1)) {
                break result;
            }
        };
        if map_result.is_err() {
            return Vec::new();
        }
        let data = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((self.width as usize) * (self.height as usize));
        for row in 0..self.height as usize {
            let row_start = row * bytes_per_row as usize;
            for x in 0..self.width as usize {
                let i = row_start + x * 4;
                pixels.push(((data[i] as i32) << 16) | ((data[i + 1] as i32) << 8) | (data[i + 2] as i32));
            }
        }
        drop(data);
        buffer.unmap();
        pixels
    }
}

/// The output of a rendered frame, owned by the backend that produced it.
/// `CpuBackend` returns the composited `draw_area` as an owned `PixMap`
/// (the pixel-faithful path); the wgpu backend returns a `Texture` (the
/// full-frame GPU composite). `finish` must be able to return either, so
/// the seam does not assume the renderer owns the framebuffer.
pub enum FrameOutput {
    /// CpuBackend: the composited 765×503 frame.
    PixMap(PixMap),
    /// GpuBackend: the full-frame wgpu texture (scene + chrome composited
    /// on the GPU; no readback).
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
