//! The GPU chrome recorder (task 5c): the 2D chrome (title/login, ifaces,
//! chat, minimap frame, HUD, text) draws as GPU quads — coloured rects,
//! sprites from the shared sprite atlas, glyphs from the font atlas — into
//! one growable per-frame quad buffer flushed once into the full-frame
//! texture. The CPU drawing code is untouched: on the GPU path the frame
//! stages run with the `GpuChrome` recorder active (a thread-local set by
//! `GpuChromeGuard`), and the low-level primitives (`Pix2D` rects,
//! `Pix8`/`Pix32` sprite plots, `PixMap::blit_into`, `PixFont` glyphs)
//! record quads *in addition to* their pixel writes, which keeps the CPU
//! oracle byte-identical.
//!
//! Quads are recorded into the surface that is currently drawn into
//! (deferred blits): `surface_open` pushes a buffer tagged with the
//! surface's pixel pointer, the primitives append to the top buffer in
//! surface-local coordinates, and `map_blit` pops the source map's buffer
//! and re-emits its quads translated by the blit offset into the
//! destination — the same composition the CPU's persistent `draw_area`
//! blits express. Surfaces whose content cannot be quads (the rotated
//! minimap, the title flames) are marked `staged`: their primitives skip
//! and the blit uploads the whole CPU map into the sprite atlas (a
//! per-frame exception, not the rule).

use crate::graphics::{Pix32, Pix8, PixMap};
use crate::render::backend::gpu_atlas::{GpuAssets, Region};
use std::cell::Cell;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// The full-frame draw_area size the chrome quads live in.
pub const FRAME_W: u32 = 765;
pub const FRAME_H: u32 = 503;

/// One recorded chrome quad (in surface-local pixels until a blit
/// translates it into the frame). Layer selects the fragment source:
/// 0 = plain colour, 1 = sprite atlas, 2 = font atlas. `region` is the
/// quad's atlas region in atlas pixels; the `u/v` fields are the clipped
/// sub-rect relative to the region (0..1), resolved to atlas UVs at flush
/// against the current atlas size.
struct ChromeQuad {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    u_off: f32,
    v_off: f32,
    u_span: f32,
    v_span: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    layer: u32,
    region: Region,
}

/// One chrome vertex in the flushed vertex buffer (six per quad).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ChromeVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    layer: u32,
}

/// An open draw surface in the deferred-blit stack.
struct ChromeBuffer {
    /// The surface's pixel-pointer tag (the map being drawn into).
    tag: usize,
    /// The current clip rect in surface-local pixels.
    clip: [i32; 4],
    quads: Vec<ChromeQuad>,
    /// Staged surfaces skip quad recording; their blit uploads the whole
    /// CPU map instead (the rotated minimap, the title flames).
    staged: bool,
}

/// The chrome quad shader: a 2D passthrough (draw_area pixels → NDC) with
/// per-vertex colour and atlas layer.
const CHROME_SHADER: &str = r#"
@group(0) @binding(0) var sprite_atlas: texture_2d<f32>;
@group(0) @binding(1) var font_atlas: texture_2d<f32>;
@group(0) @binding(2) var chrome_sampler: sampler;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) layer: u32,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) layer: u32,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.position = vec4<f32>(
        in.pos.x * (2.0 / 765.0) - 1.0,
        1.0 - in.pos.y * (2.0 / 503.0),
        0.0,
        1.0,
    );
    out.uv = in.uv;
    out.color = in.color;
    out.layer = in.layer;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.layer == 0u) {
        return in.color;
    }
    var t: vec4<f32>;
    if (in.layer == 1u) {
        t = textureSample(sprite_atlas, chrome_sampler, in.uv);
    } else {
        t = textureSample(font_atlas, chrome_sampler, in.uv);
    }
    return vec4<f32>(in.color.rgb * t.rgb, in.color.a * t.a);
}
"#;

// The active recorder, set by `GpuChromeGuard` for the duration of a GPU
// frame stage. A raw pointer in a thread-local: single-threaded render
// loop, the guard owns the lifetime (cleared on drop, including unwind).
thread_local! {
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
}

/// Quads flushed by the last frame (the chrome-as-quads test).
static LAST_QUAD_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// The per-frame chrome recorder + quad pipeline, owned by the `GpuBackend`
/// (one per renderer; the atlases it uploads into are the process-shared
/// `GpuAssets`).
pub struct GpuChrome {
    device: wgpu::Device,
    queue: wgpu::Queue,
    assets: Arc<Mutex<GpuAssets>>,
    /// The deferred-blit surface stack (the root buffer at index 0 is the
    /// frame-space quad list flushed at `finish`).
    buffers: Vec<ChromeBuffer>,
    /// Surface tags composited from a per-frame CPU staging upload.
    staged: HashSet<usize>,
    vertex_buf: wgpu::Buffer,
    vertex_buf_capacity: usize,
    pipeline: wgpu::RenderPipeline,
    /// Quads recorded in the current frame (the chrome-as-quads test).
    quad_count: usize,
}

