//! The wgpu backend (task 7): the 3D scene is rasterized on the GPU into
//! a texture and the 2D chrome composites over it as one CPU texture
//! upload — the RuneLite pattern (the game's CPU immediate-mode draws the
//! UI into a canvas, and the GPU plugin uploads that canvas and draws it
//! over the scene). The whole frame is composited on the GPU and handed to
//! the host as a full-frame texture, with no scene readback. The scene
//! mesh (`RenderWorld::build_scene_mesh`) transforms the marked tiles'
//! ground, walls, decor, objects and sprites to camera space with the
//! exact CPU fixed-point math; the vertex shader does the perspective
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
//! textured faces sample the model texture array (see `gpu_atlas.rs`), loc
//! mouse picks use the CPU AABB pre-test plus the render2 face bbox,
//! the occluder tests are skipped (the depth buffer
//! occludes), and the 2D chrome draws on the CPU into the persistent
//! `draw_area`, which `finish` uploads as one RGBA8 texture and composites
//! over the scene. Scene-window overlay alpha is the coverage byte
//! (255 = opaque chrome, 0 = the 3D hole, 1..=254 = translucent nav paint).

use std::sync::{Arc, Mutex, OnceLock};

use crate::client::client::Client;
use crate::graphics::pix2d::coverage_guard;
use crate::graphics::{Pix2D, Pix3D, Pix3DDraw, PixMap};
use crate::render::backend::gpu_atlas::GpuAssets;
use crate::render::backend::{
    BackendKind, CpuBackend, FrameKind, FrameOutput, RenderBackend, TextureHandle,
};
use crate::render::draw::get_av_h;
use crate::render::world::{GpuVertex, SceneMesh};
use crate::render::Renderer;

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
    CONTEXT.get_or_init(|| Ok(GpuContext::new(device, queue)));
}

/// Whether the init failure has been logged (once per process).
static FAILURE_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How many times `GpuBackend::try_new` asked for a wgpu device (task 8).
/// The headless proof (`tests/headless.rs`) asserts a pure `Client` run
/// keeps it at 0.
static GPU_BACKEND_TRIED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many times the shared context built its shader modules / pipelines
/// (task 6): the `OnceLock` means the first `GpuContext::new` builds them
/// and a second `GpuBackend::try_new` reuses them, so two heads pay one
/// `create_shader_module` batch.
static SHADER_MODULES_CREATED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// The process-wide wgpu state (one per process): the device/queue, the
/// shared asset store, and the immutable scene/chrome shader modules and
/// pipelines (task 6 — `GpuBackend::try_new` must not build a pipeline per
/// head). `GpuBackend` keeps only the per-view textures + vertex buffer.
struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The shared asset store (model texture array; the chrome atlases in
    /// the later slices). Uploads happen once per process on first use.
    assets: Arc<Mutex<GpuAssets>>,
    /// The scene-uniforms bind group layout (the per-backend brightness
    /// buffer binds against it).
    scene_brightness_layout: wgpu::BindGroupLayout,
    /// The scene pass pipelines (opaque first with depth writes, then the
    /// translucent faces alpha-blended). The pipelines hold the scene
    /// shader module (created once, here), so no field keeps it.
    pipeline_opaque: wgpu::RenderPipeline,
    pipeline_translucent: wgpu::RenderPipeline,
    /// The chrome composite pipeline (the CPU `draw_area` upload draws
    /// over the scene); holds the chrome shader module.
    chrome_layout: wgpu::BindGroupLayout,
    chrome_pipeline: wgpu::RenderPipeline,
}

