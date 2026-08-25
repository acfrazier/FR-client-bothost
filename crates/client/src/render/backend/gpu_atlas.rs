//! The process-wide GPU assets (the "upload once" half of the GPU-chrome
//! campaign): the model-texture atlas the scene shader samples, and the
//! chrome sprite/font atlases the quad path draws from. All uploads happen
//! through the shared queue on first use; the only data movement is the
//! initial upload, never per frame.
//!
//! The model atlas is a fixed 8×8 grid of 128×128 cells (1024×1024 total)
//! — the client has at most 50 textures, so a fixed grid needs no growth;
//! the region for texture id `i` is the cell `(i % 8, i / 8)`, derived in
//! the shader with no per-vertex data beyond the id. Texels are baked from
//! the renderer's gamma-corrected texture palette (`tex_pal`, brightness
//! 0.8), 64×64 textures upscaled 2×2 exactly like the CPU's high-mem
//! `getTexels`. Alpha is set where the palette entry is non-zero, matching
//! the CPU's transparent-texel skip (palette index 0).
//!
//! The chrome atlases are growable shelf-packed `GpuAtlas`es (the sprite
//! atlas holds every chrome sprite and the lazily depacked item icons; the
//! font atlas holds the rasterised glyph masks). A sprite/glyph/map is
//! uploaded once per process, keyed by its data pointer — the caches below.

use crate::graphics::{Pix3DDraw, Pix32, Pix8, PixMap};
use std::collections::HashMap;
use std::sync::Mutex;

/// The grid: 50 textures max, 128×128 each, 8 per row.
const MODEL_ATLAS: u32 = 1024;
const MODEL_CELL: u32 = 128;

/// The initial chrome sprite atlas size (grows by doubling the height).
const SPRITE_ATLAS: (u32, u32) = (1024, 1024);
/// The initial font atlas size (the four fonts' 256 glyphs are small).
const FONT_ATLAS: (u32, u32) = (512, 512);

/// A rectangular region of an atlas texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A growable shelf-packed atlas texture. `alloc` places a `w×h` sprite on
/// the current shelf, starting a new shelf (or growing the texture — the
/// old content is copied at the same offsets, so existing region rects
/// stay valid) when it does not fit. Region rects stay valid across a
/// grow; only the bind group (the texture object) must be rebuilt, which
/// `GpuAssets` does when `generation` changes.
pub struct GpuAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    x: u32,
    y: u32,
    shelf_h: u32,
    generation: u64,
}

impl GpuAtlas {
    pub fn new(device: &wgpu::Device, label: &str, size: (u32, u32)) -> GpuAtlas {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // COPY_SRC: a grow copies the old content into the new texture.
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        GpuAtlas { texture, view, size, x: 0, y: 0, shelf_h: 0, generation: 0 }
    }

    /// The texture view the chrome bind group binds.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// How many times the atlas has grown (the bind group must be rebuilt
    /// when this changes — the texture object changed).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Allocate a `w×h` region, growing the texture (copying the old
    /// content at the same offsets) when the shelf cannot hold it.
    pub fn alloc(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
    ) -> Region {
        if self.x + w > self.size.0 {
            // Next shelf.
            self.x = 0;
            self.y += self.shelf_h;
            self.shelf_h = 0;
        }
        while self.y + h > self.size.1 {
            self.grow(device, queue);
        }
        let region = Region { x: self.x, y: self.y, w, h };
        self.x += w;
        self.shelf_h = self.shelf_h.max(h);
        region
    }

    /// Double the height: new texture, old content copied (an encoder
    /// submitted immediately, so any later `write_texture` on the queue
    /// lands after the copy).
    fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let (old_w, old_h) = self.size;
        let new_size = (old_w, old_h * 2);
        let new_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 chrome atlas (grown)"),
            size: wgpu::Extent3d {
                width: new_size.0,
                height: new_size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("r274 atlas grow"),
        });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &new_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d { width: old_w, height: old_h, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        self.texture = new_texture;
        self.view = self.texture.create_view(&Default::default());
        self.size = new_size;
        self.generation += 1;
    }
}

