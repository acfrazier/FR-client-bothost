//! The wgpu backend (task 7): the 3D scene is rasterized on the GPU into
//! a texture, then composited into `draw_area` at the (4, 4) blit point
//! under the CPU 2D chrome. The scene mesh (`RenderWorld::build_scene_mesh`)
//! transforms the marked tiles' ground, walls, decor, objects and
//! sprites to camera space with the exact CPU fixed-point math and shades
//! them from the same colour table; the vertex shader does the perspective
//! divide (near-clipping at z = 50 for free) and the depth buffer replaces
//! the CPU painter's priority merge. The chrome, the overlays and the
//! title screen stay `CpuBackend` this slice.
//!
//! One device/queue per process: the first `GpuBackend::try_new`
//! initialises a shared `GpuContext` (`CONTEXT`, a `OnceLock`) and every
//! backend clones it. A failed init is cached and logged once; the
//! renderer falls back to `CpuBackend` — a missing GPU never takes the bot
//! down. The `R274_TEST_FORCE_NO_GPU` env var forces an init failure for
//! the selection test.
//!
//! Known divergences from the CPU path (documented in `render/world.rs`):
//! textured faces render flat-shaded from their per-vertex shade (no
//! texture sampling), mouse picks are AABB-only, and the occluder tests
//! are skipped (the depth buffer occludes).

use std::sync::{mpsc, Arc, OnceLock};
use std::time::Duration;

use crate::client::client::Client;
use crate::graphics::Pix3D;
use crate::graphics::PixMap;
use crate::render::backend::{BackendKind, CpuBackend, FrameKind, FrameOutput, RenderBackend};
use crate::render::world::{GpuVertex, SceneMesh};
use crate::render::Renderer;
use crate::render::draw::get_av_h;

/// The wgpu scene texture size: `area_game` is 512×334, blitted at (4, 4).
const SCENE_W: u32 = 512;
const SCENE_H: u32 = 334;

/// The shared wgpu device/queue home (one per process). `Result` so a
/// failed init is cached: the fallback log fires once and later renderers
/// do not retry (and cannot take the bot down).
static CONTEXT: OnceLock<Result<Arc<GpuContext>, String>> = OnceLock::new();

/// Whether the init failure has been logged (once per process).
static FAILURE_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
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
        apply_limit_buckets: false,
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
    Ok(Arc::new(GpuContext { device, queue }))
}

/// The scene pipeline: a passthrough projection (camera-space vertices,
/// perspective divide in the shader) plus the shaded vertex colour.
const SCENE_SHADER: &str = r#"
const NEAR: f32 = 50.0;
const SCALE_X: f32 = 2.0;
const SCALE_Y: f32 = -512.0 / 167.0;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let z = in.pos.z;
    // The projection mirrors the CPU `origin + (x << 9) / z` (origin = the
    // 512×334 area_game centre). clip.z is set so the interpolated depth
    // is perspective-correct and stays in [0, 1] for every z >= 50 (the
    // CPU's near guard); the GPU still clips triangles at the near plane
    // (`clip.z >= -w` ⟺ `z >= NEAR / 2`).
    out.position = vec4<f32>(in.pos.x * SCALE_X, in.pos.y * SCALE_Y, z - NEAR, z);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

/// The wgpu scene backend (see the module docs). `begin`/`chrome` delegate
/// to the inner `CpuBackend` (the 2D chrome stays CPU this slice); `scene`
/// rasterizes the 3D world into `scene_texture` and reads it back into
/// `scene_pix`; `composite_scene` merges the `area_game` overlay content
/// over the read-back and blits the result at (4, 4).
pub struct GpuBackend {
    context: Arc<GpuContext>,
    scene_texture: wgpu::Texture,
    scene_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    pipeline_opaque: wgpu::RenderPipeline,
    pipeline_translucent: wgpu::RenderPipeline,
    /// Streaming vertex buffer (grows on demand; the mesh is re-uploaded
    /// every frame, the RuneLite plugin shape).
    vertex_buf: wgpu::Buffer,
    vertex_buf_capacity: usize,
    readback_buf: wgpu::Buffer,
    /// The CPU copy of the read-back scene, composited into `draw_area`.
    scene_pix: PixMap,
    /// The 2D chrome/title half.
    cpu: CpuBackend,
    /// A scene was rendered and read back this frame (a title frame, or an
    /// in-game frame with no built scene, leaves `composite_scene` a no-op
    /// like the CPU path's frozen-frame blit).
    scene_ready: bool,
}

impl GpuBackend {
    /// Initialise the process-wide device/queue and build the scene
    /// pipeline. Returns `Err` on any init failure (or a cached one); the
    /// renderer logs and falls back to `CpuBackend`.
    pub fn try_new() -> Result<GpuBackend, String> {
        let context = context().ok_or_else(|| "no GPU context".to_string())?;

        let scene_texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 scene"),
            size: wgpu::Extent3d { width: SCENE_W, height: SCENE_H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Not sRGB: the colour-table values are already display-encoded
            // and are read straight back into the CPU `draw_area`.
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

        let shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("r274 scene shader"),
                source: wgpu::ShaderSource::Wgsl(SCENE_SHADER.into()),
            });