impl GpuContext {
    /// Build the process-wide context: the shared assets plus the scene
    /// and chrome shader modules / pipelines, exactly once (the `CONTEXT`
    /// `OnceLock` wins — a second `GpuBackend::try_new` reuses these).
    fn new(device: wgpu::Device, queue: wgpu::Queue) -> Arc<GpuContext> {
        SHADER_MODULES_CREATED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let assets = Arc::new(Mutex::new(GpuAssets::new(&device, &queue)));

        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("r274 scene shader"),
            source: wgpu::ShaderSource::Wgsl(SCENE_SHADER.into()),
        });

        let scene_brightness_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

        let assets_lock = assets.lock().unwrap();
        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("r274 scene layout"),
                bind_group_layouts: &[
                    Some(&assets_lock.model_bind_group_layout),
                    Some(&scene_brightness_layout),
                ],
                immediate_size: 0,
            });
        let pipeline_opaque = make_pipeline(&device, &scene_pipeline_layout, &scene_shader, true);
        let pipeline_translucent =
            make_pipeline(&device, &scene_pipeline_layout, &scene_shader, false);
        drop(assets_lock);

        let chrome_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("r274 chrome composite shader"),
            source: wgpu::ShaderSource::Wgsl(CHROME_SHADER.into()),
        });
        let chrome_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("r274 chrome composite layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let chrome_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("r274 chrome composite pipeline layout"),
                bind_group_layouts: &[Some(&chrome_layout)],
                immediate_size: 0,
            });
        let chrome_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("r274 chrome composite"),
            layout: Some(&chrome_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &chrome_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ChromeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
            },
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
            fragment: Some(wgpu::FragmentState {
                module: &chrome_shader,
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
            }),
            multiview_mask: None,
            cache: None,
        });

        Arc::new(GpuContext {
            device,
            queue,
            assets,
            scene_brightness_layout,
            pipeline_opaque,
            pipeline_translucent,
            chrome_layout,
            chrome_pipeline,
        })
    }
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
    Ok(GpuContext::new(device, queue))
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
    // is perspective-correct and stays in [0, 1] for every z >= 50.
    // RuneLite `vert.glsl` does `screenPos.z += float(bias) / 128.0` after
    // a reverse-z projection (`Mat4.projection`, `GL_GREATER`); we subtract
    // the same term under `LessEqual`. Face priority (0..11) is too small
    // to hide a 16-unit wall/booth overlap — that is handled in the mesh
    // builder, not by stretching this bias.
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
        // RuneLite `vert.glsl`: `fUv = tex / 256` with no shader clamp.
        // `frag.glsl` samples the mipmapped atlas, then `textureLod(..., 0)`
        // for alpha discard so mips do not punch holes.
        let uv = vec2<f32>(in.u, in.v) / 256.0;
        let t = textureSample(model_atlas, model_sampler, uv, i32(id));
        let t0 = textureSampleLevel(model_atlas, model_sampler, uv, i32(id), 0.0);
        if (t0.a < 1.0) { discard; }
        // The CPU's per-texel brightness: the interpolated 7-bit shade
        // (0..127, `Model.getColour`'s `127 - scalar`) selects one of the
        // four pre-baked texel blocks with bits 4-5 and then halves it for
        // shades >= 64 with bit 6 (`Pix3D.textureRaster`'s
        // `curU += (shadeA >> 3) & 0xc0000` and `shadeShift = shadeA >> 23`).
        let s = u32(in.hsl) & 0x7fu;
        let block = (s >> 4u) & 3u;
        let block_factor = array<f32, 4>(1.0, 0.875, 0.75, 0.625)[block];
        let factor = block_factor * select(1.0, 0.5, (s >> 6u) == 1u);
        return vec4<f32>(t.rgb * factor, in.alpha);
    }
    return vec4<f32>(in.color, in.alpha);
}
"#;

/// The 2D chrome composite shader: a full-frame passthrough that samples
/// the CPU-drawn `draw_area` uploaded as one RGBA8 texture (the RuneLite
/// pattern — the UI is never recorded as GPU quads). The alpha byte the
/// upload carries decides what covers the scene: opaque chrome, or the
/// scene's transparent hole (black pixels inside the scene window).
const CHROME_SHADER: &str = r#"
@group(0) @binding(0) var chrome_texture: texture_2d<f32>;
@group(0) @binding(1) var chrome_sampler: sampler;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    // draw_area pixels -> NDC (the same mapping the chrome quad shader
    // used: the full frame maps exactly onto the 765x503 viewport).
    out.position = vec4<f32>(
        in.pos.x * (2.0 / 765.0) - 1.0,
        1.0 - in.pos.y * (2.0 / 503.0),
        0.0,
        1.0,
    );
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(chrome_texture, chrome_sampler, in.uv);
}
"#;