/// The shared, lazily-initialised GPU asset store (one per process). The
/// `GpuContext` owns the `Arc`; renderers clone it. `Mutex` because the
/// atlases grow on first use from any renderer's thread.
pub struct GpuAssets {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The model-texture atlas texture (scene shader group 0, binding 0).
    model_atlas: wgpu::Texture,
    /// The scene pipeline's bind group layout (texture + sampler).
    pub model_bind_group_layout: wgpu::BindGroupLayout,
    /// The scene pipeline's bind group, bound once per frame.
    pub model_bind_group: wgpu::BindGroup,
    /// Which texture ids are already in the atlas (upload once per
    /// process; a missing texture on this renderer just stays unset).
    model_regions: [bool; 50],
    /// The chrome sprite atlas (layer 1) and font atlas (layer 2), plus
    /// the chrome bind group (both textures + one sampler). The quad path
    /// uploads every sprite/glyph here on first use.
    sprite_atlas: GpuAtlas,
    font_atlas: GpuAtlas,
    chrome_sampler: wgpu::Sampler,
    chrome_bind_group_layout: wgpu::BindGroupLayout,
    chrome_bind_group: wgpu::BindGroup,
    /// The last-seen atlas generations (a grow must rebuild the chrome
    /// bind group).
    chrome_generations: (u64, u64),
    /// Sprite → atlas region, keyed by the sprite data pointer (Pix8 or
    /// Pix32). Uploaded once per process.
    sprite_regions: HashMap<usize, Region>,
    /// Static `PixMap` → atlas region (the chrome strips, the title JPEG
    /// regions), keyed by the map's pixels pointer. Uploaded once.
    map_regions: HashMap<usize, Region>,
    /// Glyph mask → font-atlas region, keyed by the mask data pointer.
    glyph_regions: HashMap<(usize, u32, u32), Region>,
    /// Per-frame staged maps (the rotated minimap, the title flames):
    /// a region allocated once per map, re-uploaded each blit.
    staged_regions: HashMap<usize, Region>,
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

        let sprite_atlas = GpuAtlas::new(device, "r274 sprite atlas", SPRITE_ATLAS);
        let font_atlas = GpuAtlas::new(device, "r274 font atlas", FONT_ATLAS);
        let chrome_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("r274 chrome sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let chrome_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("r274 chrome atlas layout"),
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let chrome_bind_group = build_chrome_bind_group(
            device,
            &sprite_atlas,
            &font_atlas,
            &chrome_sampler,
            &chrome_bind_group_layout,
        );
        let mut assets = GpuAssets {
            device: device.clone(),
            queue: queue.clone(),
            model_atlas,

            model_bind_group_layout,
            model_bind_group,
            model_regions: [false; 50],
            sprite_atlas,
            font_atlas,
            chrome_sampler,
            chrome_bind_group_layout,
            chrome_bind_group,
            chrome_generations: (0, 0),
            sprite_regions: HashMap::new(),
            map_regions: HashMap::new(),
            glyph_regions: HashMap::new(),
            staged_regions: HashMap::new(),
        };
        assets.refresh_chrome_bind_group();
        assets
    }

    /// The chrome quad pipeline's bind group (sprite atlas + font atlas +
    /// sampler), rebuilt when either atlas grows.
    pub fn chrome_bind_group(&self) -> &wgpu::BindGroup {
        &self.chrome_bind_group
    }