impl GpuChrome {
    /// Build the recorder on the shared context/assets.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        assets: Arc<Mutex<GpuAssets>>,
    ) -> GpuChrome {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("r274 chrome shader"),
            source: wgpu::ShaderSource::Wgsl(CHROME_SHADER.into()),
        });
        let bind_group_layout = assets.lock().unwrap().chrome_bind_group_layout().clone();
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("r274 chrome layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let vertex = wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ChromeVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2,
                    1 => Float32x2,
                    2 => Float32x4,
                    3 => Uint32,
                ],
            }],
        };
        let fragment = wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("r274 chrome quads"),
            layout: Some(&layout),
            vertex,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(fragment),
            multiview_mask: None,
            cache: None,
        });
        let vertex_buf_capacity = 1 << 18; // 256 KiB of quads to start
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 chrome vertices"),
            size: vertex_buf_capacity as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        GpuChrome {
            device: device.clone(),
            queue: queue.clone(),
            assets,
            buffers: Vec::new(),
            staged: HashSet::new(),
            vertex_buf,
            vertex_buf_capacity,
            pipeline,
            quad_count: 0,
        }
    }

    /// Quads recorded in the current frame (the chrome-as-quads test).
    pub fn quad_count(&self) -> usize {
        self.quad_count
    }

    /// Reset for a new frame: the surface stack empties and the root
    /// (draw_area-space) buffer opens. `draw_area` is the root tag.
    pub fn frame_begin(&mut self, draw_area: &PixMap) {
        self.buffers.clear();
        self.staged.clear();
        self.buffers.push(ChromeBuffer {
            tag: draw_area.pixels.as_ptr() as usize,
            clip: [0, 0, FRAME_W as i32, FRAME_H as i32],
            quads: Vec::new(),
            staged: false,
        });
    }

    /// Mark a map whose content is composited from a per-frame staging
    /// upload (the rotated minimap, the title flames).
    pub fn mark_staged(&mut self, map: Option<&PixMap>) {
        if let Some(m) = map {
            self.staged.insert(m.pixels.as_ptr() as usize);
        }
    }

    /// Activate the recorder for the calling thread until the guard drops.
    pub fn guard(&mut self) -> GpuChromeGuard {
        ACTIVE.set(self as *mut GpuChrome as usize);
        GpuChromeGuard
    }

    /// The active recorder (the drawing primitives' hook).
    pub fn active() -> Option<&'static mut GpuChrome> {
        let ptr = ACTIVE.get();
        if ptr == 0 {
            None
        } else {
            // SAFETY: `GpuChromeGuard` owns the scope; the pointer is
            // cleared on drop (normal or unwinding). Single-threaded.
            Some(unsafe { &mut *(ptr as *mut GpuChrome) })
        }
    }

    fn top(&mut self) -> &mut ChromeBuffer {
        self.buffers.last_mut().expect("frame_begin opened the root")
    }

    /// A drawing surface over `pixels` opened (`Pix2D::with_pixels`): push
    /// a tagged buffer unless the top already is that surface (the root
    /// re-bind, e.g. a `PixMap::fill` on draw_area).
    pub fn surface_open(&mut self, tag: usize) {
        if self.buffers.last().map(|b| b.tag == tag).unwrap_or(false) {
            return;
        }
        let staged = self.staged.contains(&tag);
        self.buffers.push(ChromeBuffer {
            tag,
            clip: [0, 0, FRAME_W as i32, FRAME_H as i32],
            quads: Vec::new(),
            staged,
        });
    }

    /// The current surface's clip changed (`Pix2D::set_clipping`).
    pub fn surface_clip(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        if let Some(b) = self.buffers.last_mut() {
            b.clip = [x1, y1, x2, y2];
        }
    }

    /// The current surface was cleared (`Pix2D::cls`): drop its recorded
    /// quads (the surface starts fresh).
    pub fn surface_cls(&mut self) {
        if let Some(b) = self.buffers.last_mut() {
            b.quads.clear();
        }
    }

    /// A coloured rectangle (the `Pix2D` rect/line fills). `alpha` is the
    /// 256-scale CPU alpha (256 = opaque).
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: i32, alpha: i32) {
        let Some((bx, by, bw, bh)) = self.clip_rect(x, y, w, h) else {
            return;
        };
        if self.top().staged {
            return;
        }
        self.quad_count += 1;
        self.top().quads.push(ChromeQuad {
            x: bx as f32,
            y: by as f32,
            w: bw as f32,
            h: bh as f32,
            u_off: 0.0,
            v_off: 0.0,
            u_span: 1.0,
            v_span: 1.0,
            r: ((rgb >> 16) & 0xff) as f32 / 255.0,
            g: ((rgb >> 8) & 0xff) as f32 / 255.0,
            b: (rgb & 0xff) as f32 / 255.0,
            a: alpha.clamp(0, 256) as f32 / 256.0,
            layer: 0,
            region: Region { x: 0, y: 0, w: 1, h: 1 },
        });
    }

    /// A sprite plot (`Pix8::plot_sprite`): upload once into the sprite
    /// atlas and record a textured quad. `alpha` is 256-scale.
    pub fn sprite_pix8(&mut self, sprite: &Pix8, x: i32, y: i32, alpha: i32) {
        let region = self.assets.lock().unwrap().sprite_region_pix8(sprite);
        self.sprite_quad(x + sprite.xof, y + sprite.yof, sprite.wi, sprite.hi, region, alpha);
    }

    /// A `Pix32` sprite plot.
    pub fn sprite_pix32(&mut self, sprite: &Pix32, x: i32, y: i32, alpha: i32) {
        let region = self.assets.lock().unwrap().sprite_region_pix32(sprite);
        self.sprite_quad(x + sprite.xof, y + sprite.yof, sprite.wi, sprite.hi, region, alpha);
    }

    /// A whole-map blit (`PixMap::blit_into`): a staged map uploads its
    /// CPU pixels (per-frame exception), an open surface's buffer pops and
    /// translates into the destination, and a static map draws its cached
    /// atlas region.
    pub fn map_blit(&mut self, src: &PixMap, x: i32, y: i32) {
        let tag = src.pixels.as_ptr() as usize;
        if self.staged.contains(&tag) {
            let region = {
                let mut assets = self.assets.lock().unwrap();
                assets.staged_upload(tag, src)
            };
            self.sprite_quad(x, y, src.width, src.height, region, 256);
            self.buffers.retain(|b| b.tag != tag);
            return;
        }
        // Every open surface over the source map (e.g. the `prepare_title`
        // background + the chrome redraw) pops in order and re-emits
        // translated into the frame (all chrome blits target draw_area).
        let mut i = 0;
        let mut popped = false;
        while i < self.buffers.len() {
            if self.buffers[i].tag == tag {
                let buffer = self.buffers.remove(i);
                self.translate_into_root(buffer, x, y);
                popped = true;
            } else {
                i += 1;
            }
        }
        if popped {
            return;
        }
        // No open surface: a static map (the chrome strips) draws its
        // cached atlas region.
        let region = self.assets.lock().unwrap().map_region(src);
        self.sprite_quad(x, y, src.width, src.height, region, 256);
    }

    /// Drain every open surface for `tag` into the root buffer translated
    /// by `(x, y)` — the composite seam for the `area_game` overlay
    /// surfaces (drawn into by the scene stage, composited at (4, 4)).
    pub fn drain_tagged(&mut self, tag: usize, x: i32, y: i32) {
        let mut i = 0;
        while i < self.buffers.len() {
            if self.buffers[i].tag == tag {
                let buffer = self.buffers.remove(i);
                self.translate_into_root(buffer, x, y);
            } else {
                i += 1;
            }
        }
    }

    /// A font glyph (the `PixFont::plot_letter` hook): upload the mask
    /// into the font atlas on first use and record a tinted quad.
    pub fn glyph(&mut self, mask: &[i8], w: i32, h: i32, x: i32, y: i32, rgb: i32, alpha: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let region = self.assets.lock().unwrap().glyph_region(mask, w as u32, h as u32);
        let Some((bx, by, bw, bh)) = self.clip_rect(x, y, w, h) else {
            return;
        };
        if self.top().staged {
            return;
        }
        self.quad_count += 1;
        self.top().quads.push(ChromeQuad {
            x: bx as f32,
            y: by as f32,
            w: bw as f32,
            h: bh as f32,
            u_off: (bx - x) as f32 / w as f32,
            v_off: (by - y) as f32 / h as f32,
            u_span: bw as f32 / w as f32,
            v_span: bh as f32 / h as f32,
            r: ((rgb >> 16) & 0xff) as f32 / 255.0,
            g: ((rgb >> 8) & 0xff) as f32 / 255.0,
            b: (rgb & 0xff) as f32 / 255.0,
            a: alpha.clamp(0, 256) as f32 / 256.0,
            layer: 2,
            region,
        });
    }

    /// Clip `(x, y, w, h)` to the current surface's clip rect.
    fn clip_rect(&self, x: i32, y: i32, w: i32, h: i32) -> Option<(i32, i32, i32, i32)> {
        let clip = self.buffers.last()?.clip;
        let x0 = x.max(clip[0]);
        let y0 = y.max(clip[1]);
        let x1 = (x + w).min(clip[2]);
        let y1 = (y + h).min(clip[3]);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some((x0, y0, x1 - x0, y1 - y0))
    }

    /// A textured quad from `region` of the sprite atlas (the UV sub-rect
    /// follows the clip).
    fn sprite_quad(&mut self, x: i32, y: i32, w: i32, h: i32, region: Region, alpha: i32) {
        let Some((bx, by, bw, bh)) = self.clip_rect(x, y, w, h) else {
            return;
        };
        if self.top().staged {
            return;
        }
        self.quad_count += 1;
        self.top().quads.push(ChromeQuad {
            x: bx as f32,
            y: by as f32,
            w: bw as f32,
            h: bh as f32,
            u_off: (bx - x) as f32 / w as f32,
            v_off: (by - y) as f32 / h as f32,
            u_span: bw as f32 / w as f32,
            v_span: bh as f32 / h as f32,
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: alpha.clamp(0, 256) as f32 / 256.0,
            layer: 1,
            region,
        });
    }

    fn translate_into_root(&mut self, buffer: ChromeBuffer, x: i32, y: i32) {
        if buffer.quads.is_empty() {
            return;
        }
        let root = &mut self.buffers[0];
        for mut q in buffer.quads {
            q.x += x as f32;
            q.y += y as f32;
            root.quads.push(q);
        }
    }

    /// Flush the root quads into `target` (the full-frame texture): one
    /// vertex-buffer upload, then one draw call per contiguous same-layer
    /// run (the quads keep the CPU's exact draw order).
    pub fn flush(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let quads = std::mem::take(&mut self.buffers[0].quads);
        self.quad_count = quads.len();
        LAST_QUAD_COUNT.store(quads.len(), std::sync::atomic::Ordering::Relaxed);
        if quads.is_empty() {
            return;
        }
        let bytes = quads.len() * 6 * std::mem::size_of::<ChromeVertex>();
        if bytes > self.vertex_buf_capacity {
            self.vertex_buf_capacity = bytes;
            self.vertex_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("r274 chrome vertices"),
                size: bytes as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        // Resolve each quad's region to atlas UVs against the current
        // atlas size (the atlases may have grown).
        let (sprite_size, font_size) = {
            let assets = self.assets.lock().unwrap();
            (assets.sprite_atlas_size(), assets.font_atlas_size())
        };
        let mut vertices = Vec::with_capacity(quads.len() * 6);
        for q in &quads {
            let (au, ah) = if q.layer == 1 {
                (sprite_size.0 as f32, sprite_size.1 as f32)
            } else {
                (font_size.0 as f32, font_size.1 as f32)
            };
            let u0 = (q.region.x as f32 + q.u_off * q.region.w as f32) / au;
            let v0 = (q.region.y as f32 + q.v_off * q.region.h as f32) / ah;
            let u1 = (q.region.x as f32 + (q.u_off + q.u_span) * q.region.w as f32) / au;
            let v1 = (q.region.y as f32 + (q.v_off + q.v_span) * q.region.h as f32) / ah;
            for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)] {
                vertices.push(ChromeVertex {
                    x: q.x + dx * q.w,
                    y: q.y + dy * q.h,
                    u: u0 + dx * (u1 - u0),
                    v: v0 + dy * (v1 - v0),
                    r: q.r,
                    g: q.g,
                    b: q.b,
                    a: q.a,
                    layer: q.layer,
                });
            }
        }
        self.queue
            .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&vertices));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("r274 chrome pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        let assets = self.assets.lock().unwrap();
        pass.set_bind_group(0, assets.chrome_bind_group(), &[]);
        drop(assets);
        pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
        let mut start = 0usize;
        while start < quads.len() {
            let layer = quads[start].layer;
            let mut end = start + 1;
            while end < quads.len() && quads[end].layer == layer {
                end += 1;
            }
            pass.draw((start * 6) as u32..(end * 6) as u32, 0..1);
            start = end;
        }
    }
}

/// A guard that activates the recorder for the calling thread's draw
/// calls; cleared on drop (normal or unwinding).
pub struct GpuChromeGuard;

impl Drop for GpuChromeGuard {
    fn drop(&mut self) {
        ACTIVE.set(0);
    }
}

/// Quads flushed by the last frame (the chrome-as-quads test reads this).
pub fn last_quad_count() -> usize {
    LAST_QUAD_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}