/// One corner of the full-frame chrome quad (position in draw_area pixels,
/// UV in texture space). Six vertices, uploaded once.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ChromeVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
}

/// The wgpu scene backend (see the module docs). `begin`/`scene`/`chrome`
/// delegate to the inner `CpuBackend` (the 2D chrome stays CPU this slice);
/// `scene` rasterizes the 3D world into `scene_texture` and draws the
/// overlays into `area_game` as pixels; `composite_scene` blits that into
/// the persistent `draw_area`; `finish` uploads `draw_area` as a texture
/// and composites it over the scene.
pub struct GpuBackend {
    context: Arc<GpuContext>,
    scene_texture: wgpu::Texture,
    scene_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    /// The full-frame 765×503 composite (scene at (4, 4) + chrome), the
    /// texture `finish` returns.
    frame_texture: wgpu::Texture,
    frame_view: wgpu::TextureView,
    /// The shared model-texture-array bind group, bound each scene pass.
    model_bind_group: wgpu::BindGroup,
    /// The scene-shader brightness uniform (group 1): the current
    /// `Pix3D::colour_table` gamma, so `hslToRgb` matches the CPU table.
    /// Per-backend because `render_scene` writes the current brightness
    /// every frame (the layout + pipelines are the shared `GpuContext`).
    brightness_bind_group: wgpu::BindGroup,
    brightness_buf: wgpu::Buffer,
    /// Streaming vertex buffer (grows on demand; the mesh is re-uploaded
    /// every frame, the RuneLite plugin shape).
    vertex_buf: wgpu::Buffer,
    vertex_buf_capacity: usize,
    /// The 2D chrome/title half: the CPU fidelity path the frame stages
    /// delegate to. The chrome draws into the persistent `draw_area`.
    cpu: CpuBackend,
    /// The CPU `draw_area` uploaded as one RGBA8 texture each frame (the
    /// RuneLite canvas-upload composite), drawn over the frame with alpha
    /// blending. The texture + bind group are per-backend (each frame's
    /// upload); the composite shader/layout/pipeline are the shared
    /// `GpuContext`.
    chrome_texture: wgpu::Texture,
    chrome_bind_group: wgpu::BindGroup,
    /// The full-frame quad (six `ChromeVertex`es, uploaded once).
    chrome_vertex_buf: wgpu::Buffer,
    /// The overlay-coverage marks (one byte per scene-window pixel): the
    /// GPU overlay pass records which `area_game` pixels it wrote here,
    /// and `finish` keys the scene-window transparency off it.
    overlay_coverage: Vec<u8>,
    /// Last 3D texture is valid to composite. Title and `scene_state == 0`
    /// clear it. `scene_state == 1` keeps it (Java `area_game` without
    /// `cls` — freeze the last 3D frame under the loading splash).
    scene_ready: bool,
    last_kind: FrameKind,
}

