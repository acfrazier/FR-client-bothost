//! The process-wide GPU assets (the "upload once" half of the GPU-chrome
//! campaign): the model-texture array the scene shader samples, and the
//! chrome sprite/font atlases the quad path draws from. All uploads happen
//! through the shared queue on first use; the only data movement is the
//! initial upload, never per frame.
//!
//! The model atlas is a `texture_2d_array` with one 128×128 layer per
//! texture id (50 layers — the client has at most 50 textures); the shader
//! samples layer `id` directly, clamped to the layer count, with no cell
//! maths. Texels are baked from the renderer's gamma-corrected texture
//! palette (`tex_pal`, brightness 0.8), 64×64 textures upscaled 2×2 exactly
//! like the CPU's high-mem `getTexels`. Alpha is set where the palette
//! entry is non-zero, matching the CPU's transparent-texel skip (palette
//! index 0).
//!
//! The chrome sprite atlas is a stack of shelf-packed `GpuAtlas` layers
//! (`LayerAtlas`). Each layer grows by doubling its height up to the
//! device's `max_texture_dimension_2d`; once every layer is at the cap a
//! new layer is appended (up to `MAX_SPRITE_LAYERS`), so the atlas can
//! never ask the device for an over-limit texture. The font atlas is a
//! single capped layer (glyphs are small). A sprite/glyph/map is uploaded
//! once per process, keyed by its data pointer — the caches below. The
//! caches are bounded LRUs: when a cache is full, the least-recently-used
//! region is evicted and returned to a free list for exact-size reuse, so
//! reallocated sources (a re-created `PixMap`, a dropped sprite, a new
//! renderer's fonts) stop growing the atlas. Reuse is gated by a frame
//! epoch: a region evicted during a frame is only reused from the next
//! frame on, so no recorded quad can sample an overwritten region.

use crate::graphics::{Pix32, Pix3DDraw, Pix8, PixMap};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

/// The texture array: 50 textures max, one 128×128 layer per id.
const MODEL_LAYERS: u32 = 50;
const MODEL_CELL: u32 = 128;
/// RuneLite `TextureManager`: `glTexStorage3D(..., 8, GL_RGBA8, 128, 128, …)`.
const MODEL_MIP_LEVELS: u32 = 8;

/// Average a square RGBA8 image down 2× (box filter). Used to fill the
/// model-atlas mip chain after lod 0, matching `glGenerateMipmap`.
fn downsample_rgba(src: &[u8], width: u32) -> Vec<u8> {
    let w = width as usize;
    let hw = w / 2;
    let mut dst = vec![0u8; hw * hw * 4];
    for y in 0..hw {
        for x in 0..hw {
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut a = 0u32;
            for oy in 0..2 {
                for ox in 0..2 {
                    let i = ((y * 2 + oy) * w + (x * 2 + ox)) * 4;
                    r += src[i] as u32;
                    g += src[i + 1] as u32;
                    b += src[i + 2] as u32;
                    a += src[i + 3] as u32;
                }
            }
            let o = (y * hw + x) * 4;
            dst[o] = (r / 4) as u8;
            dst[o + 1] = (g / 4) as u8;
            dst[o + 2] = (b / 4) as u8;
            dst[o + 3] = (a / 4) as u8;
        }
    }
    dst
}

/// Lod 0 plus the 2× box-filter chain down to 1×1 (8 levels from 128).
fn rgba_mip_chain(lod0: &[u8], mut width: u32) -> Vec<(u32, Vec<u8>)> {
    let mut chain = vec![(width, lod0.to_vec())];
    let mut src = lod0.to_vec();
    while width > 1 {
        src = downsample_rgba(&src, width);
        width /= 2;
        chain.push((width, src.clone()));
    }
    chain
}

/// The initial chrome sprite atlas size (grows by doubling the height).
const SPRITE_ATLAS: (u32, u32) = (1024, 1024);
/// The initial font atlas size (the four fonts' 256 glyphs are small).
const FONT_ATLAS: (u32, u32) = (512, 512);

/// The maximum number of sprite atlas layers the chrome bind group binds.
/// Each layer can be `max_texture_dimension_2d` tall, so the sprites that
/// matter for one frame (bounded by the caches below) never reach this.
pub const MAX_SPRITE_LAYERS: usize = 8;

