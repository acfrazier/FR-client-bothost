//! The process-wide GPU assets (the "upload once" half of the GPU-chrome
//! campaign): the model-texture atlas the scene shader samples, and (the
//! later slices) the chrome sprite/font atlases the quad path draws from.
//! All uploads happen through the shared queue on first use; the only data
//! movement is the initial upload, never per frame.
//!
//! The model atlas is a fixed 8×8 grid of 128×128 cells (1024×1024 total)
//! — the client has at most 50 textures, so a fixed grid needs no growth;
//! the region for texture id `i` is the cell `(i % 8, i / 8)`, derived in
//! the shader with no per-vertex data beyond the id. Texels are baked from
//! the renderer's gamma-corrected texture palette (`tex_pal`, brightness
//! 0.8), 64×64 textures upscaled 2×2 exactly like the CPU's high-mem
//! `getTexels`. Alpha is set where the palette entry is non-zero, matching
//! the CPU's transparent-texel skip (palette index 0).

use crate::graphics::Pix3DDraw;
use std::sync::Mutex;

/// The grid: 50 textures max, 128×128 each, 8 per row.
const MODEL_ATLAS: u32 = 1024;
const MODEL_CELL: u32 = 128;

/// The shared, lazily-initialised GPU asset store (one per process). The
/// `GpuContext` owns the `Arc`; renderers clone it. `Mutex` because the
/// atlas grows on first use from any renderer's thread.
pub struct GpuAssets {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The model-texture atlas texture (scene shader group 0, binding 0).
    model_atlas: wgpu::Texture,
    model_view: wgpu::TextureView,
    model_sampler: wgpu::Sampler,
    /// The scene pipeline's bind group layout (texture + sampler).
    pub model_bind_group_layout: wgpu::BindGroupLayout,
    /// The scene pipeline's bind group, bound once per frame.
    pub model_bind_group: wgpu::BindGroup,
    /// Which texture ids are already in the atlas (upload once per
    /// process; a missing texture on this renderer just stays unset).
    model_regions: [bool; 50],
}

impl GpuAssets {
    /// Build the asset store on the shared device/queue.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> GpuAssets {
        let model_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 model texture atlas"),
            size: wgpu::Extent3d {
                width: MODEL_ATLAS,
                height: MODEL_ATLAS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let model_view = model_atlas.create_view(&Default::default());
        let model_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("r274 model sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let model_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("r274 model atlas layout"),
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
        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("r274 model atlas group"),
            layout: &model_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&model_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&model_sampler),
                },
            ],
        });
        GpuAssets {
            device: device.clone(),
            queue: queue.clone(),
            model_atlas,
            model_view,
            model_sampler,
            model_bind_group_layout,
            model_bind_group,
            model_regions: [false; 50],
        }
    }

    /// Upload any of the renderer's model textures not yet in the atlas
    /// (once per process, keyed by texture id). A renderer without the
    /// `textures` jag (or a failed depack) leaves its id unset; the scene
    /// mesh then samples nothing for those faces.
    pub fn ensure_model_textures(&mut self, pix: &Pix3DDraw) {
        for id in 0..50 {
            if self.model_regions[id] {
                continue;
            }
            let (Some(texture), Some(palette)) = (&pix.textures[id], &pix.tex_pal[id]) else {
                continue;
            };
            let mut rgba = vec![0u8; (MODEL_CELL * MODEL_CELL * 4) as usize];
            if texture.wi == 128 {
                for y in 0..MODEL_CELL as usize {
                    for x in 0..MODEL_CELL as usize {
                        let data = texture
                            .data
                            .get((x + y * MODEL_CELL as usize) as usize)
                            .copied()
                            .unwrap_or(0);
                        let rgb = palette_lookup(palette, data);
                        let i = (y * MODEL_CELL as usize + x) * 4;
                        rgba[i] = ((rgb >> 16) & 0xff) as u8;
                        rgba[i + 1] = ((rgb >> 8) & 0xff) as u8;
                        rgba[i + 2] = (rgb & 0xff) as u8;
                        rgba[i + 3] = if rgb != 0 { 255 } else { 0 };
                    }
                }
            } else {
                // 64×64 → 2×2 upscale, the CPU high-mem `getTexels` repeat.
                for y in 0..MODEL_CELL as usize {
                    for x in 0..MODEL_CELL as usize {
                        let data = texture
                            .data
                            .get((x >> 1) + ((y >> 1) << 6))
                            .copied()
                            .unwrap_or(0);
                        let rgb = palette_lookup(palette, data);
                        let i = (y * MODEL_CELL as usize + x) * 4;
                        rgba[i] = ((rgb >> 16) & 0xff) as u8;
                        rgba[i + 1] = ((rgb >> 8) & 0xff) as u8;
                        rgba[i + 2] = (rgb & 0xff) as u8;
                        rgba[i + 3] = if rgb != 0 { 255 } else { 0 };
                    }
                }
            }
            let cell_x = ((id as u32) % 8) * MODEL_CELL;
            let cell_y = ((id as u32) / 8) * MODEL_CELL;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.model_atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: cell_x, y: cell_y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(MODEL_CELL * 4),
                    rows_per_image: Some(MODEL_CELL),
                },
                wgpu::Extent3d {
                    width: MODEL_CELL,
                    height: MODEL_CELL,
                    depth_or_array_layers: 1,
                },
            );
            self.model_regions[id] = true;
        }
    }
}

/// TS typed-array palette lookup (the `Pix3D` private helper): an index
/// outside the palette (negative `i8`, or past the end) is `undefined` → 0.
fn palette_lookup(palette: &[i32], data: i8) -> i32 {
    let idx = data as i32;
    if idx < 0 {
        0
    } else {
        palette.get(idx as usize).copied().unwrap_or(0)
    }
}

/// The assets behind the shared `GpuContext` (a `Mutex` so any renderer
/// thread can grow them on first use).
pub type SharedAssets = Mutex<GpuAssets>;
