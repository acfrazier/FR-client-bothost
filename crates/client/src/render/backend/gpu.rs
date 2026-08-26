//! The wgpu backend (task 7): the 3D scene is rasterized on the GPU into
//! a texture and the 2D chrome draws on top as GPU quads (task 5 of the
//! GPU-chrome campaign) — the whole frame is composited on the GPU and
//! handed to the host as a full-frame texture, with no scene readback. The
//! scene mesh (`RenderWorld::build_scene_mesh`) transforms the marked
//! tiles' ground, walls, decor, objects and sprites to camera space with
//! the exact CPU fixed-point math; the vertex shader does the perspective
//! divide (near-clipping at z = 50 for free) and the depth buffer replaces
//! the CPU painter's priority merge. The `CpuBackend` stays the
//! pixel-faithful oracle/fallback.
//!
//! One device/queue per process: the first `GpuBackend::try_new`
//! initialises a shared `GpuContext` (`CONTEXT`, a `OnceLock`) and every
//! backend clones it — or the host injects its own device/queue first via
//! [`inject_device`] (the shared-device seam), which wins the slot. A
//! failed self-init is cached and logged once; the renderer falls back to
//! `CpuBackend` — a missing GPU never takes the bot down. The
//! `R274_TEST_FORCE_NO_GPU` env var forces an init failure for the
//! selection test.
//!
//! Known divergences from the CPU path (documented in `render/world.rs`):
//! textured faces sample the model texture array (see `gpu_atlas.rs`), mouse picks
//! are AABB-only, the occluder tests are skipped (the depth buffer
//! occludes), and the chrome draws as GPU quads (rects/sprites/glyphs)
//! with the rotated minimap composited from a CPU staging upload.

use std::sync::{Arc, Mutex, OnceLock};

use crate::client::client::Client;
use crate::graphics::{Pix2D, Pix3D, Pix3DDraw};
use crate::render::backend::gpu_atlas::GpuAssets;
use crate::render::backend::gpu_chrome::GpuChrome;
use crate::render::backend::{BackendKind, CpuBackend, FrameKind, FrameOutput, RenderBackend, TextureHandle};
use crate::render::world::{GpuVertex, SceneMesh};
use crate::render::Renderer;
use crate::render::draw::get_av_h;

/// The wgpu scene texture size: `area_game` is 512×334, blitted at (4, 4).
const SCENE_W: u32 = 512;
const SCENE_H: u32 = 334;

/// The full-frame size: the 765×503 applet `draw_area`.
const FRAME_W: u32 = crate::client::client::APPLET_W as u32;
const FRAME_H: u32 = crate::client::client::APPLET_H as u32;

/// The shared wgpu device/queue home (one per process). `Result` so a
/// failed init is cached: the fallback log fires once and later renderers
/// do not retry (and cannot take the bot down).
static CONTEXT: OnceLock<Result<Arc<GpuContext>, String>> = OnceLock::new();

/// Shared-device seam (campaign task 3): hand the host's wgpu device/queue
/// to the process-wide GPU context so the client renders on the host's
/// device and the host binds the frame texture directly (no read-back
/// round-trip). Call before any `GpuBackend` exists; the first context
/// wins — an injection arriving after a self-init (or vice versa) is a
/// no-op, so a slot renderer can never create its own device once the host
/// injected. The headless host never injects and falls back to `init_gpu`.
pub fn inject_device(device: wgpu::Device, queue: wgpu::Queue) {
    CONTEXT.get_or_init(|| {
        let assets = Arc::new(Mutex::new(GpuAssets::new(&device, &queue)));
        Ok(Arc::new(GpuContext { device, queue, assets }))
    });
}

/// Whether the init failure has been logged (once per process).
static FAILURE_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How many times `GpuBackend::try_new` asked for a wgpu device (task 8).
/// The headless proof (`tests/headless.rs`) asserts a pure `Client` run
/// keeps it at 0.
static GPU_BACKEND_TRIED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The shared asset store (model texture array; the chrome atlases in
    /// the later slices). Uploads happen once per process on first use.
    assets: Arc<Mutex<GpuAssets>>,
}