    /// The chrome bind group layout (the quad pipeline references it).
    pub fn chrome_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.chrome_bind_group_layout
    }

    /// Rebuild the chrome bind group after an atlas grow.
    fn rebuild_chrome_bind_group(&mut self) {
        self.chrome_bind_group = build_chrome_bind_group(
            &self.device,
            &self.sprite_atlas,
            &self.font_atlas,
            &self.chrome_sampler,
            &self.chrome_bind_group_layout,
        );
        self.chrome_generations = (self.sprite_atlas.generation(), self.font_atlas.generation());
    }

    /// Rebuild the chrome bind group when either atlas has grown since the
    /// last rebuild (called after every chrome upload).
    fn refresh_chrome_bind_group(&mut self) {
        if (self.sprite_atlas.generation(), self.font_atlas.generation())
            != self.chrome_generations
        {
            self.rebuild_chrome_bind_group();
        }
    }

    /// The sprite-atlas region for a `Pix8` sprite, uploading it (palette →
    /// RGBA, index 0 transparent) on first use.
    pub fn sprite_region_pix8(&mut self, sprite: &Pix8) -> Region {
        let key = sprite.data.as_ptr() as usize;
        if let Some(&region) = self.sprite_regions.get(&key) {
            return region;
        }
        let (w, h) = (sprite.wi as u32, sprite.hi as u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for i in 0..(w * h) as usize {
            let index = sprite.data.get(i).copied().unwrap_or(0);
            if index == 0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let rgb = sprite.bpal.get(index as u8 as usize).copied().unwrap_or(0);
                rgba.extend_from_slice(&[
                    ((rgb >> 16) & 0xff) as u8,
                    ((rgb >> 8) & 0xff) as u8,
                    (rgb & 0xff) as u8,
                    255,
                ]);
            }
        }
        self.upload_sprite(&key, w, h, &rgba)
    }

    /// The sprite-atlas region for a `Pix32` sprite (direct RGBA, 0
    /// transparent), uploading it on first use.
    pub fn sprite_region_pix32(&mut self, sprite: &Pix32) -> Region {
        let key = sprite.data.as_ptr() as usize;
        if let Some(&region) = self.sprite_regions.get(&key) {
            return region;
        }
        let (w, h) = (sprite.wi as u32, sprite.hi as u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for i in 0..(w * h) as usize {
            let rgb = sprite.data.get(i).copied().unwrap_or(0);
            rgba.extend_from_slice(&[
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
                if rgb != 0 { 255 } else { 0 },
            ]);
        }
        self.upload_sprite(&key, w, h, &rgba)
    }

    /// A map region for a static `PixMap` (the chrome strips, the title
    /// JPEG regions): upload once, blit as an opaque quad. The map's
    /// pixels are read verbatim (`0x00RRGGBB`).
    pub fn map_region(&mut self, map: &PixMap) -> Region {
        let key = map.pixels.as_ptr() as usize;
        if let Some(&region) = self.map_regions.get(&key) {
            return region;
        }
        let (w, h) = (map.width as u32, map.height as u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for &rgb in &map.pixels {
            rgba.extend_from_slice(&[
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
                255,
            ]);
        }
        self.upload_sprite(&key, w, h, &rgba)
    }

    /// A font-atlas region for a glyph mask (white RGB, alpha = mask),
    /// uploading the mask on first use (keyed by the mask pointer + size).
    pub fn glyph_region(&mut self, mask: &[i8], w: u32, h: u32) -> Region {
        let key = (mask.as_ptr() as usize, w, h);
        if let Some(&region) = self.glyph_regions.get(&key) {
            return region;
        }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for i in 0..(w * h) as usize {
            let on = mask.get(i).copied().unwrap_or(0) != 0;
            rgba.extend_from_slice(&[255, 255, 255, if on { 255 } else { 0 }]);
        }
        let region = self.font_atlas.alloc(&self.device, &self.queue, w, h);
        self.write_atlas(&self.font_atlas, region, &rgba);
        self.refresh_chrome_bind_group();
        self.glyph_regions.insert(key, region);
        region
    }

    /// The reused per-frame region for a staged map (the rotated minimap,
    /// the title flames): allocate once, re-upload every blit.
    pub fn staged_region(&mut self, tag: usize, w: u32, h: u32) -> Region {
        if let Some(&region) = self.staged_regions.get(&tag) {
            return region;
        }
        let region = self.sprite_atlas.alloc(&self.device, &self.queue, w, h);
        self.staged_regions.insert(tag, region);
        region
    }

    /// Upload the current pixels of a staged map into its region and
    /// return the region.
    pub fn staged_upload(&mut self, tag: usize, map: &PixMap) -> Region {
        let (w, h) = (map.width as u32, map.height as u32);
        let region = self.staged_region(tag, w, h);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for &rgb in &map.pixels {
            rgba.extend_from_slice(&[
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
                255,
            ]);
        }
        self.write_atlas(&self.sprite_atlas, region, &rgba);
        self.refresh_chrome_bind_group();
        region
    }

    /// The current sprite-atlas size (the quad flush resolves region UVs
    /// against it — the atlas may have grown).
    pub fn sprite_atlas_size(&self) -> (u32, u32) {
        self.sprite_atlas.size()
    }

    /// The current font-atlas size.
    pub fn font_atlas_size(&self) -> (u32, u32) {
        self.font_atlas.size()
    }

    /// Allocate a sprite-atlas region, upload the RGBA pixels, and cache
    /// the region under `key`.
    fn upload_sprite(&mut self, key: &usize, w: u32, h: u32, rgba: &[u8]) -> Region {
        let region = self.sprite_atlas.alloc(&self.device, &self.queue, w, h);
        self.write_atlas(&self.sprite_atlas, region, rgba);
        self.refresh_chrome_bind_group();
        self.sprite_regions.insert(*key, region);
        region
    }

    /// Write `rgba` into an atlas at `region` (the caller refreshes the
    /// chrome bind group after — the alloc may have grown the atlas).
    fn write_atlas(&self, atlas: &GpuAtlas, region: Region, rgba: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: atlas.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d { x: region.x, y: region.y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(region.w * 4),
                rows_per_image: Some(region.h),
            },
            wgpu::Extent3d {
                width: region.w,
                height: region.h,
                depth_or_array_layers: 1,
            },
        );
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

impl GpuAtlas {
    /// The atlas texture (for queue writes).
    fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// The current atlas size.
    fn size(&self) -> (u32, u32) {
        self.size
    }
}

/// Build the chrome quad pipeline's bind group (both atlases + sampler).
fn build_chrome_bind_group(
    device: &wgpu::Device,
    sprite_atlas: &GpuAtlas,
    font_atlas: &GpuAtlas,
    chrome_sampler: &wgpu::Sampler,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("r274 chrome atlas group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(sprite_atlas.view()),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(font_atlas.view()),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(chrome_sampler),
            },
        ],
    })
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