impl GpuBackend {
    /// Initialise the process-wide device/queue + shared pipelines (once)
    /// and build the per-backend view textures + vertex buffer. Returns
    /// `Err` on any init failure (or a cached one); the renderer logs and
    /// falls back to `CpuBackend`.
    pub fn try_new() -> Result<GpuBackend, String> {
        if std::env::var("SKIP_GPU").ok().as_deref() == Some("1") {
            return Err("SKIP_GPU=1".into());
        }
        GPU_BACKEND_TRIED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let context = context().ok_or_else(|| "no GPU context".to_string())?;

        let scene_texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 scene"),
            size: wgpu::Extent3d {
                width: SCENE_W,
                height: SCENE_H,
                depth_or_array_layers: 1,
            },
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
            size: wgpu::Extent3d {
                width: SCENE_W,
                height: SCENE_H,
                depth_or_array_layers: 1,
            },
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
            size: wgpu::Extent3d {
                width: FRAME_W,
                height: FRAME_H,
                depth_or_array_layers: 1,
            },
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

        let brightness_buf = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 scene uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let brightness_bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("r274 scene uniforms group"),
                layout: &context.scene_brightness_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &brightness_buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(16),
                    }),
                }],
            });

        // The scene shader/layout/pipelines are the shared `GpuContext`
        // (task 6): a second backend reuses them instead of rebuilding.
        let model_bind_group = context.assets.lock().unwrap().model_bind_group.clone();

        let vertex_buf_capacity = 1 << 20; // 1 MiB of vertices to start
        let vertex_buf = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 scene vertices"),
            size: vertex_buf_capacity as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // The chrome composite (the RuneLite canvas-upload pattern): the
        // CPU `draw_area` uploads as one RGBA8 texture each frame and the
        // shared chrome pipeline draws it over the scene. The texture +
        // bind group are per-backend (each frame's upload); the
        // shader/layout/pipeline are the shared `GpuContext`.
        let chrome_texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 chrome upload"),
            size: wgpu::Extent3d {
                width: FRAME_W,
                height: FRAME_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let chrome_view = chrome_texture.create_view(&Default::default());
        let chrome_sampler = context.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("r274 chrome sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let chrome_bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("r274 chrome upload group"),
                layout: &context.chrome_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&chrome_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&chrome_sampler),
                    },
                ],
            });
        let chrome_vertices: [ChromeVertex; 6] = [
            ChromeVertex {
                x: 0.0,
                y: 0.0,
                u: 0.0,
                v: 0.0,
            },
            ChromeVertex {
                x: FRAME_W as f32,
                y: 0.0,
                u: 1.0,
                v: 0.0,
            },
            ChromeVertex {
                x: 0.0,
                y: FRAME_H as f32,
                u: 0.0,
                v: 1.0,
            },
            ChromeVertex {
                x: 0.0,
                y: FRAME_H as f32,
                u: 0.0,
                v: 1.0,
            },
            ChromeVertex {
                x: FRAME_W as f32,
                y: 0.0,
                u: 1.0,
                v: 0.0,
            },
            ChromeVertex {
                x: FRAME_W as f32,
                y: FRAME_H as f32,
                u: 1.0,
                v: 1.0,
            },
        ];
        let chrome_vertex_buf = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 chrome quad"),
            size: (chrome_vertices.len() * std::mem::size_of::<ChromeVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        context.queue.write_buffer(
            &chrome_vertex_buf,
            0,
            bytemuck::cast_slice(&chrome_vertices),
        );

        Ok(GpuBackend {
            context,
            scene_texture,
            scene_view,
            depth_view,
            frame_texture,
            frame_view,
            model_bind_group,
            brightness_bind_group,
            brightness_buf,
            vertex_buf,
            vertex_buf_capacity,
            cpu: CpuBackend,
            chrome_texture,
            chrome_bind_group,
            chrome_vertex_buf,
            overlay_coverage: vec![0; (SCENE_W * SCENE_H) as usize],
            scene_ready: false,
            last_kind: FrameKind::Title,
        })
    }

    /// wgpu device initialisations attempted in this process (the task-8
    /// headless counter; a cached failure still counts one attempt).
    pub fn tried() -> usize {
        GPU_BACKEND_TRIED.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many times the shared context built its shader modules /
    /// pipelines (task 6): two `try_new` must count one — the second
    /// backend reuses the process-wide `GpuContext`.
    pub fn shader_modules_created() -> usize {
        SHADER_MODULES_CREATED.load(std::sync::atomic::Ordering::Relaxed)
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

        let mut encoder =
            self.context
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
                pass.set_pipeline(&self.context.pipeline_opaque);
                pass.draw(0..opaque_len as u32, 0..1);
            }
            if opaque_len < vertices.len() {
                pass.set_pipeline(&self.context.pipeline_translucent);
                pass.draw(opaque_len as u32..vertices.len() as u32, 0..1);
            }
        }
        self.context.queue.submit([encoder.finish()]);
        self.scene_ready = true;
    }

    /// CPU overlay pixels into `area_game` plus coverage marks. Runs on
    /// the live scene and on a `scene_state==1` freeze so main-modals
    /// (`ship_journey`, `glidermap`) stay over the last 3D texture.
    fn draw_scene_overlays(&mut self, core: &mut Client, r: &mut Renderer) {
        if let Some(game) = r.area_game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            surface.cls();
        }
        self.overlay_coverage.fill(0);
        let _cov_guard = coverage_guard(&mut self.overlay_coverage, SCENE_W, SCENE_H);
        let mut game = r.area_game.take();
        if let Some(game) = game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            r.pix3d.set_clipping(game.width, game.height);
            crate::render::nav_debug::draw(&mut *core, r, &mut surface);
        }
        r.area_game = game;
        r.entity_overlays(core);
        r.coord_arrow(core);
        r.other_overlays(core);
        drop(_cov_guard);
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
        label: Some(if opaque {
            "r274 scene opaque"
        } else {
            "r274 scene translucent"
        }),
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
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
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
    /// Frame start: delegate to the CPU backend. The 2D chrome draws into
    /// the persistent `draw_area`, so the `redraw_frame` gating (the
    /// chrome-strip blits and the frozen-frame blit) works exactly like
    /// the CPU path.
    fn begin(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        self.last_kind = kind;
        if kind == FrameKind::Title {
            // Left torch column sits inside the scene-window rect. A stale
            // `scene_ready` from the last ingame frame would punch a 3D
            // hole through the flames. Title has no scene.
            self.scene_ready = false;
        }
        self.cpu.begin(core, r, kind);
    }

    /// `gameDrawMain`'s 3D pass on the GPU: the same entity/camera/prep
    /// steps as the CPU backend, then `prepare_scene` (draw-front marking,
    /// share-light) + the scene mesh rasterized into the wgpu scene
    /// texture. The overlays (chat bubbles, modal, the minimenu) draw into
    /// `area_game` as pixels (the CPU writes — no recorder); `composite_scene`
    /// blits them over the scene at (4, 4).
    fn scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        if kind != FrameKind::Game || core.scene_state != 2 {
            // Loading (`scene_state == 1`): do not rebuild the mesh and do
            // not drop the last scene texture — `finish` still copies it
            // (Java freeze + "Loading - please wait."). Title / no-scene
            // still punch a black 3D hole. Overlay IFs (`ship_journey`,
            // `glidermap`) still draw: GPU 3D is a separate texture, so a
            // freeze without this pass would show dock boats and no chart.
            if freeze_last_scene(kind, core.scene_state) {
                self.draw_scene_overlays(core, r);
            } else {
                self.scene_ready = false;
            }
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
                let target_y = get_av_h(
                    &core.groundh,
                    &core.mapl,
                    player.x,
                    player.z,
                    core.minusedlevel,
                ) - 50;
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
        // mesh (this also resolves the lazy model caches, appends loc
        // mouse picks and runs the ground click raycast), render it
        // into `scene_texture`.
        // Model textures upload into the shared array once (per texture
        // id), before the mesh is built so the ids resolve to layers.
        self.context
            .assets
            .lock()
            .unwrap()
            .ensure_model_textures(&r.pix3d);
        r.world.prepare_scene(
            &mut core.world,
            cache,
            loop_cycle,
            cam_x,
            cam_y,
            cam_z,
            level,
            cam_yaw,
            cam_pitch,
        );
        let mesh = r
            .world
            .build_scene_mesh(&mut core.world, cache, loop_cycle, &mut r.pix3d);
        self.render_scene(mesh);

        r.world.remove_sprites(&mut core.world);
        r.texture_run_anims(core, cycle);
        core.pick_count = r.pix3d.picked_count;
        core.pick_typecodes
            .copy_from_slice(&r.pix3d.picked_entity_typecode);
        self.draw_scene_overlays(core, r);

        core.cam_x = eye_x;
        core.cam_y = eye_y;
        core.cam_z = eye_z;
        core.cam_pitch = eye_pitch;
        core.cam_yaw = eye_yaw;
    }

    /// The composite seam: `area_game` overlay pixels blit into
    /// `draw_area` at (4, 4). The CPU backend only blits while
    /// `scene_state==2`; GPU also blits on a freeze so the modal coverage
    /// lands on the last 3D texture.
    fn composite_scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        if freeze_last_scene(kind, core.scene_state) {
            if let Some(game) = &r.area_game {
                game.blit_into(&mut r.draw_area, 4, 4);
            }
            return;
        }
        self.cpu.composite_scene(core, r, kind);
    }

    /// 2D chrome: delegate to the `CpuBackend` body, which draws the
    /// side/chat/icons/minimap/title into the persistent `draw_area`. The
    /// redraw flags gate the draws (no forced redraw — the persistent
    /// `draw_area` carries the previous frame's chrome until a redraw
    /// flag is set).
    fn chrome(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        self.cpu.chrome(core, r, kind);
    }

    /// The full-frame composite (the RuneLite canvas-upload pattern):
    /// clear the frame, copy the GPU scene at (4, 4) when it was
    /// rendered, then upload the CPU-drawn `draw_area` as one RGBA8
    /// texture and draw it over the frame with alpha blending. The upload
    /// is opaque chrome except inside the scene window, where overlay
    /// alpha is the coverage byte (255 = opaque chrome including black
    /// minimenu bars, 0 = 3D hole, 1..=254 = translucent nav paint; see
    /// [`draw_area_rgba`]). No readback.
    fn finish(&mut self, r: &mut Renderer) -> FrameOutput {
        let mut encoder =
            self.context
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
        if self.last_kind == FrameKind::Title {
            self.scene_ready = false;
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
                wgpu::Extent3d {
                    width: SCENE_W,
                    height: SCENE_H,
                    depth_or_array_layers: 1,
                },
            );
        }
        // The CPU chrome body drew the 2D UI into the persistent
        // `draw_area`; upload it as one RGBA8 texture. The rows are padded
        // to wgpu's 256-byte `COPY_BYTES_PER_ROW_ALIGNMENT`.
        let bytes_per_row = (FRAME_W * 4).div_ceil(256) * 256;
        let rgba = draw_area_rgba(
            &r.draw_area,
            &self.overlay_coverage,
            self.scene_ready,
            bytes_per_row,
        );
        self.context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.chrome_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(FRAME_H),
            },
            wgpu::Extent3d {
                width: FRAME_W,
                height: FRAME_H,
                depth_or_array_layers: 1,
            },
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("r274 chrome composite pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.frame_view,
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
            pass.set_pipeline(&self.context.chrome_pipeline);
            pass.set_bind_group(0, &self.chrome_bind_group, &[]);
            pass.set_vertex_buffer(0, self.chrome_vertex_buf.slice(..));
            pass.draw(0..6, 0..1);
        }
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