/// The process-wide GPU context, or `None` on a (cached) init failure.
fn context() -> Option<Arc<GpuContext>> {
    let result = CONTEXT.get_or_init(|| {
        if std::env::var_os("R274_TEST_FORCE_NO_GPU").is_some() {
            return Err("GPU init disabled by R274_TEST_FORCE_NO_GPU (test hook)".into());
        }
        init_gpu()
    });
    match result {
        Ok(ctx) => Some(ctx.clone()),
        Err(err) => {
            if !FAILURE_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                eprintln!("gpu: {err}; falling back to the CPU backend");
            }
            None
        }
    }
}

fn init_gpu() -> Result<Arc<GpuContext>, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|e| format!("no adapter: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("r274 client"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::default(),
    }))
    .map_err(|e| format!("device: {e}"))?;
    let assets = Arc::new(Mutex::new(GpuAssets::new(&device, &queue)));
    Ok(Arc::new(GpuContext { device, queue, assets }))
}

/// The scene pipeline: a passthrough projection (camera-space vertices,
/// perspective divide in the shader) plus the packed RuneLite-GPU-plugin
/// vertex (`abhsl`, `uv_tex`, `v`). Flat faces convert the raw 16-bit
/// shade to RGB via `hslToRgb` in the vertex shader (so gouraud faces
/// interpolate RGB, not the packed shade); textured faces sample the
/// shared `texture_2d_array` by clamped layer (texture id), the shade's
/// top two bits picking the CPU's brightness level.
const SCENE_SHADER: &str = r#"
const NEAR: f32 = 50.0;
const SCALE_X: f32 = 2.0;
const SCALE_Y: f32 = -512.0 / 167.0;

@group(0) @binding(0) var model_atlas: texture_2d_array<f32>;
@group(0) @binding(1) var model_sampler: sampler;

struct SceneUniforms {
    brightness: f32,
};
@group(1) @binding(0) var<uniform> scene: SceneUniforms;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) abhsl: u32,
    @location(2) uv_tex: u32,
    @location(3) v: u32,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) hsl: f32,
    @location(2) u: f32,
    @location(3) v: f32,
    @location(4) @interpolate(flat) tex_id: u32,
    @location(5) alpha: f32,
};

