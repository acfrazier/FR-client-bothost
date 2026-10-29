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
use crate::render::budget::{BudgetCharge, ByteBudget, RenderBudget, RenderLimit, RenderResource};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

/// The texture array: 50 textures max, one 128×128 layer per id.
const MODEL_LAYERS: u32 = 50;
const MODEL_CELL: u32 = 128;
/// RuneLite `TextureManager`: `glTexStorage3D(..., 8, GL_RGBA8, 128, 128, …)`.
const MODEL_MIP_LEVELS: u32 = 8;

fn budget_error(budget: &ByteBudget) -> RenderLimit {
    RenderLimit::new(budget.resource(), usize::MAX, budget.maximum())
}

fn rgba_texture_bytes(width: u32, height: u32, layers: u32, mip_levels: u32) -> Option<usize> {
    let mut width = width as usize;
    let mut height = height as usize;
    let layers = layers as usize;
    let mut total = 0usize;
    for _ in 0..mip_levels {
        total = total.checked_add(
            width
                .checked_mul(height)?
                .checked_mul(layers)?
                .checked_mul(4)?,
        )?;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    Some(total)
}

struct ScratchBytes {
    bytes: Vec<u8>,
    _charge: Option<BudgetCharge>,
}

impl ScratchBytes {
    fn zeroed(len: usize, budget: Option<&ByteBudget>) -> Result<Self, RenderLimit> {
        let charge = budget.map(|budget| budget.reserve(len)).transpose()?;
        let mut bytes = Vec::new();
        if let Some(budget) = budget {
            bytes
                .try_reserve_exact(len)
                .map_err(|_| budget_error(budget))?;
        } else {
            bytes.reserve_exact(len);
        }
        bytes.resize(len, 0);
        Ok(Self {
            bytes,
            _charge: charge,
        })
    }
}

/// Average a square RGBA8 image down 2× (box filter). Used to fill the
/// model-atlas mip chain after lod 0, matching `glGenerateMipmap`.
fn downsample_rgba(src: &[u8], width: u32) -> Vec<u8> {
    let side = width as usize / 2;
    let mut dst = vec![0u8; side * side * 4];
    downsample_rgba_into(src, width, &mut dst);
    dst
}

fn downsample_rgba_into(src: &[u8], width: u32, dst: &mut [u8]) {
    let w = width as usize;
    let hw = w / 2;
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
    storage: crate::profiling::Allocation,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    max: u32,
    layer: u32,
    x: u32,
    y: u32,
    shelf_h: u32,
    generation: u64,
    gpu_budget: Option<ByteBudget>,
    gpu_charge: Option<BudgetCharge>,
}

impl GpuAtlas {
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        size: (u32, u32),
        max: u32,
        layer: u32,
    ) -> Result<GpuAtlas, RenderLimit> {
        let gpu_budget = RenderBudget::current_resource(RenderResource::ContextGpu);
        let bytes = rgba_texture_bytes(size.0, size.1, 1, 1).ok_or_else(|| {
            gpu_budget
                .as_ref()
                .map(budget_error)
                .unwrap_or(RenderLimit::new(RenderResource::ContextGpu, usize::MAX, 0))
        })?;
        let gpu_charge = gpu_budget
            .as_ref()
            .map(|budget| budget.reserve(bytes))
            .transpose()?;
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
        Ok(GpuAtlas {
            storage: crate::profiling::Allocation::new(
                0,
                crate::profiling::texture_bytes(&texture),
            ),
            texture,
            view,
            size,
            max,
            layer,
            x: 0,
            y: 0,
            shelf_h: 0,
            generation: 0,
            gpu_budget,
            gpu_charge,
        })
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
    ) -> Result<Option<Region>, RenderLimit> {
        if let Some(current) = RenderBudget::current_resource(RenderResource::ContextGpu) {
            if !self
                .gpu_budget
                .as_ref()
                .is_some_and(|budget| budget.same_budget(&current))
            {
                return Err(budget_error(&current));
            }
        }
        if w > self.size.0 || h > self.max {
            return Ok(None);
        }
        if self.x + w > self.size.0 {
            // Next shelf.
            self.x = 0;
            self.y += self.shelf_h;
            self.shelf_h = 0;
        }
        while self.y + h > self.size.1 {
            if !self.grow(device, queue)? {
                return Ok(None);
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
        Ok(Some(region))
    }

    /// Double the height (capped at `max`): new texture, old content
    /// copied (an encoder submitted immediately, so any later
    /// `write_texture` on the queue lands after the copy). `false` when
    /// the layer is already at the cap.
    fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<bool, RenderLimit> {
        let (old_w, old_h) = self.size;
        let new_h = (old_h * 2).min(self.max);
        if new_h == old_h {
            return Ok(false);
        }
        let new_size = (old_w, new_h);
        let new_bytes = rgba_texture_bytes(new_size.0, new_size.1, 1, 1).ok_or_else(|| {
            self.gpu_budget
                .as_ref()
                .map(budget_error)
                .unwrap_or(RenderLimit::new(RenderResource::ContextGpu, usize::MAX, 0))
        })?;
        let growth_charge = match &self.gpu_budget {
            Some(budget) => {
                if RenderBudget::current_resource(RenderResource::ContextGpu)
                    .is_some_and(|current| !current.same_budget(budget))
                {
                    return Err(budget_error(budget));
                }
                if self.gpu_charge.is_none() {
                    return Err(budget_error(budget));
                }
                Some(budget.reserve(new_bytes)?)
            }
            None => {
                if let Some(current) = RenderBudget::current_resource(RenderResource::ContextGpu) {
                    return Err(budget_error(&current));
                }
                None
            }
        };
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
        let storage =
            crate::profiling::Allocation::new(0, crate::profiling::texture_bytes(&new_texture));
        self.texture = new_texture;
        self.storage = storage;
        self.gpu_charge = growth_charge;
        self.view = self.texture.create_view(&Default::default());
        self.size = new_size;
        self.generation += 1;
        Ok(true)
    }
    fn logical_gpu_bytes(&self) -> usize {
        super::gpu::logical_texture_bytes(&self.texture)
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
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        size: (u32, u32),
        max: u32,
    ) -> Result<LayerAtlas, RenderLimit> {
        let layers = vec![GpuAtlas::new(device, label, size, max, 0)?];
        Ok(LayerAtlas {
            layers,
            free: Vec::new(),
            max,
        })
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
    ) -> Result<Option<Region>, RenderLimit> {
        if let Some(i) = self
            .free
            .iter()
            .position(|f| f.freed_epoch < epoch && f.region.w == w && f.region.h == h)
        {
            let free = self.free.remove(i);
            return Ok(Some(free.region));
        }
        for i in 0..self.layers.len() {
            if let Some(region) = self.layers[i].alloc(device, queue, w, h)? {
                return Ok(Some(region));
            }
        }
        if self.layers.len() < MAX_SPRITE_LAYERS {
            let layer = self.layers.len() as u32;
            // A fresh layer starts at the atlas width, min(1024, cap).
            let initial = (self.layers[0].size.0, self.max.min(1024));
            let mut fresh =
                GpuAtlas::new(device, "r274 sprite atlas layer", initial, self.max, layer)?;
            if let Some(region) = fresh.alloc(device, queue, w, h)? {
                self.layers.push(fresh);
                return Ok(Some(region));
            }
        }
        Ok(None)
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
    fn retained_cpu_bytes(&self) -> usize {
        self.layers
            .capacity()
            .saturating_mul(std::mem::size_of::<GpuAtlas>())
            .saturating_add(
                self.free
                    .capacity()
                    .saturating_mul(std::mem::size_of::<FreeRegion>()),
            )
    }

    fn logical_gpu_bytes(&self) -> usize {
        self.layers
            .iter()
            .map(GpuAtlas::logical_gpu_bytes)
            .fold(0usize, usize::saturating_add)
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
    fn retained_bytes(&self) -> usize {
        let table_bytes = if self.map.capacity() == 0 {
            0
        } else {
            let buckets = self
                .map
                .capacity()
                .saturating_mul(8)
                .div_ceil(7)
                .max(4)
                .checked_next_power_of_two()
                .unwrap_or(usize::MAX);
            buckets
                .saturating_mul(std::mem::size_of::<(K, CachedRegion)>())
                .saturating_add(buckets)
                .saturating_add(16)
        };
        table_bytes.saturating_add(
            self.order
                .capacity()
                .saturating_mul(std::mem::size_of::<K>()),
        )
    }
}

#[cfg(feature = "render-test-hooks")]
fn prior_growth_height(initial: u32, max: u32) -> u32 {
    if initial >= max {
        return 0;
    }
    let mut height = initial;
    loop {
        let next = height.saturating_mul(2).min(max);
        if next == height {
            return 0;
        }
        if next == max {
            return height;
        }
        height = next;
    }
}

#[cfg(feature = "render-test-hooks")]
fn model_mip_peak_scratch_bytes() -> Option<usize> {
    const COPY_ALIGNMENT: u32 = 256;
    let lod0_bytes = rgba_texture_bytes(MODEL_CELL, MODEL_CELL, 1, 1)?;
    let mut previous_bytes = 0usize;
    let mut width = MODEL_CELL;
    let mut peak = 0usize;
    loop {
        let row_bytes = width.checked_mul(4)?;
        let padded_row_bytes = row_bytes.div_ceil(COPY_ALIGNMENT) * COPY_ALIGNMENT;
        let padded_bytes = if padded_row_bytes == row_bytes {
            0
        } else {
            (padded_row_bytes as usize).checked_mul(width as usize)?
        };
        let next_bytes = if width > 1 {
            let next_width = (width / 2) as usize;
            next_width.checked_mul(next_width)?.checked_mul(4)?
        } else {
            0
        };
        peak = peak.max(
            lod0_bytes
                .checked_add(previous_bytes)?
                .checked_add(padded_bytes)?
                .checked_add(next_bytes)?,
        );
        if width == 1 {
            break;
        }
        previous_bytes = next_bytes;
        width /= 2;
    }
    Some(peak)
}

#[cfg(feature = "render-test-hooks")]
fn region_cache_capacity_bytes<K>(bound: usize) -> Option<usize> {
    let buckets = bound
        .checked_mul(8)?
        .div_ceil(7)
        .max(4)
        .checked_next_power_of_two()?;
    let table = buckets
        .checked_mul(std::mem::size_of::<(K, CachedRegion)>())?
        .checked_add(buckets)?
        .checked_add(16)?;
    let order_capacity = bound.checked_next_power_of_two()?;
    table.checked_add(order_capacity.checked_mul(std::mem::size_of::<K>())?)
}

/// The shared, lazily-initialised GPU asset store (one per process). The
/// `GpuContext` owns the `Arc`; renderers clone it. `Mutex` because the
/// atlases grow on first use from any renderer's thread.
pub struct GpuAssets {
    _model_storage: crate::profiling::Allocation,
    _model_charge: Option<BudgetCharge>,
    context_cpu_budget: Option<ByteBudget>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The model-texture array texture (scene shader group 0, binding 0).
    model_atlas: wgpu::Texture,
    /// The scene pipeline's bind group layout (texture + sampler).
    pub model_bind_group_layout: wgpu::BindGroupLayout,
    /// The scene pipeline's bind group, bound once per frame.
    pub model_bind_group: wgpu::BindGroup,
    /// Which static texture ids are already in the array (upload once per
    /// process). Animated ids 17/24 are restaged for every scene.
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

/// Source-defined GPU atlas limits and element layouts for the ignored
/// full-content census.
#[cfg(feature = "render-test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTestAtlasLayout {
    pub max_texture_dimension_2d: u32,
    pub max_storage_buffer_binding_size: u32,
    pub sprite_layer_count: usize,
    pub model_gpu_bytes: usize,
    pub sprite_layer_gpu_bytes: usize,
    pub font_gpu_bytes: usize,
    pub sprite_old_layer_gpu_bytes: usize,
    pub font_old_texture_gpu_bytes: usize,
    pub context_gpu_replacement_peak_bytes: usize,
    pub model_mip_peak_scratch_bytes: usize,
    pub gpu_assets_bytes: usize,
    pub gpu_atlas_bytes: usize,
    pub free_region_bytes: usize,
    pub sprite_free_region_capacity: usize,
    pub font_free_region_capacity: usize,
    pub sprite_atlas_cpu_bytes: usize,
    pub font_free_bytes: usize,
    pub region_cache_bytes: usize,
    pub sprite_region_bound: usize,
    pub map_region_bound: usize,
    pub glyph_region_bound: usize,
    pub staged_region_bound: usize,
}

impl GpuAssets {
    /// Build the asset store on the shared device/queue.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<GpuAssets, RenderLimit> {
        let context_cpu_budget = RenderBudget::current_resource(RenderResource::ContextCpu);
        let context_gpu_budget = RenderBudget::current_resource(RenderResource::ContextGpu);
        let model_bytes =
            rgba_texture_bytes(MODEL_CELL, MODEL_CELL, MODEL_LAYERS, MODEL_MIP_LEVELS).ok_or_else(
                || {
                    context_gpu_budget
                        .as_ref()
                        .map(budget_error)
                        .unwrap_or(RenderLimit::new(RenderResource::ContextGpu, usize::MAX, 0))
                },
            )?;
        let model_charge = context_gpu_budget
            .as_ref()
            .map(|budget| budget.reserve(model_bytes))
            .transpose()?;
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
        // Match Java's model raster: U is clamped before lookup, while V
        // wraps through the texture-height mask (`0x3f80` high-memory,
        // `0xfc0` low-memory). Keep RuneLite's anisotropic=1 filtering:
        // mag nearest, min NEAREST_MIPMAP_LINEAR (nearest in-mip, linear
        // between mips). Linear-without-mips caused notice-board / ground-
        // clutter crawl.
        let model_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("r274 model sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::Repeat,
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

        let sprite_atlas = LayerAtlas::new(device, "r274 sprite atlas", SPRITE_ATLAS, max)?;
        let font_atlas = GpuAtlas::new(device, "r274 font atlas", FONT_ATLAS, max, 0)?;
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
            _model_storage: crate::profiling::Allocation::new(
                0,
                crate::profiling::texture_bytes(&model_atlas),
            ),
            _model_charge: model_charge,
            context_cpu_budget,
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
        Ok(assets)
    }

    /// Independent capacity census for shared CPU metadata and logical GPU
    /// atlas storage. Budget counters are deliberately not used as the source.
    pub(crate) fn render_bytes(&self) -> (usize, usize) {
        let cpu = std::mem::size_of::<Self>()
            .saturating_add(
                self.font_free
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(Region, u64)>()),
            )
            .saturating_add(self.sprite_atlas.retained_cpu_bytes())
            .saturating_add(self.sprite_regions.retained_bytes())
            .saturating_add(self.map_regions.retained_bytes())
            .saturating_add(self.glyph_regions.retained_bytes())
            .saturating_add(self.staged_regions.retained_bytes());
        let gpu = super::gpu::logical_texture_bytes(&self.model_atlas)
            .saturating_add(self.sprite_atlas.logical_gpu_bytes())
            .saturating_add(self.font_atlas.logical_gpu_bytes());
        (cpu, gpu)
    }

    /// Exact atlas dimensions, structural cache ceilings, and mip scratch
    /// maxima for the ignored full-content census. Does not create a device
    /// or traverse the process-owned assets.
    #[cfg(feature = "render-test-hooks")]
    pub fn render_test_content_layout() -> Option<RenderTestAtlasLayout> {
        let limits = wgpu::Limits::default();
        let max_texture_dimension_2d = limits.max_texture_dimension_2d;
        let max_storage_buffer_binding_size = limits.max_storage_buffer_binding_size;
        let sprite_height = max_texture_dimension_2d;
        let font_height = max_texture_dimension_2d;
        let sprite_layer_gpu_bytes = rgba_texture_bytes(SPRITE_ATLAS.0, sprite_height, 1, 1)?;
        let font_gpu_bytes = rgba_texture_bytes(FONT_ATLAS.0, font_height, 1, 1)?;
        let model_gpu_bytes =
            rgba_texture_bytes(MODEL_CELL, MODEL_CELL, MODEL_LAYERS, MODEL_MIP_LEVELS)?;
        let sprite_old_layer_gpu_bytes = rgba_texture_bytes(
            SPRITE_ATLAS.0,
            prior_growth_height(SPRITE_ATLAS.1, sprite_height),
            1,
            1,
        )?;
        let font_old_texture_gpu_bytes = rgba_texture_bytes(
            FONT_ATLAS.0,
            prior_growth_height(FONT_ATLAS.1, font_height),
            1,
            1,
        )?;
        let sprite_final_bytes = sprite_layer_gpu_bytes.checked_mul(MAX_SPRITE_LAYERS)?;
        let sprite_replacement_peak = sprite_final_bytes.checked_add(sprite_old_layer_gpu_bytes)?;
        let font_replacement_peak = font_gpu_bytes.checked_add(font_old_texture_gpu_bytes)?;
        let context_gpu_replacement_peak_bytes = model_gpu_bytes.checked_add(
            sprite_replacement_peak
                .checked_add(font_gpu_bytes)?
                .max(sprite_final_bytes.checked_add(font_replacement_peak)?),
        )?;
        let model_mip_peak_scratch_bytes = model_mip_peak_scratch_bytes()?;
        let sprite_free_region_capacity = (SPRITE_ATLAS.0 as usize)
            .checked_mul(sprite_height as usize)?
            .checked_mul(MAX_SPRITE_LAYERS)?;
        let font_free_region_capacity =
            (FONT_ATLAS.0 as usize).checked_mul(font_height as usize)?;
        let sprite_atlas_cpu_bytes = MAX_SPRITE_LAYERS
            .checked_mul(std::mem::size_of::<GpuAtlas>())?
            .checked_add(
                sprite_free_region_capacity.checked_mul(std::mem::size_of::<FreeRegion>())?,
            )?;
        let font_free_bytes =
            font_free_region_capacity.checked_mul(std::mem::size_of::<(Region, u64)>())?;
        let region_cache_bytes = region_cache_capacity_bytes::<usize>(SPRITE_REGION_BOUND)?
            .checked_add(region_cache_capacity_bytes::<usize>(MAP_REGION_BOUND)?)?
            .checked_add(region_cache_capacity_bytes::<(usize, u32, u32)>(
                GLYPH_REGION_BOUND,
            )?)?
            .checked_add(region_cache_capacity_bytes::<usize>(STAGED_REGION_BOUND)?)?;
        Some(RenderTestAtlasLayout {
            max_texture_dimension_2d,
            max_storage_buffer_binding_size,
            sprite_layer_count: MAX_SPRITE_LAYERS,
            model_gpu_bytes,
            sprite_layer_gpu_bytes,
            font_gpu_bytes,
            sprite_old_layer_gpu_bytes,
            font_old_texture_gpu_bytes,
            context_gpu_replacement_peak_bytes,
            model_mip_peak_scratch_bytes,
            gpu_assets_bytes: std::mem::size_of::<GpuAssets>(),
            gpu_atlas_bytes: std::mem::size_of::<GpuAtlas>(),
            free_region_bytes: std::mem::size_of::<FreeRegion>(),
            sprite_free_region_capacity,
            font_free_region_capacity,
            sprite_atlas_cpu_bytes,
            font_free_bytes,
            region_cache_bytes,
            sprite_region_bound: SPRITE_REGION_BOUND,
            map_region_bound: MAP_REGION_BOUND,
            glyph_region_bound: GLYPH_REGION_BOUND,
            staged_region_bound: STAGED_REGION_BOUND,
        })
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

    fn rgba_len(&self, width: u32, height: u32) -> Result<(usize, usize), RenderLimit> {
        let pixels = (width as usize).checked_mul(height as usize);
        let bytes = pixels.and_then(|pixels| pixels.checked_mul(4));
        match (pixels, bytes, self.context_cpu_budget.as_ref()) {
            (Some(pixels), Some(bytes), _) => Ok((pixels, bytes)),
            (_, _, Some(budget)) => Err(budget_error(budget)),
            _ => {
                let pixels = (width * height) as usize;
                Ok((pixels, (width * height * 4) as usize))
            }
        }
    }

    fn rgba_scratch(&self, bytes: usize) -> Result<ScratchBytes, RenderLimit> {
        ScratchBytes::zeroed(bytes, self.context_cpu_budget.as_ref())
    }

    /// The sprite-atlas region for a `Pix8` sprite, uploading it (palette →
    /// RGBA, index 0 transparent) on first use.
    pub fn sprite_region_pix8(
        &mut self,
        sprite: &Pix8,
        epoch: u64,
    ) -> Result<Option<Region>, RenderLimit> {
        let key = sprite.data.as_ptr() as usize;
        if let Some(region) = self.sprite_regions.get(key, epoch) {
            return Ok(Some(region));
        }
        let (w, h) = (sprite.wi as u32, sprite.hi as u32);
        let (pixel_count, rgba_len) = self.rgba_len(w, h)?;
        let mut rgba = self.rgba_scratch(rgba_len)?;
        for i in 0..pixel_count {
            let index = sprite.data.get(i).copied().unwrap_or(0);
            let offset = i * 4;
            if index == 0 {
                rgba.bytes[offset..offset + 4].copy_from_slice(&[0, 0, 0, 0]);
            } else {
                let rgb = sprite.bpal.get(index as u8 as usize).copied().unwrap_or(0);
                rgba.bytes[offset..offset + 4].copy_from_slice(&[
                    ((rgb >> 16) & 0xff) as u8,
                    ((rgb >> 8) & 0xff) as u8,
                    (rgb & 0xff) as u8,
                    255,
                ]);
            }
        }
        self.upload_sprite(key, w, h, &rgba.bytes, epoch)
    }

    /// The sprite-atlas region for a `Pix32` sprite (direct RGBA, 0
    /// transparent), uploading it on first use.
    pub fn sprite_region_pix32(
        &mut self,
        sprite: &Pix32,
        epoch: u64,
    ) -> Result<Option<Region>, RenderLimit> {
        let key = sprite.data.as_ptr() as usize;
        if let Some(region) = self.sprite_regions.get(key, epoch) {
            return Ok(Some(region));
        }
        let (w, h) = (sprite.wi as u32, sprite.hi as u32);
        let (pixel_count, rgba_len) = self.rgba_len(w, h)?;
        let mut rgba = self.rgba_scratch(rgba_len)?;
        for i in 0..pixel_count {
            let rgb = sprite.data.get(i).copied().unwrap_or(0);
            let offset = i * 4;
            rgba.bytes[offset..offset + 4].copy_from_slice(&[
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
                if rgb != 0 { 255 } else { 0 },
            ]);
        }
        self.upload_sprite(key, w, h, &rgba.bytes, epoch)
    }

    /// A map region for a static `PixMap` (the chrome strips, the title
    /// JPEG regions): upload once, blit as an opaque quad.
    pub fn map_region(&mut self, map: &PixMap, epoch: u64) -> Result<Option<Region>, RenderLimit> {
        let key = map.pixels.as_ptr() as usize;
        if let Some(region) = self.map_regions.get(key, epoch) {
            return Ok(Some(region));
        }
        let (w, h) = (map.width as u32, map.height as u32);
        let (pixel_count, expected_len) = self.rgba_len(w, h)?;
        if self.context_cpu_budget.is_some() && map.pixels.len() != pixel_count {
            return Err(budget_error(self.context_cpu_budget.as_ref().unwrap()));
        }
        let rgba_len = map
            .pixels
            .len()
            .checked_mul(4)
            .filter(|&bytes| self.context_cpu_budget.is_none() || bytes == expected_len)
            .ok_or_else(|| {
                self.context_cpu_budget
                    .as_ref()
                    .map(budget_error)
                    .unwrap_or(RenderLimit::new(RenderResource::ContextCpu, usize::MAX, 0))
            })?;
        let mut rgba = self.rgba_scratch(rgba_len)?;
        for (i, &rgb) in map.pixels.iter().enumerate() {
            let offset = i * 4;
            rgba.bytes[offset..offset + 4].copy_from_slice(&[
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
                255,
            ]);
        }
        self.upload_sprite(key, w, h, &rgba.bytes, epoch)
    }

    /// A font-atlas region for a glyph mask (white RGB, alpha = mask),
    /// uploading the mask on first use (keyed by the mask pointer + size).
    pub fn glyph_region(
        &mut self,
        mask: &[i8],
        w: u32,
        h: u32,
        epoch: u64,
    ) -> Result<Option<Region>, RenderLimit> {
        let key = (mask.as_ptr() as usize, w, h);
        if let Some(region) = self.glyph_regions.get(key, epoch) {
            return Ok(Some(region));
        }
        let (pixel_count, rgba_len) = self.rgba_len(w, h)?;
        let mut rgba = self.rgba_scratch(rgba_len)?;
        for i in 0..pixel_count {
            let on = mask.get(i).copied().unwrap_or(0) != 0;
            let offset = i * 4;
            rgba.bytes[offset..offset + 4].copy_from_slice(&[
                255,
                255,
                255,
                if on { 255 } else { 0 },
            ]);
        }
        let Some(region) = self.alloc_font(w, h, epoch)? else {
            return Ok(None);
        };
        self.write_atlas(self.font_atlas.texture(), region, &rgba.bytes);
        self.refresh_chrome_bind_group();
        if let Some((freed, _)) = self.glyph_regions.insert(key, region, epoch) {
            self.font_free.push((freed, epoch));
        }
        Ok(Some(region))
    }

    /// Upload the current pixels of a staged map into its region and return it.
    pub fn staged_upload(
        &mut self,
        tag: usize,
        map: &PixMap,
        epoch: u64,
    ) -> Result<Option<Region>, RenderLimit> {
        let (w, h) = (map.width as u32, map.height as u32);
        let (pixel_count, expected_len) = self.rgba_len(w, h)?;
        if self.context_cpu_budget.is_some() && map.pixels.len() != pixel_count {
            return Err(budget_error(self.context_cpu_budget.as_ref().unwrap()));
        }
        let rgba_len = map
            .pixels
            .len()
            .checked_mul(4)
            .filter(|&bytes| self.context_cpu_budget.is_none() || bytes == expected_len)
            .ok_or_else(|| {
                self.context_cpu_budget
                    .as_ref()
                    .map(budget_error)
                    .unwrap_or(RenderLimit::new(RenderResource::ContextCpu, usize::MAX, 0))
            })?;
        let region = match self.staged_regions.get(tag, epoch) {
            Some(region) => region,
            None => {
                let Some(region) =
                    self.sprite_atlas
                        .alloc(&self.device, &self.queue, w, h, epoch)?
                else {
                    return Ok(None);
                };
                if let Some((freed, _)) = self.staged_regions.insert(tag, region, epoch) {
                    self.sprite_atlas.free_region(freed, epoch);
                }
                region
            }
        };
        let mut rgba = self.rgba_scratch(rgba_len)?;
        for (i, &rgb) in map.pixels.iter().enumerate() {
            let offset = i * 4;
            rgba.bytes[offset..offset + 4].copy_from_slice(&[
                ((rgb >> 16) & 0xff) as u8,
                ((rgb >> 8) & 0xff) as u8,
                (rgb & 0xff) as u8,
                255,
            ]);
        }
        self.write_atlas(
            self.sprite_atlas.layer_texture(region.layer),
            region,
            &rgba.bytes,
        );
        self.refresh_chrome_bind_group();
        Ok(Some(region))
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
    ) -> Result<Option<Region>, RenderLimit> {
        let Some(region) = self
            .sprite_atlas
            .alloc(&self.device, &self.queue, w, h, epoch)?
        else {
            return Ok(None);
        };
        self.write_atlas(self.sprite_atlas.layer_texture(region.layer), region, rgba);
        self.refresh_chrome_bind_group();
        if let Some((freed, _)) = self.sprite_regions.insert(key, region, epoch) {
            self.sprite_atlas.free_region(freed, epoch);
        }
        Ok(Some(region))
    }

    /// Allocate a font-atlas region, reusing an exact-size evicted region
    /// from a previous epoch first.
    fn alloc_font(&mut self, w: u32, h: u32, epoch: u64) -> Result<Option<Region>, RenderLimit> {
        if let Some(i) = self
            .font_free
            .iter()
            .position(|(r, e)| *e < epoch && r.w == w && r.h == h)
        {
            return Ok(Some(self.font_free.remove(i).0));
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
    fn write_model_mips(&self, layer: u32, lod0: &[u8]) -> Result<(), RenderLimit> {
        const ALIGN: u32 = 256;
        let Some(budget) = self.context_cpu_budget.as_ref() else {
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
            return Ok(());
        };

        let mut previous: Option<ScratchBytes> = None;
        let mut width = MODEL_CELL;
        for level in 0..MODEL_MIP_LEVELS {
            let rgba = previous
                .as_ref()
                .map_or(lod0, |scratch| scratch.bytes.as_slice());
            let unpadded = width * 4;
            let padded = unpadded.div_ceil(ALIGN) * ALIGN;
            let height = width;
            let padded_data = if padded == unpadded {
                None
            } else {
                let len = (padded as usize)
                    .checked_mul(height as usize)
                    .ok_or_else(|| budget_error(budget))?;
                let mut scratch = ScratchBytes::zeroed(len, Some(budget))?;
                let row_bytes = unpadded as usize;
                let padded_row_bytes = padded as usize;
                for y in 0..height as usize {
                    scratch.bytes[y * padded_row_bytes..y * padded_row_bytes + row_bytes]
                        .copy_from_slice(&rgba[y * row_bytes..(y + 1) * row_bytes]);
                }
                Some(scratch)
            };
            let data = padded_data
                .as_ref()
                .map_or(rgba, |scratch| scratch.bytes.as_slice());
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.model_atlas,
                    mip_level: level,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
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
            if width == 1 {
                break;
            }
            let next_width = width / 2;
            let next_len = (next_width as usize)
                .checked_mul(next_width as usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| budget_error(budget))?;
            let mut next = ScratchBytes::zeroed(next_len, Some(budget))?;
            downsample_rgba_into(rgba, width, &mut next.bytes);
            previous = Some(next);
            width = next_width;
        }
        Ok(())
    }

    /// Rewrite one model-texture array layer from the current Pix8 bytes.
    /// Used for static first upload and animated pre-submit staging.
    fn refresh_model_texture(&mut self, pix: &Pix3DDraw, id: usize) -> Result<bool, RenderLimit> {
        if id >= self.model_regions.len() {
            return Ok(false);
        }
        let (Some(texture), Some(palette)) = (&pix.textures[id], &pix.tex_pal[id]) else {
            if crate::render_debug_enabled() {
                eprintln!(
                    "[gpu-atlas] texture {id} skipped (texture={} palette={})",
                    pix.textures[id].is_some(),
                    pix.tex_pal[id].is_some()
                );
            }
            return Ok(false);
        };
        let rgba_len = rgba_texture_bytes(MODEL_CELL, MODEL_CELL, 1, 1).ok_or_else(|| {
            self.context_cpu_budget
                .as_ref()
                .map(budget_error)
                .unwrap_or(RenderLimit::new(RenderResource::ContextCpu, usize::MAX, 0))
        })?;
        let mut rgba = self.rgba_scratch(rgba_len)?;
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
                    rgba.bytes[i] = ((rgb >> 16) & 0xff) as u8;
                    rgba.bytes[i + 1] = ((rgb >> 8) & 0xff) as u8;
                    rgba.bytes[i + 2] = (rgb & 0xff) as u8;
                    rgba.bytes[i + 3] = if rgb != 0 { 255 } else { 0 };
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
                    rgba.bytes[i] = ((rgb >> 16) & 0xff) as u8;
                    rgba.bytes[i + 1] = ((rgb >> 8) & 0xff) as u8;
                    rgba.bytes[i + 2] = (rgb & 0xff) as u8;
                    rgba.bytes[i + 3] = if rgb != 0 { 255 } else { 0 };
                }
            }
        }
        self.write_model_mips(id as u32, &rgba.bytes)?;
        self.model_regions[id] = true;
        if crate::render_debug_enabled() {
            let opaque = rgba
                .bytes
                .iter()
                .skip(3)
                .step_by(4)
                .filter(|&&a| a != 0)
                .count();
            eprintln!(
                "[gpu-atlas] texture {id} uploaded wi={} hi={} opaque={}/{}",
                texture.wi,
                texture.hi,
                opaque,
                MODEL_CELL * MODEL_CELL
            );
        }
        Ok(true)
    }

    /// Upload any static model textures not yet in the array (once per
    /// process, keyed by texture id). Animated layers 17 and 24 are staged
    /// separately for every scene so one slot cannot leave its phase behind
    /// for another slot. A renderer without the `textures` jag (or a failed
    /// depack) leaves its id unset; the scene mesh then samples nothing for
    /// those faces.
    pub fn ensure_model_textures(&mut self, pix: &Pix3DDraw) -> Result<(), RenderLimit> {
        for id in 0..50 {
            if matches!(id, 17 | 24) {
                continue;
            }
            if !self.model_regions[id] {
                self.refresh_model_texture(pix, id)?;
            }
        }
        Ok(())
    }

    /// Force each sampled Java-animated layer to the calling slot's current
    /// Pix8 phase. The caller serializes these writes through its scene
    /// submission so another slot cannot replace them before sampling.
    pub(crate) fn stage_animated_model_textures(
        &mut self,
        pix: &Pix3DDraw,
        sampled: &[bool; 50],
    ) -> Result<(), RenderLimit> {
        for id in [17, 24] {
            if sampled[id] {
                self.refresh_model_texture(pix, id)?;
            }
        }
        Ok(())
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
            eprintln!("GPU unavailable (no adapter or SKIP_GPU=1); skipping");
            return;
        };
        let assets = GpuAssets::new(&device, &queue).unwrap();
        assert_eq!(
            assets.model_atlas.mip_level_count(),
            MODEL_MIP_LEVELS,
            "RuneLite TextureManager allocates 8 mip levels on the 128px array"
        );
    }

    fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        // Headless CI sets this; match `GpuBackend::try_new` without probing an adapter.
        if std::env::var("SKIP_GPU").ok().as_deref() == Some("1") {
            return None;
        }
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
            eprintln!("GPU unavailable (no adapter or SKIP_GPU=1); skipping");
            return;
        };
        let mut atlas = GpuAtlas::new(&device, "test", (8, 8), 16, 0).unwrap();
        let mut count = 0;
        // 4×4 regions: 2 per shelf, 8 per 8×16 layer → the 9th returns None.
        while atlas.alloc(&device, &queue, 4, 4).unwrap().is_some() {
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
            eprintln!("GPU unavailable (no adapter or SKIP_GPU=1); skipping");
            return;
        };
        let mut atlas = LayerAtlas::new(&device, "test", (8, 8), 16).unwrap();
        // A free list empty and every layer 8×16 full: 8 regions per layer,
        // so the 8×8+1th alloc returns None, not a panic.
        let mut count = 0;
        while atlas.alloc(&device, &queue, 4, 4, 1).unwrap().is_some() {
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
            eprintln!("GPU unavailable (no adapter or SKIP_GPU=1); skipping");
            return;
        };
        let mut atlas = LayerAtlas::new(&device, "test", (8, 8), 16).unwrap();
        let first = atlas.alloc(&device, &queue, 4, 4, 1).unwrap().unwrap();
        assert_eq!(atlas.layer_count(), 1);
        // Freed at epoch 1: not reusable at epoch 1 (same frame)...
        atlas.free_region(first, 1);
        let second = atlas.alloc(&device, &queue, 4, 4, 1).unwrap().unwrap();
        assert_ne!(
            second, first,
            "a region freed this frame is not reused this frame"
        );
        assert_eq!(atlas.layer_count(), 1);
        // ...but reusable from epoch 2 on (the freeing frame is flushed).
        atlas.free_region(second, 1);
        let third = atlas.alloc(&device, &queue, 4, 4, 2).unwrap().unwrap();
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
            eprintln!("GPU unavailable (no adapter or SKIP_GPU=1); skipping");
            return;
        };
        let mut assets = GpuAssets::new(&device, &queue).unwrap();
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
                    assets.sprite_region_pix8(sprite, epoch).unwrap().is_some(),
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