/// Keep compositing the last 3D texture (no mesh rebuild). Matches Java
/// `checkMinimap`: bind `area_game` without `cls` while `scene_state == 1`.
pub(crate) fn freeze_last_scene(kind: FrameKind, scene_state: i32) -> bool {
    kind == FrameKind::Game && scene_state == 1
}

/// Build the RGBA8 upload bytes from the CPU-drawn `draw_area` (opaque
/// `0x00RRGGBB` pixels), one row padded to `bytes_per_row` (wgpu's 256-byte
/// row alignment; the padding is zero-filled). When the scene was rendered,
/// the scene window — the fixed rect x∈[4,516), y∈[4,338) — takes overlay
/// alpha from the coverage byte: 255 is opaque chrome (minimenu black
/// bars stay black), 0 is the 3D hole, 1..=254 is translucent nav paint
/// over the scene. Outside the scene window the chrome is opaque; with no
/// scene (title screen) the whole frame is opaque.
fn draw_area_rgba(
    draw_area: &PixMap,
    coverage: &[u8],
    scene_ready: bool,
    bytes_per_row: u32,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((bytes_per_row * FRAME_H) as usize);
    for y in 0..FRAME_H {
        for x in 0..FRAME_W {
            let rgb = draw_area.pixels[(y * FRAME_W + x) as usize];
            let alpha = if scene_ready
                && (4..516).contains(&(x as i32))
                && (4..338).contains(&(y as i32))
            {
                coverage[((y - 4) * SCENE_W + (x - 4)) as usize]
            } else {
                255
            };
            bytes.push(((rgb >> 16) & 0xff) as u8);
            bytes.push(((rgb >> 8) & 0xff) as u8);
            bytes.push((rgb & 0xff) as u8);
            bytes.push(alpha);
        }
        bytes.resize(bytes.len() + (bytes_per_row - FRAME_W * 4) as usize, 0);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::{draw_area_rgba, freeze_last_scene, FRAME_H, FRAME_W, SCENE_H, SCENE_W};
    use crate::graphics::PixMap;
    use crate::render::backend::FrameKind;

    #[test]
    fn freeze_last_scene_only_while_the_game_is_loading() {
        assert!(
            freeze_last_scene(FrameKind::Game, 1),
            "scene_state==1 keeps the last 3D texture (Java no-cls)"
        );
        assert!(
            !freeze_last_scene(FrameKind::Game, 2),
            "a built scene is redrawn, not frozen"
        );
        assert!(
            !freeze_last_scene(FrameKind::Game, 0),
            "no scene: punch the 3D hole"
        );
        assert!(
            !freeze_last_scene(FrameKind::Title, 1),
            "title must not keep an ingame scene"
        );
    }

    #[test]
    fn draw_area_rgba_uses_coverage_as_scene_window_alpha() {
        let mut draw = PixMap::new(FRAME_W as i32, FRAME_H as i32);
        draw.pixels[(4 * FRAME_W + 4) as usize] = 0x00ff0000;
        draw.pixels[(4 * FRAME_W + 5) as usize] = 0x000000ff;
        let mut coverage = vec![0u8; (SCENE_W * SCENE_H) as usize];
        coverage[0] = 255;
        coverage[1] = 82;
        let bytes_per_row = (FRAME_W * 4).div_ceil(256) * 256;
        let bytes = draw_area_rgba(&draw, &coverage, true, bytes_per_row);
        let px = |x: u32, y: u32| {
            let o = (y * bytes_per_row + x * 4) as usize;
            (bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3])
        };
        assert_eq!(px(4, 4), (255, 0, 0, 255), "opaque overlay keeps A=255");
        assert_eq!(
            px(5, 4),
            (0, 0, 255, 82),
            "nav fill coverage is the overlay alpha"
        );
        assert_eq!(px(6, 4).3, 0, "uncovered scene pixel is the 3D hole");
        assert_eq!(px(0, 0).3, 255, "chrome outside the scene window is opaque");
    }

    #[test]
    fn draw_area_rgba_title_is_fully_opaque() {
        let mut draw = PixMap::new(FRAME_W as i32, FRAME_H as i32);
        draw.pixels[(4 * FRAME_W + 4) as usize] = 0x00ff8800;
        let coverage = vec![0u8; (SCENE_W * SCENE_H) as usize];
        let bytes_per_row = (FRAME_W * 4).div_ceil(256) * 256;
        let bytes = draw_area_rgba(&draw, &coverage, false, bytes_per_row);
        let o = (4 * bytes_per_row + 4 * 4) as usize;
        assert_eq!(
            bytes[o + 3],
            255,
            "title (no scene) must not punch a hole through the left torch column"
        );
    }
}