// `build_colour_table`'s HSL→RGB + gamma (the `hsl_to_rgb.glsl` port):
// flat faces must match `Pix3D::colour_table()[shade] / 256.0`.
fn hslToRgb(hsl: u32) -> vec3<f32> {
    let hue = f32((hsl >> 10) & 0x3fu) / 64.0 + 0.0078125;
    let sat = f32((hsl >> 7) & 0x7u) / 8.0 + 0.0625;
    let lum = f32(hsl & 0x7fu);
    let var11 = lum / 128.0;
    var r = var11;
    var g = var11;
    var b = var11;

    let var19 = select(var11 + sat - var11 * sat, var11 * (1.0 + sat), var11 < 0.5);
    let var21 = 2.0 * var11 - var19;
    var var23 = hue + 0.3333333333333333;
    if (var23 > 1.0) { var23 -= 1.0; }
    var var27 = hue - 0.3333333333333333;
    if (var27 < 0.0) { var27 += 1.0; }

    if (6.0 * var23 < 1.0) {
        r = var21 + (var19 - var21) * 6.0 * var23;
    } else if (2.0 * var23 < 1.0) {
        r = var19;
    } else if (3.0 * var23 < 2.0) {
        r = var21 + (var19 - var21) * (0.6666666666666666 - var23) * 6.0;
    } else {
        r = var21;
    }

    if (6.0 * hue < 1.0) {
        g = var21 + (var19 - var21) * 6.0 * hue;
    } else if (2.0 * hue < 1.0) {
        g = var19;
    } else if (3.0 * hue < 2.0) {
        g = var21 + (var19 - var21) * (0.6666666666666666 - hue) * 6.0;
    } else {
        g = var21;
    }

    if (6.0 * var27 < 1.0) {
        b = var21 + (var19 - var21) * 6.0 * var27;
    } else if (2.0 * var27 < 1.0) {
        b = var19;
    } else if (3.0 * var27 < 2.0) {
        b = var21 + (var19 - var21) * (0.6666666666666666 - var27) * 6.0;
    } else {
        b = var21;
    }

    // `build_colour_table` truncates each channel to `(r * 256)` and gamma
    // corrects that quantized value, so mirror the double rounding or dark
    // shades drift well off the CPU table.
    let r0 = floor(r * 256.0) / 256.0;
    let g0 = floor(g * 256.0) / 256.0;
    let b0 = floor(b * 256.0) / 256.0;
    return vec3<f32>(pow(r0, scene.brightness), pow(g0, scene.brightness), pow(b0, scene.brightness));
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let z = in.pos.z;
    let alpha = f32((in.abhsl >> 24) & 0xffu) / 255.0;
    let bias = (in.abhsl >> 16) & 0xffu;
    let hsl = in.abhsl & 0xffffu;
    let tex_id = in.uv_tex & 0xffffu;
    let u = (in.uv_tex >> 16) & 0xffffu;

    // The projection mirrors the CPU `origin + (x << 9) / z` (origin = the
    // 512×334 area_game centre). clip.z is set so the interpolated depth
    // is perspective-correct and stays in [0, 1] for every z >= 50. The
    // face priority biases it *nearer* (higher priority wins under
    // `CompareFunction::Less`), matching the CPU painter's ascending
    // priority-bucket order; priorities are ≤ ~11, so bias/128 ≤ ~0.086
    // and the near plane is never crossed.
    out.position = vec4<f32>(in.pos.x * SCALE_X, in.pos.y * SCALE_Y, z - NEAR - f32(bias) / 128.0, z);
    out.color = hslToRgb(hsl);
    out.hsl = f32(hsl);
    out.u = f32(u);
    out.v = f32(in.v);
    out.tex_id = tex_id;
    out.alpha = alpha;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.tex_id > 0u) {
        // The `texture_2d_array` has one 128×128 layer per texture id;
        // the packed `tex_id` is texture id + 1. The fixed-point 0..255
        // u/v maps across the whole layer (u/256 × 128 = u/2), sampled
        // with the layer clamped to the array depth (RuneLite `frag.glsl`
        // samples `textures` by `fTextureId - 1`; ids beyond the array
        // clamp instead of dropping, so textured faces never vanish).
        let id = min(in.tex_id - 1u, 49u);
        let uv = clamp(vec2<f32>(in.u * 0.5, in.v * 0.5) / 128.0, vec2<f32>(0.0), vec2<f32>(1.0));
        let t = textureSample(model_atlas, model_sampler, uv, i32(id));
        if (t.a < 0.5) { discard; }
        // The CPU's per-texel brightness: the interpolated 16-bit shade's
        // top two bits select the texel block (rgb, ~7/8, ~3/4, ~5/8).
        let level = (i32(in.hsl) >> 14) & 3;
        let factor = 1.0 - f32(level) * 0.125;
        return vec4<f32>(t.rgb * factor, in.alpha);
    }
    return vec4<f32>(in.color, in.alpha);
}
"#;

/// The wgpu scene backend (see the module docs). `begin`/`chrome` delegate
/// to the inner `CpuBackend` (the 2D chrome stays CPU this slice); `scene`
/// rasterizes the 3D world into `scene_texture`; `composite_scene` copies
/// the scene into the full-frame texture at (4, 4); `finish` hands the
/// full-frame texture to the host.
pub struct GpuBackend {
    context: Arc<GpuContext>,
    scene_texture: wgpu::Texture,
    scene_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    /// The full-frame 765×503 composite (scene at (4, 4) + chrome), the
    /// texture `finish` returns.
    frame_texture: wgpu::Texture,
    frame_view: wgpu::TextureView,
    pipeline_opaque: wgpu::RenderPipeline,
    pipeline_translucent: wgpu::RenderPipeline,
    /// The shared model-texture-array bind group, bound each scene pass.
    model_bind_group: wgpu::BindGroup,
    /// The scene-shader brightness uniform (group 1): the current
    /// `Pix3D::colour_table` gamma, so `hslToRgb` matches the CPU table.
    brightness_bind_group: wgpu::BindGroup,
    brightness_buf: wgpu::Buffer,
    /// Streaming vertex buffer (grows on demand; the mesh is re-uploaded
    /// every frame, the RuneLite plugin shape).
    vertex_buf: wgpu::Buffer,
    vertex_buf_capacity: usize,
    /// The 2D chrome/title half.
    cpu: CpuBackend,
    /// The chrome quad recorder + pipeline (task 5c): the chrome draws as
    /// GPU quads flushed into the full-frame texture.
    chrome: GpuChrome,
    /// A scene was rendered this frame (a title frame, or an in-game frame
    /// with no built scene, leaves `composite_scene` a no-op like the CPU
    /// path's frozen-frame blit).
    scene_ready: bool,
}

