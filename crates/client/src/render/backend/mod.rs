//! The rasterizer seam (plan Interfaces, task 4): a frame is
//! `begin` → `scene` → `composite_scene` → `chrome` → `finish`.
//! `CpuBackend` keeps the pixel-faithful Pix3D/Pix2D path; the wgpu backend
//! (task 7) overrides `scene`/`composite_scene`/`finish` (2D chrome stays
//! CPU that slice).

pub mod capture;
pub mod cpu;
pub mod gpu;
pub mod gpu_atlas;
pub mod present;
pub mod readback;
pub(crate) mod retained_camera;
use crate::client::client::Client;
use crate::graphics::PixMap;
use crate::render::budget::{RenderBytes, RenderLimit};
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

/// Typed failure to produce a complete fresh GPU presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderFault {
    Budget(RenderLimit),
    CaptureContended,
    AssetsContended,
    SceneLockContended,
    PresentContended,
    PresentIdentityExhausted,
    MailboxSequenceExhausted,
    PresentRetired,
    #[cfg(feature = "render-test-hooks")]
    InjectedResourceLimit,
}

impl std::fmt::Display for RenderFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget(limit) => std::fmt::Display::fmt(limit, f),
            Self::CaptureContended => f.write_str("capture is busy"),
            Self::AssetsContended => f.write_str("render assets are busy"),
            Self::SceneLockContended => f.write_str("scene is busy"),
            Self::PresentContended => f.write_str("presentation is busy"),
            Self::PresentIdentityExhausted => f.write_str("presentation sequence is exhausted"),
            Self::MailboxSequenceExhausted => f.write_str("frame mailbox sequence is exhausted"),
            Self::PresentRetired => f.write_str("presentation has been retired"),
            #[cfg(feature = "render-test-hooks")]
            Self::InjectedResourceLimit => f.write_str("fixture render limit was reached"),
        }
    }
}

/// A wgpu texture view returned by the renderer. Direct display binding is
/// available through `display_view`; host readback authority is only granted
/// by a registered [`present::ConsumerRegistration`].
pub struct TextureHandle {
    device: wgpu::Device,
    queue: wgpu::Queue,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    token: Option<present::PresentToken>,
    has_valid_scene: bool,
    gate: Option<std::sync::Arc<present::PresentGate>>,
    fresh: bool,
    render_fault: Option<RenderFault>,
}

impl TextureHandle {
    /// Construct a successfully submitted GPU presentation.
    #[expect(
        clippy::too_many_arguments,
        reason = "Sealed constructor transfers the GPU resource and all publication evidence together"
    )]
    fn presented(
        device: wgpu::Device,
        queue: wgpu::Queue,
        view: wgpu::TextureView,
        geometry: capture::CaptureGeometry,
        token: present::PresentToken,
        has_valid_scene: bool,
        gate: std::sync::Arc<present::PresentGate>,
        fresh: bool,
        render_fault: Option<RenderFault>,
    ) -> Self {
        Self {
            device,
            queue,
            view,
            width: geometry.width(),
            height: geometry.height(),
            token: Some(token),
            has_valid_scene,
            gate: Some(gate),
            fresh,
            render_fault,
        }
    }

    /// Construct a caller-owned, untracked texture. It never probes or
    /// initializes an adapter and cannot register a retained consumer.
    pub fn untracked(
        device: wgpu::Device,
        queue: wgpu::Queue,
        view: wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            device,
            queue,
            view,
            width,
            height,
            token: None,
            has_valid_scene: false,
            gate: None,
            fresh: true,
            render_fault: None,
        }
    }

    /// An output whose final present copy was refused. Its display view is
    /// the prior stable texture and it cannot publish a new consumer token.
    #[expect(
        clippy::too_many_arguments,
        reason = "Sealed constructor preserves the prior GPU resource and its refusal evidence together"
    )]
    fn nonfresh(
        device: wgpu::Device,
        queue: wgpu::Queue,
        view: wgpu::TextureView,
        geometry: capture::CaptureGeometry,
        token: Option<present::PresentToken>,
        has_valid_scene: bool,
        gate: std::sync::Arc<present::PresentGate>,
        render_fault: Option<RenderFault>,
    ) -> Self {
        Self {
            device,
            queue,
            view,
            width: geometry.width(),
            height: geometry.height(),
            token,
            has_valid_scene,
            gate: Some(gate),
            fresh: false,
            render_fault,
        }
    }

    pub fn display_view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn present_token(&self) -> Option<present::PresentToken> {
        self.token
    }

    pub fn has_valid_scene(&self) -> bool {
        self.has_valid_scene
    }
    /// Typed failure to produce a complete fresh GPU presentation, if any.
    pub fn render_fault(&self) -> Option<RenderFault> {
        self.render_fault
    }

    /// Legacy synchronous path for standalone callers only. Production host
    /// capture uses a held consumer frame and the shared readback permit.
    pub fn read_back(&self) -> Vec<i32> {
        let bytes_per_row = (self.width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 frame readback"),
            size: (bytes_per_row as u64) * (self.height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let _readback_storage = crate::profiling::Allocation::new(buffer.size(), 0);
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
                pixels.push(
                    ((data[i] as i32) << 16) | ((data[i + 1] as i32) << 8) | (data[i + 2] as i32),
                );
            }
        }
        drop(data);
        buffer.unmap();
        pixels
    }
}

pub use capture::{CaptureGeometry, READBACK_MAX_STAGE_BYTES, RECORD_MAX_RAW_PIXELS};
pub use present::{
    configure_retained_present_budget, ConsumerFrame, ConsumerRegistration, PresentToken,
    ReadbackKind, RetainedPresentBudget,
};
pub use readback::{
    begin_window_readback, CompletedReadback, CompletedWindowReadback, PendingReadback,
    PendingWindowReadback, ReadbackBudget, ReadbackError, ReadbackPermit,
};