/// Cache bounds (the live set in a frame is far smaller; the bounds are a
/// safety cap so stale/reallocated sources stop growing the atlases).
const SPRITE_REGION_BOUND: usize = 4096;
const MAP_REGION_BOUND: usize = 128;
const GLYPH_REGION_BOUND: usize = 2048;
const STAGED_REGION_BOUND: usize = 16;

/// A rectangular region of an atlas layer. `layer` selects which sprite
/// atlas layer the region lives in (0 for the font atlas).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub layer: u32,
}

/// A growable shelf-packed atlas texture. `alloc` places a `w×h` sprite on
/// the current shelf, starting a new shelf (or growing the texture — the
/// old content is copied at the same offsets, so existing region rects
/// stay valid) when it does not fit. Growth stops at `max` (the device's
/// `max_texture_dimension_2d`); `alloc` returns `None` once the layer
/// cannot hold the region, and the multi-layer `LayerAtlas` spills to a
/// fresh layer. Region rects stay valid across a grow; only the bind
/// group (the texture object) must be rebuilt, which `GpuAssets` does when
/// `generation` changes.
pub struct GpuAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    max: u32,
    layer: u32,
    x: u32,
    y: u32,
    shelf_h: u32,
    generation: u64,
}

impl GpuAtlas {
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        size: (u32, u32),
        max: u32,
        layer: u32,
    ) -> GpuAtlas {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
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
        GpuAtlas {
            texture,
            view,
            size,
            max,
            layer,
            x: 0,
            y: 0,
            shelf_h: 0,
            generation: 0,
        }
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

    /// The layer index this atlas is bound as (the sprite layer).
    pub fn layer_index(&self) -> u32 {
        self.layer
    }

    /// Allocate a `w×h` region, growing the texture (copying the old
    /// content at the same offsets) when the shelf cannot hold it. Returns
    /// `None` once the layer is at its size cap and cannot fit the region.
    pub fn alloc(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
    ) -> Option<Region> {
        if w > self.size.0 || h > self.max {
            return None;
        }
        if self.x + w > self.size.0 {
            // Next shelf.
            self.x = 0;
            self.y += self.shelf_h;
            self.shelf_h = 0;
        }
        while self.y + h > self.size.1 {
            if !self.grow(device, queue) {
                return None;
            }
        }
        let region = Region {
            x: self.x,
            y: self.y,
            w,
            h,
            layer: self.layer,
        };
        self.x += w;
        self.shelf_h = self.shelf_h.max(h);
        Some(region)
    }

    /// Double the height (capped at `max`): new texture, old content
    /// copied (an encoder submitted immediately, so any later
    /// `write_texture` on the queue lands after the copy). `false` when
    /// the layer is already at the cap.
    fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> bool {
        let (old_w, old_h) = self.size;
        let new_h = (old_h * 2).min(self.max);
        if new_h == old_h {
            return false;
        }
        let new_size = (old_w, new_h);
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
            wgpu::Extent3d {
                width: old_w,
                height: old_h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        self.texture = new_texture;
        self.view = self.texture.create_view(&Default::default());
        self.size = new_size;
        self.generation += 1;
        true
    }
}

/// A reusable evicted region: its rect (which carries its sprite layer)
/// and the epoch it was freed at (reuse only from a strictly later frame).
struct FreeRegion {
    region: Region,
    freed_epoch: u64,
}

/// The multi-layer sprite atlas. `alloc` reuses an exact-size evicted
/// region (from a previous epoch) before growing/spilling; a layer grows
/// to the device cap, then a fresh layer is appended.
pub struct LayerAtlas {
    layers: Vec<GpuAtlas>,
    free: Vec<FreeRegion>,
    max: u32,
}

impl LayerAtlas {
    pub fn new(device: &wgpu::Device, label: &str, size: (u32, u32), max: u32) -> LayerAtlas {
        let layers = vec![GpuAtlas::new(device, label, size, max, 0)];
        LayerAtlas {
            layers,
            free: Vec::new(),
            max,
        }
    }

    /// Allocate a `w×h` region on some layer: exact-size evicted regions
    /// (freed before `epoch`) are reused first, then the existing layers
    /// in order, then a fresh layer (up to `MAX_SPRITE_LAYERS`).
    /// `None` when every layer is full and no region can be reused.
    pub fn alloc(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
        epoch: u64,
    ) -> Option<Region> {
        if let Some(i) = self
            .free
            .iter()
            .position(|f| f.freed_epoch < epoch && f.region.w == w && f.region.h == h)
        {
            let free = self.free.remove(i);
            return Some(free.region);
        }
        for i in 0..self.layers.len() {
            if let Some(region) = self.layers[i].alloc(device, queue, w, h) {
                return Some(region);
            }
        }
        if self.layers.len() < MAX_SPRITE_LAYERS {
            let layer = self.layers.len() as u32;
            // A fresh layer starts at the atlas width, min(1024, cap) tall.
            let initial = (self.layers[0].size.0, self.max.min(1024));
            let mut fresh =
                GpuAtlas::new(device, "r274 sprite atlas layer", initial, self.max, layer);
            if let Some(region) = fresh.alloc(device, queue, w, h) {
                self.layers.push(fresh);
                return Some(region);
            }
        }
        None
    }

    /// Return an evicted region to the free list (reusable from the next
    /// epoch on).
    pub fn free_region(&mut self, region: Region, epoch: u64) {
        self.free.push(FreeRegion {
            region,
            freed_epoch: epoch,
        });
    }

    /// The number of layers (a grow/appends change the bind group).
    pub fn generation(&self) -> u64 {
        self.layers.iter().map(|l| l.generation()).sum::<u64>() * 7919 + self.layers.len() as u64
    }

    /// The current size of one layer (the quad flush resolves region UVs
    /// against it — layers may have grown).
    pub fn layer_size(&self, layer: u32) -> (u32, u32) {
        self.layers
            .get(layer as usize)
            .map(|l| l.size)
            .unwrap_or((1, 1))
    }

    /// The texture views, one per layer, padded to `MAX_SPRITE_LAYERS`
    /// with the last real layer's view (unused bindings sample nothing —
    /// no region references a non-existent layer).
    pub fn views_padded(&self) -> Vec<&wgpu::TextureView> {
        let last = self.layers.last().map(|l| l.view()).unwrap();
        (0..MAX_SPRITE_LAYERS)
            .map(|i| self.layers.get(i).map(|l| l.view()).unwrap_or(last))
            .collect()
    }

    /// The layer texture for a region (queue writes).
    pub fn layer_texture(&self, layer: u32) -> &wgpu::Texture {
        self.layers
            .get(layer as usize)
            .map(|l| &l.texture)
            .expect("regions reference an existing layer")
    }

    /// The current size of every layer (the quad flush).
    pub fn layer_sizes(&self) -> Vec<(u32, u32)> {
        self.layers.iter().map(|l| l.size).collect()
    }

    /// The layer count (tests).
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// The free-list length (tests).
    pub fn free_len(&self) -> usize {
        self.free.len()
    }
}

/// A bounded LRU of `key → region` with a per-access epoch. Evicting the
/// least-recently-used entry returns its region so the caller can reclaim
/// it. `get` bumps the entry's recency; `insert` over the bound evicts.
struct RegionCache<K: Copy + Eq + std::hash::Hash> {
    bound: usize,
    map: HashMap<K, CachedRegion>,
    order: VecDeque<K>,
}

struct CachedRegion {
    region: Region,
    last_epoch: u64,
}

impl<K: Copy + Eq + std::hash::Hash> RegionCache<K> {
    fn new(bound: usize) -> RegionCache<K> {
        RegionCache {
            bound,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: K, epoch: u64) -> Option<Region> {
        let entry = self.map.get_mut(&key)?;
        entry.last_epoch = epoch;
        if let Some(pos) = self.order.iter().position(|&k| k == key) {
            let k = self.order.remove(pos).unwrap();
            self.order.push_back(k);
        }
        Some(entry.region)
    }

    /// Insert `key → region`, evicting the least-recently-used entry when
    /// over the bound; the evicted region (with the current epoch) is
    /// returned for reclamation.
    fn insert(&mut self, key: K, region: Region, epoch: u64) -> Option<(Region, u64)> {
        let evicted = if self.map.contains_key(&key) || self.map.len() < self.bound {
            None
        } else {
            let victim = self
                .order
                .pop_front()
                .and_then(|k| self.map.remove(&k).map(|v| (k, v.region)))
                .or_else(|| {
                    // The order deque should mirror the map; fall back to a
                    // scan so a desync never panics the frame.
                    self.map
                        .iter()
                        .min_by_key(|(_, v)| v.last_epoch)
                        .map(|(k, v)| (*k, v.region))
                        .and_then(|(k, r)| self.map.remove(&k).map(|_| (k, r)))
                });
            victim.map(|(_, region)| (region, epoch))
        };
        self.map.insert(
            key,
            CachedRegion {
                region,
                last_epoch: epoch,
            },
        );
        self.order.push_back(key);
        evicted
    }
}

/// The shared, lazily-initialised GPU asset store (one per process). The
/// `GpuContext` owns the `Arc`; renderers clone it. `Mutex` because the
/// atlases grow on first use from any renderer's thread.
pub struct GpuAssets {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The model-texture array texture (scene shader group 0, binding 0).
    model_atlas: wgpu::Texture,
    /// The scene pipeline's bind group layout (texture + sampler).
    pub model_bind_group_layout: wgpu::BindGroupLayout,
    /// The scene pipeline's bind group, bound once per frame.
    pub model_bind_group: wgpu::BindGroup,
    /// Which texture ids are already in the array (upload once per
    /// process; a missing texture on this renderer just stays unset).
    model_regions: [bool; 50],
    /// The chrome sprite atlas layers (bindings 0..7) and the font atlas
    /// (binding 8), plus the chrome bind group (sampler binding 9). The
    /// quad path uploads every sprite/glyph here on first use.
    sprite_atlas: LayerAtlas,
    font_atlas: GpuAtlas,
    /// Evicted font regions (reused from the next epoch on).
    font_free: Vec<(Region, u64)>,
    chrome_sampler: wgpu::Sampler,
    chrome_bind_group_layout: wgpu::BindGroupLayout,
    chrome_bind_group: wgpu::BindGroup,
    /// The last-seen atlas generations (a grow must rebuild the chrome
    /// bind group).
    chrome_generations: (u64, u64),
    /// The current frame epoch (bumped per renderer frame; cache evictions
    /// tag regions with it so reuse waits for the next frame).
    epoch: u64,
    /// Sprite → atlas region, keyed by the sprite data pointer (Pix8 or
    /// Pix32). Bounded LRU: stale pointers (dropped sprites, reallocated
    /// sources) stop growing the atlas.
    sprite_regions: RegionCache<usize>,
    /// Static `PixMap` → atlas region (the chrome strips, the title JPEG
    /// regions), keyed by the map's pixels pointer.
    map_regions: RegionCache<usize>,
    /// Glyph mask → font-atlas region, keyed by the mask data pointer.
    glyph_regions: RegionCache<(usize, u32, u32)>,
    /// Per-frame staged maps (the rotated minimap, the title flames):
    /// a region allocated once per map, re-uploaded each blit.
    staged_regions: RegionCache<usize>,
}

impl GpuAssets {
    /// Build the asset store on the shared device/queue.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> GpuAssets {
        let max = device.limits().max_texture_dimension_2d;
        let model_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("r274 model texture array"),
            size: wgpu::Extent3d {
                width: MODEL_CELL,
                height: MODEL_CELL,
                depth_or_array_layers: MODEL_LAYERS,
            },
            mip_level_count: MODEL_MIP_LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let model_view = model_atlas.create_view(&Default::default());
        // RuneLite default anisotropic=1: mag nearest, min NEAREST_MIPMAP_LINEAR
        // (nearest in-mip, linear between mips). Linear-without-mips was the
        // notice-board / ground-clutter crawl.
        let model_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("r274 model sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let model_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("r274 model texture array layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2Array,
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
            label: Some("r274 model texture array group"),
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

        let sprite_atlas = LayerAtlas::new(device, "r274 sprite atlas", SPRITE_ATLAS, max);
        let font_atlas = GpuAtlas::new(device, "r274 font atlas", FONT_ATLAS, max, 0);
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
                    // Sprite atlas layers 0..7.
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 7,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // The font atlas.
                    wgpu::BindGroupLayoutEntry {
                        binding: 8,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // The shared sampler.
                    wgpu::BindGroupLayoutEntry {
                        binding: 9,
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
            font_free: Vec::new(),
            chrome_sampler,
            chrome_bind_group_layout,
            chrome_bind_group,
            chrome_generations: (0, 0),
            epoch: 0,
            sprite_regions: RegionCache::new(SPRITE_REGION_BOUND),
            map_regions: RegionCache::new(MAP_REGION_BOUND),
            glyph_regions: RegionCache::new(GLYPH_REGION_BOUND),
            staged_regions: RegionCache::new(STAGED_REGION_BOUND),
        };
        assets.refresh_chrome_bind_group();
        assets
    }

    /// The chrome quad pipeline's bind group (sprite layers + font atlas +
    /// sampler), rebuilt when an atlas grows or a layer is appended.
    pub fn chrome_bind_group(&self) -> &wgpu::BindGroup {
        &self.chrome_bind_group
    }

    /// The chrome bind group layout (the quad pipeline references it).
    pub fn chrome_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.chrome_bind_group_layout
    }

    /// The current frame epoch (a renderer's `frame_begin` bumps it before
    /// recording, so evictions tag regions and reuse waits for the next
    /// frame).
    pub fn bump_epoch(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    /// The current epoch (the renderer passes it into uploads).
    pub fn epoch(&self) -> u64 {
        self.epoch
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

    /// Rebuild the chrome bind group when an atlas has grown since the
    /// last rebuild (called after every chrome upload).
    fn refresh_chrome_bind_group(&mut self) {
        if (self.sprite_atlas.generation(), self.font_atlas.generation()) != self.chrome_generations
        {
            self.rebuild_chrome_bind_group();
        }
    }

    /// The sprite-atlas region for a `Pix8` sprite, uploading it (palette →
    /// RGBA, index 0 transparent) on first use. `None` when every layer is
    /// full (the frame then skips the sprite).
    pub fn sprite_region_pix8(&mut self, sprite: &Pix8, epoch: u64) -> Option<Region> {
        let key = sprite.data.as_ptr() as usize;
        if let Some(region) = self.sprite_regions.get(key, epoch) {
            return Some(region);
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
        self.upload_sprite(key, w, h, &rgba, epoch)
    }

    /// The sprite-atlas region for a `Pix32` sprite (direct RGBA, 0
    /// transparent), uploading it on first use.
    pub fn sprite_region_pix32(&mut self, sprite: &Pix32, epoch: u64) -> Option<Region> {
        let key = sprite.data.as_ptr() as usize;
        if let Some(region) = self.sprite_regions.get(key, epoch) {
            return Some(region);
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
        self.upload_sprite(key, w, h, &rgba, epoch)
    }

    /// A map region for a static `PixMap` (the chrome strips, the title
    /// JPEG regions): upload once, blit as an opaque quad. The map's
    /// pixels are read verbatim (`0x00RRGGBB`).
    pub fn map_region(&mut self, map: &PixMap, epoch: u64) -> Option<Region> {
        let key = map.pixels.as_ptr() as usize;
        if let Some(region) = self.map_regions.get(key, epoch) {
            return Some(region);
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
        self.upload_sprite(key, w, h, &rgba, epoch)
    }

    /// A font-atlas region for a glyph mask (white RGB, alpha = mask),
    /// uploading the mask on first use (keyed by the mask pointer + size).
    pub fn glyph_region(&mut self, mask: &[i8], w: u32, h: u32, epoch: u64) -> Option<Region> {
        let key = (mask.as_ptr() as usize, w, h);
        if let Some(region) = self.glyph_regions.get(key, epoch) {
            return Some(region);
        }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for i in 0..(w * h) as usize {
            let on = mask.get(i).copied().unwrap_or(0) != 0;
            rgba.extend_from_slice(&[255, 255, 255, if on { 255 } else { 0 }]);
        }
        let region = self.alloc_font(w, h, epoch)?;
        self.write_atlas(self.font_atlas.texture(), region, &rgba);
        self.refresh_chrome_bind_group();
        if let Some((freed, _)) = self.glyph_regions.insert(key, region, epoch) {
            self.font_free.push((freed, epoch));
        }
        Some(region)
    }

    /// Upload the current pixels of a staged map into its region and
    /// return the region.
    pub fn staged_upload(&mut self, tag: usize, map: &PixMap, epoch: u64) -> Option<Region> {
        let (w, h) = (map.width as u32, map.height as u32);
        let region = match self.staged_regions.get(tag, epoch) {
            Some(region) => region,
            None => {
                let region = self
                    .sprite_atlas
                    .alloc(&self.device, &self.queue, w, h, epoch)?;
                if let Some((freed, _)) = self.staged_regions.insert(tag, region, epoch) {
                    self.sprite_atlas.free_region(freed, epoch);
                }
                region
            }
        };
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for &rgb in &map.pixels {
            rgba.extend_from_slice(&[
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
                255,
            ]);
        }
        self.write_atlas(self.sprite_atlas.layer_texture(region.layer), region, &rgba);
        self.refresh_chrome_bind_group();
        Some(region)
    }

    /// The current size of one sprite layer (the quad flush resolves
    /// region UVs against it — layers may have grown).
    pub fn sprite_layer_size(&self, layer: u32) -> (u32, u32) {
        self.sprite_atlas.layer_size(layer)
    }

    /// The current size of every sprite layer (the quad flush).
    pub fn sprite_layer_sizes(&self) -> Vec<(u32, u32)> {
        self.sprite_atlas.layer_sizes()
    }

    /// The current font-atlas size.
    pub fn font_atlas_size(&self) -> (u32, u32) {
        self.font_atlas.size()
    }

    /// The sprite layer count (tests).
    pub fn sprite_layer_count(&self) -> usize {
        self.sprite_atlas.layer_count()
    }

    /// Allocate a sprite-atlas region, upload the RGBA pixels, and cache
    /// the region under `key` (an LRU eviction returns the evicted region
    /// to the free list).
    fn upload_sprite(
        &mut self,
        key: usize,
        w: u32,
        h: u32,
        rgba: &[u8],
        epoch: u64,
    ) -> Option<Region> {
        let region = self
            .sprite_atlas
            .alloc(&self.device, &self.queue, w, h, epoch)?;
        self.write_atlas(self.sprite_atlas.layer_texture(region.layer), region, rgba);
        self.refresh_chrome_bind_group();
        if let Some((freed, _)) = self.sprite_regions.insert(key, region, epoch) {
            self.sprite_atlas.free_region(freed, epoch);
        }
        Some(region)
    }

    /// Allocate a font-atlas region, reusing an exact-size evicted region
    /// from a previous epoch first.
    fn alloc_font(&mut self, w: u32, h: u32, epoch: u64) -> Option<Region> {
        if let Some(i) = self
            .font_free
            .iter()
            .position(|(r, e)| *e < epoch && r.w == w && r.h == h)
        {
            return Some(self.font_free.remove(i).0);
        }
        self.font_atlas.alloc(&self.device, &self.queue, w, h)
    }

    /// Write `rgba` into an atlas layer at `region` (the caller refreshes
    /// the chrome bind group after — the alloc may have grown the atlas).
    fn write_atlas(&self, texture: &wgpu::Texture, region: Region, rgba: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region.x,
                    y: region.y,
                    z: 0,
                },
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

    /// Write lod 0 and the box-filter mip chain for one array layer.
    /// Rows of mips smaller than 64px are padded to wgpu's 256-byte copy
    /// alignment.
    fn write_model_mips(&self, layer: u32, lod0: &[u8]) {
        const ALIGN: u32 = 256;
        for (level, (width, rgba)) in rgba_mip_chain(lod0, MODEL_CELL).into_iter().enumerate() {
            let unpadded = width * 4;
            let padded = unpadded.div_ceil(ALIGN) * ALIGN;
            let height = width;
            let data: Vec<u8> = if padded == unpadded {
                rgba
            } else {
                let mut buf = vec![0u8; (padded * height) as usize];
                let u = unpadded as usize;
                let p = padded as usize;
                for y in 0..height as usize {
                    buf[y * p..y * p + u].copy_from_slice(&rgba[y * u..(y + 1) * u]);
                }
                buf
            };
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.model_atlas,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Upload any of the renderer's model textures not yet in the array
    /// (once per process, keyed by texture id). A renderer without the
    /// `textures` jag (or a failed depack) leaves its id unset; the scene
    /// mesh then samples nothing for those faces.
    pub fn ensure_model_textures(&mut self, pix: &Pix3DDraw) {
        for id in 0..50 {
            if self.model_regions[id] {
                continue;
            }
            let (Some(texture), Some(palette)) = (&pix.textures[id], &pix.tex_pal[id]) else {
                if crate::render_debug_enabled() {
                    eprintln!(
                        "[gpu-atlas] texture {id} skipped (texture={} palette={})",
                        pix.textures[id].is_some(),
                        pix.tex_pal[id].is_some()
                    );
                }
                continue;
            };
            let mut rgba = vec![0u8; (MODEL_CELL * MODEL_CELL * 4) as usize];
            if texture.wi == 128 {
                for y in 0..MODEL_CELL as usize {
                    for x in 0..MODEL_CELL as usize {
                        let data = texture
                            .data
                            .get(x + y * MODEL_CELL as usize)
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
            let layer = id as u32;
            self.write_model_mips(layer, &rgba);
            self.model_regions[id] = true;
            if crate::render_debug_enabled() {
                let opaque = rgba.iter().skip(3).step_by(4).filter(|&&a| a != 0).count();
                eprintln!(
                    "[gpu-atlas] texture {id} uploaded wi={} hi={} opaque={}/{}",
                    texture.wi,
                    texture.hi,
                    opaque,
                    MODEL_CELL * MODEL_CELL
                );
            }
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

/// Build the chrome quad pipeline's bind group: the sprite atlas layers
/// (bindings 0..7, padded with the last real layer's view), the font atlas
/// (binding 8) and the shared sampler (binding 9).
fn build_chrome_bind_group(
    device: &wgpu::Device,
    sprite_atlas: &LayerAtlas,
    font_atlas: &GpuAtlas,
    chrome_sampler: &wgpu::Sampler,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::BindGroup {
    let views = sprite_atlas.views_padded();
    let mut entries: Vec<wgpu::BindGroupEntry> = views
        .iter()
        .enumerate()
        .map(|(i, view)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    entries.push(wgpu::BindGroupEntry {
        binding: MAX_SPRITE_LAYERS as u32,
        resource: wgpu::BindingResource::TextureView(font_atlas.view()),
    });
    entries.push(wgpu::BindGroupEntry {
        binding: MAX_SPRITE_LAYERS as u32 + 1,
        resource: wgpu::BindingResource::Sampler(chrome_sampler),
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("r274 chrome atlas group"),
        layout,
        entries: &entries,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_rgba_averages_a_2x2_checker() {
        // red, blue / red, blue → one magenta-ish texel.
        let src = [
            255, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 255, 255,
        ];
        let mip = downsample_rgba(&src, 2);
        assert_eq!(mip, [127, 0, 127, 255]);
    }

    #[test]
    fn rgba_mip_chain_from_128_has_eight_levels() {
        let lod0 = vec![255u8; (MODEL_CELL * MODEL_CELL * 4) as usize];
        let chain = rgba_mip_chain(&lod0, MODEL_CELL);
        assert_eq!(chain.len(), MODEL_MIP_LEVELS as usize);
        let widths: Vec<u32> = chain.iter().map(|(w, _)| *w).collect();
        assert_eq!(widths, vec![128, 64, 32, 16, 8, 4, 2, 1]);
        assert_eq!(chain[7].1.len(), 4);
        assert_eq!(&chain[7].1, &[255, 255, 255, 255]);
    }

    #[test]
    fn model_atlas_texture_reports_eight_mips() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no adapter; skipping");
            return;
        };
        let assets = GpuAssets::new(&device, &queue);
        assert_eq!(
            assets.model_atlas.mip_level_count(),
            MODEL_MIP_LEVELS,
            "RuneLite TextureManager allocates 8 mip levels on the 128px array"
        );
    }

    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        pollster::block_on(adapter.request_device(&Default::default())).ok()
    }

    /// A layer's height never exceeds its cap: allocating past the cap
    /// returns `None` instead of asking the device for an over-limit
    /// texture (the operator-reported 16384 panic).
    #[test]
    fn atlas_growth_stops_at_the_cap() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no adapter; skipping");
            return;
        };
        let mut atlas = GpuAtlas::new(&device, "test", (8, 8), 16, 0);
        let mut count = 0;
        // 4×4 regions: 2 per shelf, 8 per 8×16 layer → the 9th returns None.
        while atlas.alloc(&device, &queue, 4, 4).is_some() {
            count += 1;
        }
        assert_eq!(count, 8);
        assert_eq!(
            atlas.size,
            (8, 16),
            "the layer must stop at the cap, never grow past it"
        );
    }

    /// Filling past one layer spills to fresh layers (never a panic), and
    /// the layer count is bounded by `MAX_SPRITE_LAYERS`.
    #[test]
    fn layer_atlas_spills_without_overflow() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no adapter; skipping");
            return;
        };
        let mut atlas = LayerAtlas::new(&device, "test", (8, 8), 16);
        // A free list empty and every layer 8×16 full: 8 regions per layer,
        // so the 8×8+1th alloc returns None, not a panic.
        let mut count = 0;
        while atlas.alloc(&device, &queue, 4, 4, 1).is_some() {
            count += 1;
        }
        assert_eq!(count, 8 * MAX_SPRITE_LAYERS);
        assert_eq!(atlas.layer_count(), MAX_SPRITE_LAYERS);
    }

    /// An evicted region is reused by an exact-size alloc from the next
    /// epoch, so reallocated sources stop growing the atlas.
    #[test]
    fn evicted_regions_are_reused_from_the_next_epoch() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no adapter; skipping");
            return;
        };
        let mut atlas = LayerAtlas::new(&device, "test", (8, 8), 16);
        let first = atlas.alloc(&device, &queue, 4, 4, 1).unwrap();
        assert_eq!(atlas.layer_count(), 1);
        // Freed at epoch 1: not reusable at epoch 1 (same frame)...
        atlas.free_region(first, 1);
        let second = atlas.alloc(&device, &queue, 4, 4, 1).unwrap();
        assert_ne!(
            second, first,
            "a region freed this frame is not reused this frame"
        );
        assert_eq!(atlas.layer_count(), 1);
        // ...but reusable from epoch 2 on (the freeing frame is flushed).
        atlas.free_region(second, 1);
        let third = atlas.alloc(&device, &queue, 4, 4, 2).unwrap();
        assert!(
            third == first || third == second,
            "an exact-size region freed last frame is reused (got {third:?})"
        );
        assert_eq!(atlas.layer_count(), 1);
    }

    /// The bounded LRU caches evict the least-recently-used entry and
    /// return its region for reclamation, so stale keys cannot grow the
    /// atlas without bound.
    #[test]
    fn region_cache_evicts_the_lru_entry() {
        let r = |n: u32| Region {
            x: n,
            y: 0,
            w: 1,
            h: 1,
            layer: 0,
        };
        let mut cache = RegionCache::<u32>::new(3);
        assert_eq!(cache.insert(1, r(1), 1), None);
        assert_eq!(cache.insert(2, r(2), 1), None);
        assert_eq!(cache.insert(3, r(3), 1), None);
        // Entry 1 is the LRU (never re-touched); inserting 4 evicts it.
        cache.get(2, 1);
        let (freed, _) = cache.insert(4, r(4), 1).unwrap();
        assert_eq!(freed, r(1), "the least-recently-used region is returned");
        assert_eq!(cache.map.len(), 3);
        assert!(
            cache.get(1, 2).is_none(),
            "the evicted key must miss (it re-uploads)"
        );
        assert!(cache.get(2, 2).is_some());
        // A fresh key with the cache full evicts again.
        cache.insert(5, r(5), 1);
        assert_eq!(cache.map.len(), 3);
    }

    /// Reallocated sources stop growing the atlas: uploading far more
    /// distinct sprites than a single 8192-cap layer could ever hold (and
    /// more than the LRU bound) across several frames keeps the atlas on
    /// one layer — the evicted regions are reclaimed instead of leaked.
    #[test]
    fn stale_sprite_sources_stop_growing_the_atlas() {
        let Some((device, queue)) = test_device() else {
            eprintln!("no adapter; skipping");
            return;
        };
        let mut assets = GpuAssets::new(&device, &queue);
        let mut sprites = Vec::new();
        for _batch in 0..3 {
            let epoch = assets.bump_epoch();
            // 15000 distinct 16×16 sprites per "frame": the live set far
            // exceeds the LRU bound (4096), so evictions reclaim regions.
            for _ in 0..15000 {
                let mut sprite = Pix8::new(16, 16, vec![0, 0xffffff]);
                sprite.data[0] = 1; // make every sprite opaque (distinct vec)
                sprites.push(sprite);
                let sprite = sprites.last().unwrap();
                assert!(
                    assets.sprite_region_pix8(sprite, epoch).is_some(),
                    "a sprite upload must not fail while layers have room"
                );
            }
        }
        // The LRU bound keeps the cache capped and the reclaimed regions
        // keep the atlas on one layer — no unbounded multi-layer growth.
        assert!(assets.sprite_regions.map.len() <= 4096);
        assert_eq!(
            assets.sprite_layer_count(),
            1,
            "evicted regions must be reclaimed, not leaked into more layers"
        );
    }
}