impl GpuBackend {
    /// Initialise the process-wide device/queue and build the scene
    /// pipeline. Returns `Err` on any init failure (or a cached one); the
    /// renderer logs and falls back to `CpuBackend`.
    pub fn try_new() -> Result<GpuBackend, String> {
        GPU_BACKEND_TRIED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let context = context().ok_or_else(|| "no GPU context".to_string())?;

        let scene_texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 scene"),
            size: wgpu::Extent3d { width: SCENE_W, height: SCENE_H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Not sRGB: the colour-table values are already display-encoded.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let scene_view = scene_texture.create_view(&Default::default());
        let depth_texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 scene depth"),
            size: wgpu::Extent3d { width: SCENE_W, height: SCENE_H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&Default::default());

        let frame_texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 frame"),
            size: wgpu::Extent3d { width: FRAME_W, height: FRAME_H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // TEXTURE_BINDING: the shared-device seam — the host samples
            // the frame texture directly in its ImGui renderer.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let frame_view = frame_texture.create_view(&Default::default());

        let shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("r274 scene shader"),
                source: wgpu::ShaderSource::Wgsl(SCENE_SHADER.into()),
            });

        let brightness_layout =
            context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("r274 scene uniforms layout"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(16),
                        },
                        count: None,
                    }],
                });
        let brightness_buf = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 scene uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let brightness_bind_group = context.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("r274 scene uniforms group"),
            layout: &brightness_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &brightness_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(16),
                }),
            }],
        });

        let assets = context.assets.lock().unwrap();
        let layout = context
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("r274 scene layout"),
                bind_group_layouts: &[Some(&assets.model_bind_group_layout), Some(&brightness_layout)],
                immediate_size: 0,
            });
        let pipeline_opaque = make_pipeline(&context.device, &layout, &shader, true);
        let pipeline_translucent = make_pipeline(&context.device, &layout, &shader, false);
        let model_bind_group = assets.model_bind_group.clone();
        drop(assets);

        let vertex_buf_capacity = 1 << 20; // 1 MiB of vertices to start
        let vertex_buf = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 scene vertices"),
            size: vertex_buf_capacity as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let chrome = GpuChrome::new(&context.device, &context.queue, Arc::clone(&context.assets));
        Ok(GpuBackend {
            context,
            scene_texture,
            scene_view,
            depth_view,
            frame_texture,
            frame_view,
            pipeline_opaque,
            pipeline_translucent,
            model_bind_group,
            brightness_bind_group,
            brightness_buf,
            vertex_buf,
            vertex_buf_capacity,
            cpu: CpuBackend,
            chrome,
            scene_ready: false,
        })
    }

    /// wgpu device initialisations attempted in this process (the task-8
    /// headless counter; a cached failure still counts one attempt).
    pub fn tried() -> usize {
        GPU_BACKEND_TRIED.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Test hook: the device the process-wide context renders on (the
    /// host's device after injection, or the self-init fallback). Lets the
    /// shared-device test assert which device won the context.
    #[doc(hidden)]
    pub fn device(&self) -> &wgpu::Device {
        &self.context.device
    }

    /// Test hook: upload the model textures from `pix`, render `mesh` into
    /// the scene texture and read the scene back (the model-texture
    /// sampling test asserts the rendered texels are non-white). The
    /// production frame never reads back — `finish` hands the texture.
    #[doc(hidden)]
    pub fn render_scene_for_test(&mut self, mesh: SceneMesh, pix: &Pix3DDraw) -> Vec<i32> {
        self.context
            .assets
            .lock()
            .unwrap()
            .ensure_model_textures(pix);
        self.render_scene(mesh);
        let handle = TextureHandle {
            device: self.context.device.clone(),
            queue: self.context.queue.clone(),
            view: self.scene_view.clone(),
            width: SCENE_W,
            height: SCENE_H,
        };
        handle.read_back()
    }

    /// Chrome quads the last flushed frame recorded (the chrome-as-quads
    /// test).
    pub fn chrome_quad_count() -> usize {
        crate::render::backend::gpu_chrome::last_quad_count()
    }

    /// Upload the mesh and rasterize it into `scene_texture` (opaque faces
    /// first with depth writes, then the translucent faces alpha-blended).
    /// No readback: the composite step copies the texture into the
    /// full-frame on the GPU.
    fn render_scene(&mut self, mesh: SceneMesh) {
        let opaque_len = mesh.opaque_len();
        let vertices = mesh.vertices();
        if vertices.is_empty() {
            self.scene_ready = false;
            return;
        }
        let bytes = vertices.len() * std::mem::size_of::<GpuVertex>();
        if bytes > self.vertex_buf_capacity {
            self.vertex_buf_capacity = bytes;
            self.vertex_buf = self.context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("r274 scene vertices"),
                size: bytes as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.context
            .queue
            .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&vertices));
        // The scene shader's `hslToRgb` gamma must match the CPU colour
        // table the flat-face shades were built with (process-wide).
        let brightness = [Pix3D::colour_brightness() as f32, 0.0f32, 0.0f32, 0.0f32];
        self.context
            .queue
            .write_buffer(&self.brightness_buf, 0, bytemuck::cast_slice(&brightness));

        let mut encoder = self
            .context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("r274 scene encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("r274 scene pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.scene_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
            pass.set_bind_group(0, &self.model_bind_group, &[]);
            pass.set_bind_group(1, &self.brightness_bind_group, &[]);
            if opaque_len > 0 {
                pass.set_pipeline(&self.pipeline_opaque);
                pass.draw(0..opaque_len as u32, 0..1);
            }
            if opaque_len < vertices.len() {
                pass.set_pipeline(&self.pipeline_translucent);
                pass.draw(opaque_len as u32..vertices.len() as u32, 0..1);
            }
        }
        self.context.queue.submit([encoder.finish()]);
        self.scene_ready = true;
    }
}