/// The output of a rendered frame, owned by the backend that produced it.
/// `CpuBackend` returns the composited `draw_area` as an owned `PixMap`
/// (the pixel-faithful path); the wgpu backend returns a `Texture` (the
/// full-frame GPU composite). `finish` must be able to return either, so
/// the seam does not assume the renderer owns the framebuffer.
pub enum FrameOutput {
    /// CpuBackend: the composited 765×503 frame.
    PixMap(PixMap),
    /// GpuBackend: the full-frame wgpu composite.
    Texture(TextureHandle),
    /// The selected backend refused this frame before publishing a payload.
    Refused(RenderFault),
}

impl FrameOutput {
    /// Whether this output contains a newly submitted present. A nonfresh
    /// GPU result refers to the prior stable texture and must not advance a
    /// host mailbox generation.
    pub fn is_fresh(&self) -> bool {
        match self {
            Self::PixMap(_) => true,
            Self::Texture(handle) => handle.fresh,
            Self::Refused(_) => false,
        }
    }
    /// Typed backend failure to produce a complete fresh output, if any.
    pub fn render_fault(&self) -> Option<RenderFault> {
        match self {
            Self::PixMap(_) => None,
            Self::Texture(handle) => handle.render_fault(),
            Self::Refused(fault) => Some(*fault),
        }
    }
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
/// Whether the current paint built a new 3D scene or reused an existing one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SceneProgress {
    /// This paint completed a live scene render.
    Rebuilt,
    /// This paint kept an existing scene without rebuilding it.
    Retained,
    /// No valid scene was available for this paint.
    #[default]
    NoScene,
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
/// copies its `scene_texture` into the full-frame texture on the GPU and
/// `finish` composites the uploaded `draw_area` over it — no readback.
/// The default is a no-op so test backends and the title path (which has
/// no 3D scene) need nothing.
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
    /// built (`scene_state != 2`). GPU keeps the last scene texture while
    /// `scene_state == 1` (Java freeze + loading splash).
    fn scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind);
    /// Land the backend's scene in `draw_area` at the (4, 4) blit point,
    /// ahead of the 2D chrome (task 7 composite seam). `CpuBackend` blits
    /// `area_game`; `GpuBackend` copies its scene texture into the
    /// full-frame on the GPU, with the uploaded `draw_area` (the overlay
    /// pixels included) composited over it. Default: no-op.
    fn composite_scene(&mut self, _core: &mut Client, _r: &mut Renderer, _kind: FrameKind) {}
    /// 2D chrome over the scene: ifaces/HUD, side/chat/icons, minimap and
    /// the in-game redraw-frame compositing, or the title compositing.
    fn chrome(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind);
    /// The composited frame, owned by the backend: `FrameOutput::PixMap`
    /// for `CpuBackend`, `FrameOutput::Texture` for wgpu, or `Refused` when
    /// a bounded resource fault prevented publishing a complete frame.
    fn finish(&mut self, r: &mut Renderer) -> FrameOutput;
    /// Typed failure from this paint, including a recovered usable CPU frame.
    /// `Refused` is reserved for paints with no complete usable output.
    fn render_fault(&self) -> Option<RenderFault> {
        None
    }
    /// Retained backend-owned storage objects, excluding reusable CPU
    /// surfaces that are accounted separately by the embedding renderer.
    fn retained_storage_allocations(&self) -> u64 {
        0
    }
    /// Retained backend-owned payload bytes, excluding reusable CPU surfaces.
    fn retained_storage_bytes(&self) -> usize {
        0
    }
    /// Scene work completed during the current paint.
    fn scene_progress(&self) -> SceneProgress {
        SceneProgress::NoScene
    }
    /// Whether the backend currently owns a completed scene eligible for
    /// retained presentation and scene-dependent proof.
    fn has_valid_scene(&self) -> bool {
        false
    }

    /// On-demand logical owner census. Regular paints must not enumerate
    /// every owner; default backends contribute no additional GPU/context data.
    fn render_bytes(&self) -> RenderBytes {
        RenderBytes::default()
    }
    /// Which backend this is (the selection/fallback tests and the window
    /// driver use it); defaults to `Cpu`.
    fn kind(&self) -> BackendKind {
        BackendKind::Cpu
    }
}

pub use cpu::CpuBackend;
pub use gpu::inject_device;
pub use gpu::GpuBackend;

#[cfg(test)]
mod tests {
    use super::RenderFault;
    use crate::render::budget::{RenderLimit, RenderResource};

    #[test]
    fn render_fault_display_uses_operator_wording() {
        for (fault, expected) in [
            (RenderFault::CaptureContended, "capture is busy"),
            (RenderFault::AssetsContended, "render assets are busy"),
            (RenderFault::SceneLockContended, "scene is busy"),
            (RenderFault::PresentContended, "presentation is busy"),
            (
                RenderFault::PresentIdentityExhausted,
                "presentation sequence is exhausted",
            ),
            (
                RenderFault::MailboxSequenceExhausted,
                "frame mailbox sequence is exhausted",
            ),
            (RenderFault::PresentRetired, "presentation has been retired"),
            (
                RenderFault::Budget(RenderLimit::new(RenderResource::Capture, 2, 1)),
                "capture budget exceeded: requested 2 bytes, maximum 1 bytes",
            ),
        ] {
            assert_eq!(fault.to_string(), expected);
        }
        #[cfg(feature = "render-test-hooks")]
        assert_eq!(
            RenderFault::InjectedResourceLimit.to_string(),
            "fixture render limit was reached",
        );
    }
}