        let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("r274 scene layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline_opaque = make_pipeline(&context.device, &layout, &shader, true);
        let pipeline_translucent = make_pipeline(&context.device, &layout, &shader, false);

        let vertex_buf_capacity = 1 << 20; // 1 MiB of vertices to start
        let vertex_buf = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 scene vertices"),
            size: vertex_buf_capacity as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // The read-back: 512×334 RGBA, `bytes_per_row` 2048 (a multiple of
        // the 256-byte COPY_BYTES_PER_ROW_ALIGNMENT).
        let readback_buf = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("r274 scene readback"),
            size: (SCENE_W * SCENE_H * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok(GpuBackend {
            context,
            scene_texture,
            scene_view,
            depth_view,
            pipeline_opaque,
            pipeline_translucent,
            vertex_buf,
            vertex_buf_capacity,
            readback_buf,
            scene_pix: PixMap::new(SCENE_W as i32, SCENE_H as i32),
            cpu: CpuBackend,
            scene_ready: false,
        })
    }

    /// Upload the mesh, rasterize it into `scene_texture` (opaque faces
    /// first with depth writes, then the translucent faces alpha-blended)
    /// and read the result back into `scene_pix` synchronously (the
    /// composite step needs the pixels on the CPU).
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
            if opaque_len > 0 {
                pass.set_pipeline(&self.pipeline_opaque);
                pass.draw(0..opaque_len as u32, 0..1);
            }
            if opaque_len < vertices.len() {
                pass.set_pipeline(&self.pipeline_translucent);
                pass.draw(opaque_len as u32..vertices.len() as u32, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.scene_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SCENE_W * 4),
                    rows_per_image: Some(SCENE_H),
                },
            },
            wgpu::Extent3d { width: SCENE_W, height: SCENE_H, depth_or_array_layers: 1 },
        );
        self.context.queue.submit([encoder.finish()]);

        // Synchronous read-back: poll the device until the map completes
        // and copy the RGBA bytes into `scene_pix` (0x00RRGGBB pixels).
        let slice = self.readback_buf.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        loop {
            let _ = self.context.device.poll(wgpu::PollType::wait_indefinitely());
            if rx.recv_timeout(Duration::from_millis(1)).is_ok() {
                break;
            }
        }
        if let Ok(data) = slice.get_mapped_range() {
            for (dst, src) in self.scene_pix.pixels.iter_mut().zip(data.chunks_exact(4)) {
                *dst = ((src[0] as i32) << 16) | ((src[1] as i32) << 8) | (src[2] as i32);
            }
            drop(data);
            self.readback_buf.unmap();
            self.scene_ready = true;
        } else {
            self.scene_ready = false;
        }
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
        buffers: &[Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4],
        })],
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
    /// The chrome prep (prepare_game/prepare_title, the redraw-frame
    /// compositing) is CPU work — the `CpuBackend` body, unchanged.
    fn begin(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        self.cpu.begin(core, r, kind);
    }

    /// `gameDrawMain`'s 3D pass on the GPU: the same entity/camera/prep
    /// steps as the CPU backend, then `prepare_scene` (draw-front marking,
    /// share-light) + the scene mesh rasterized into the wgpu texture and
    /// read back into `scene_pix`. The overlays (chat bubbles, modal, the
    /// minimenu) still draw into `area_game` as on the CPU path; the
    /// composite merges them over the read-back scene.
    fn scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
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
        // into `scene_texture` and read it back into `scene_pix`.
        r.world
            .prepare_scene(&mut core.world, cache, loop_cycle, cam_x, cam_y, cam_z, level, cam_yaw, cam_pitch);
        let mesh = r
            .world
            .build_scene_mesh(&mut core.world, cache, loop_cycle, &mut r.pix3d);
        self.render_scene(mesh);

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

    /// The composite seam: merge the `area_game` overlay content (chat
    /// bubbles, the modal, the minimenu — drawn over a cleared `area_game`
    /// on this path, so non-black is the coverage key) over the read-back
    /// scene pixels and blit the result at the (4, 4) point, under the
    /// 2D chrome.
    fn composite_scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        if kind == FrameKind::Game && core.scene_state == 2 && self.scene_ready {
            if let Some(game) = &r.area_game {
                for (dst, src) in self.scene_pix.pixels.iter_mut().zip(game.pixels.iter()) {
                    if *src != 0 {
                        *dst = *src;
                    }
                }
            }
            self.scene_pix.blit_into(&mut r.draw_area, 4, 4);
        }
    }

    /// 2D chrome: the `CpuBackend` body, unchanged (the chrome stays CPU
    /// this slice).
    fn chrome(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        self.cpu.chrome(core, r, kind);
    }

    /// This slice's GPU path composites into the CPU `draw_area`, so the
    /// owned `PixMap` frame is returned exactly like the CPU path (the
    /// window present path stays untouched). `FrameOutput::Texture` remains
    /// the host-owned texture-handoff arm for the slice-2 present work.
    fn finish(&mut self, r: &mut Renderer) -> FrameOutput {
        FrameOutput::PixMap(r.draw_area.clone())
    }

    fn kind(&self) -> BackendKind {
        BackendKind::Gpu
    }
}