/// One of the two scene pipelines (same shaders; the translucent pipeline
/// alpha-blends and does not write depth).
fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    opaque: bool,
) -> wgpu::RenderPipeline {
    let vertex = wgpu::VertexState {
        module: shader,
        entry_point: Some("vs_main"),
        compilation_options: Default::default(),
        buffers: &[wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3,
                1 => Uint32,
                2 => Uint32,
                3 => Uint32,
            ],
        }],
    };
    let fragment = wgpu::FragmentState {
        module: shader,
        entry_point: Some("fs_main"),
        compilation_options: Default::default(),
        targets: &[Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba8Unorm,
            blend: if opaque {
                None
            } else {
                Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                })
            },
            write_mask: wgpu::ColorWrites::ALL,
        })],
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if opaque { "r274 scene opaque" } else { "r274 scene translucent" }),
        layout: Some(layout),
        vertex,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None, // the CPU winding test already culled backfaces
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(opaque),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(fragment),
        multiview_mask: None,
        cache: None,
    })
}

impl RenderBackend for GpuBackend {
    /// Frame start on the GPU path: open the chrome recorder's frame
    /// (the root quad list, the staged minimap/flame maps), run the CPU
    /// prep (prepare_game/prepare_title, the deferred brightness), and
    /// record the chrome-frame strip blits — every frame, because the GPU
    /// frame is rebuilt from quads each frame instead of a persistent
    /// `draw_area`.
    fn begin(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        let _guard = self.chrome.guard();
        self.chrome.frame_begin(&r.draw_area);
        if kind == FrameKind::Title {
            if !core.ingame && core.redraw_frame {
                r.unload_title();
                r.image_title2 = None;
                r.draw_area.fill(0);
            }
            r.prepare_title(core);
        } else {
            // TS GameShell ticks `scrollCycle` each mainloop pass while the
            // mouse is held (Client.ts 2341-2343); 0/1 here is enough for the
            // held-arrow scrollbar repeat.
            core.scroll_cycle = if core.shell.mouse_button != 0 { 1 } else { 0 };
            // `apply_clientcode` (sim) defers the brightness re-gamma to the
            // renderer's texel state (task-2b bridge).
            if let Some(brightness) = core.pending_brightness.take() {
                r.pix3d.init_texture_palettes(brightness);
                r.pix3d.refresh_texture_averages();
                core.tex_average = r.pix3d.tex_average;
            }
            r.prepare_game(core);
        }
        // The staged maps exist only after the prep above allocated them;
        // mark them now (the first frame's prep happens under this frame's
        // recorder, and their blits must stage, not cache, from the start).
        self.chrome.mark_staged(r.area_map.as_ref());
        self.chrome.mark_staged(r.image_title0.as_ref());
        self.chrome.mark_staged(r.image_title1.as_ref());

        // The loading splash draws into `draw_area` in `mainredraw`
        // (outside the frame stages); re-run it under the recorder so the
        // GPU frame carries it too.
        if kind == FrameKind::Game && core.scene_state != 2 && core.draw {
            r.scene_loading_splash(core);
        }

        if kind == FrameKind::Game {
            // The chrome-frame strips (the CPU path blits them once per
            // `redraw_frame`; the GPU path re-records them every frame).
            if let Some(b) = &r.area_backleft1 {
                b.blit_into(&mut r.draw_area, 0, 4);
            }
            if let Some(b) = &r.area_backleft2 {
                b.blit_into(&mut r.draw_area, 0, 357);
            }
            if let Some(b) = &r.area_backright1 {
                b.blit_into(&mut r.draw_area, 722, 4);
            }
            if let Some(b) = &r.area_backright2 {
                b.blit_into(&mut r.draw_area, 743, 205);
            }
            if let Some(b) = &r.area_backtop1 {
                b.blit_into(&mut r.draw_area, 0, 0);
            }
            if let Some(b) = &r.area_backvmid1 {
                b.blit_into(&mut r.draw_area, 516, 4);
            }
            if let Some(b) = &r.area_backvmid2 {
                b.blit_into(&mut r.draw_area, 516, 205);
            }
            if let Some(b) = &r.area_backvmid3 {
                b.blit_into(&mut r.draw_area, 496, 357);
            }
            if let Some(b) = &r.area_backhmid2 {
                b.blit_into(&mut r.draw_area, 0, 338);
            }
            core.redraw_frame = false;
        }
    }

    /// `gameDrawMain`'s 3D pass on the GPU: the same entity/camera/prep
    /// steps as the CPU backend, then `prepare_scene` (draw-front marking,
    /// share-light) + the scene mesh rasterized into the wgpu scene
    /// texture. The overlays (chat bubbles, modal, the minimenu) draw into
    /// `area_game` as on the CPU path and record as quads (the composite
    /// lands them over the scene at (4, 4)).
    fn scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        let _guard = self.chrome.guard();
        if kind != FrameKind::Game || core.scene_state != 2 {
            self.scene_ready = false;
            return;
        }

        r.scene_cycle += 1;
        r.add_players(core, true);
        r.add_npcs(core, true);
        r.add_players(core, false);
        r.add_npcs(core, false);
        r.add_projectiles(core);
        r.add_map_anim(core);

        let mut pitch = core.orbit_camera_pitch;
        if core.camera_pitch_clamp / 256 > pitch {
            pitch = core.camera_pitch_clamp / 256;
        }
        if core.cam_shake[4] && core.cam_shake_ran[4] + 128 > pitch {
            pitch = core.cam_shake_ran[4] + 128;
        }
        let yaw = (core.orbit_camera_yaw + core.macro_camera_angle) & 0x7ff;

        if !core.cinema_cam {
            if let Some(player) = &core.local_player {
                let target_y =
                    get_av_h(&core.groundh, &core.mapl, player.x, player.z, core.minusedlevel) - 50;
                r.cam_follow(
                    core,
                    pitch,
                    yaw,
                    core.orbit_camera_x,
                    target_y,
                    core.orbit_camera_z,
                    pitch * 3 + 600,
                );
            }
        }

        let level = if core.cinema_cam {
            r.roof_check2(core)
        } else {
            r.roof_check(core)
        };

        let eye_x = core.cam_x;
        let eye_y = core.cam_y;
        let eye_z = core.cam_z;
        let eye_pitch = core.cam_pitch;
        let eye_yaw = core.cam_yaw;

        let (cam_x, cam_y, cam_z, cam_pitch, cam_yaw) =
            r.cam_shake_jitter(core, eye_x, eye_y, eye_z, eye_pitch, eye_yaw);

        if !r.vis_calc_done {
            r.vis_calc_done = true;
            let mut distance = [0i32; 9];
            for (x, slot) in distance.iter_mut().enumerate() {
                let angle = x as i32 * 32 + 128 + 15;
                let offset = angle * 3 + 600;
                let sin = Pix3D::sin_table().get(angle as usize).copied().unwrap_or(0);
                *slot = (offset * sin) >> 16;
            }
            r.world.reset_vis_calc(&distance, 500, 800, 512, 334);
        }

        let cycle = r.pix3d.cycle;
        r.pix3d.mouse_check = true;
        r.pix3d.picked_count = 0;
        r.pix3d.mouse_x = core.shell.mouse_x - 4;
        r.pix3d.mouse_y = core.shell.mouse_y - 4;
        // The projection origin the mesh builder's winding/pick tests read
        // (the CPU scene binds it via `set_clipping` on `area_game`).
        r.pix3d.set_clipping(512, 334);

        let cache = &core.cache;
        let loop_cycle = core.loop_cycle;

        // The GPU rasterization: mark the visible tiles, build the scene
        // mesh (this also resolves the lazy model caches, appends the
        // AABB mouse picks and runs the ground click raycast), render it
        // into `scene_texture`.
        // Model textures upload into the shared array once (per texture
        // id), before the mesh is built so the ids resolve to layers.
        self.context
            .assets
            .lock()
            .unwrap()
            .ensure_model_textures(&r.pix3d);
        r.world
            .prepare_scene(&mut core.world, cache, loop_cycle, cam_x, cam_y, cam_z, level, cam_yaw, cam_pitch);
        let mesh = r
            .world
            .build_scene_mesh(&mut core.world, cache, loop_cycle, &mut r.pix3d);
        self.render_scene(mesh);

        // The overlays (chat bubbles, modal, minimenu, crosshair) draw into
        // `area_game`, recorded as quads over the scene. Clear it exactly
        // like the CPU path's `surface.cls()` before the world pass, so no
        // stale overlay pixels ghost frame-to-frame.
        if let Some(game) = r.area_game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            surface.cls();
        }

        r.world.remove_sprites(&mut core.world);
        r.entity_overlays(core);
        r.coord_arrow(core);
        r.texture_run_anims(core, cycle);

        core.pick_count = r.pix3d.picked_count;
        core.pick_typecodes.copy_from_slice(&r.pix3d.picked_entity_typecode);
        r.other_overlays(core);

        core.cam_x = eye_x;
        core.cam_y = eye_y;
        core.cam_z = eye_z;
        core.cam_pitch = eye_pitch;
        core.cam_yaw = eye_yaw;
    }

    /// The composite seam on the GPU path: land the `area_game` overlay
    /// quads (drawn by the scene stage) over the scene at the (4, 4)
    /// point, into the root quad list. The scene texture itself is copied
    /// into the full-frame by `finish`, before the chrome flush.
    fn composite_scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        let _guard = self.chrome.guard();
        if kind == FrameKind::Game && core.scene_state == 2 {
            if let Some(game) = &r.area_game {
                self.chrome.drain_tagged(game.pixels.as_ptr() as usize, 4, 4);
            }
        }
    }

    /// 2D chrome as GPU quads: the `CpuBackend` chrome body runs with the
    /// recorder active (its `Pix2D`/sprite/font/blit calls record quads)
    /// and every area redraw forced, because the GPU frame is rebuilt each
    /// frame instead of a persistent `draw_area`.
    fn chrome(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        let _guard = self.chrome.guard();
        if kind == FrameKind::Game {
            core.redraw_side = true;
            core.redraw_chat = true;
            core.redraw_icons = true;
            core.redraw_chat_mode = true;
        }
        self.cpu.chrome(core, r, kind);
    }

    /// The full-frame composite: clear the frame, copy the GPU scene at
    /// (4, 4), draw the recorded chrome quads over it, and hand the frame
    /// texture to the host. No readback.
    fn finish(&mut self, _r: &mut Renderer) -> FrameOutput {
        let _guard = self.chrome.guard();
        let mut encoder = self
            .context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("r274 frame encoder"),
            });
        {
            // An empty clear pass (begin + immediate end) — the frame
            // starts black each frame.
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("r274 frame clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.frame_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        if self.scene_ready {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.scene_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.frame_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 4, y: 4, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d { width: SCENE_W, height: SCENE_H, depth_or_array_layers: 1 },
            );
        }
        self.chrome.flush(&mut encoder, &self.frame_view);
        self.context.queue.submit([encoder.finish()]);
        FrameOutput::Texture(TextureHandle {
            device: self.context.device.clone(),
            queue: self.context.queue.clone(),
            view: self.frame_view.clone(),
            width: FRAME_W,
            height: FRAME_H,
        })
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Gpu
    }
}
