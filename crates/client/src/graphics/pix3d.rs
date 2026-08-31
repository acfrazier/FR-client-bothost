// Port of `~/experiments/Server/webclient/src/dash3d/Pix3D.ts`.
//
// The trig/divide tables and the 65536-entry colour table are immutable after
// init, so they are process-wide `OnceLock`s (design: "Immutable tables ...
// process-wide OnceLock"). Per-frame draw state that was static on Pix3D
// (`scanline`, `originX/Y`, `trans`, `cycle`, the texture pool) lives on
// `Pix3DDraw`, owned per-client. Raster methods target a `&mut Pix2D`
// surface (TS binds a target with `Pix2D.setPixels`; the TS Pix3D statics
// `pixels`/`width`/`clipMaxY`/`sizeX` become the surface's fields).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use super::pix2d::Pix2D;
use super::pix8::Pix8;
use crate::io::JagFile;

pub struct Pix3D;

struct TrigTables {
    div_table: [i32; 512],
    div_table2: [i32; 2048],
    sin_table: [i32; 2048],
    cos_table: [i32; 2048],
}

static TRIG: OnceLock<TrigTables> = OnceLock::new();
/// One table per options-panel brightness (0.9/0.8/0.7/0.6). Java rebuilds
/// the table on each slider click; a single OnceLock made the slider a no-op.
static COLOUR_TABLES: [OnceLock<Box<[i32; 65536]>>; 4] = [
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
];
static COLOUR_TABLE_SLOT: AtomicUsize = AtomicUsize::new(1);

fn brightness_slot(brightness: f64) -> usize {
    if (brightness - 0.9).abs() < 0.05 {
        0
    } else if (brightness - 0.8).abs() < 0.05 {
        1
    } else if (brightness - 0.7).abs() < 0.05 {
        2
    } else {
        3
    }
}

fn slot_brightness(slot: usize) -> f64 {
    match slot {
        0 => 0.9,
        1 => 0.8,
        2 => 0.7,
        _ => 0.6,
    }
}

fn trig() -> &'static TrigTables {
    TRIG.get_or_init(|| {
        let mut div_table = [0i32; 512];
        for i in 1..512 {
            div_table[i] = 32768 / i as i32;
        }
        let mut div_table2 = [0i32; 2048];
        for i in 1..2048 {
            div_table2[i] = 65536 / i as i32;
        }
        let mut sin_table = [0i32; 2048];
        let mut cos_table = [0i32; 2048];
        for i in 0..2048 {
            // angular frequency: 2 * pi / 2048 = 0.0030679615757712823
            // * 65536 = maximum amplitude
            sin_table[i] = (f64::sin(i as f64 * 0.0030679615757712823) * 65536.0) as i32;
            cos_table[i] = (f64::cos(i as f64 * 0.0030679615757712823) * 65536.0) as i32;
        }
        TrigTables {
            div_table,
            div_table2,
            sin_table,
            cos_table,
        }
    })
}

impl Pix3D {
    pub fn div_table() -> &'static [i32; 512] {
        &trig().div_table
    }

    pub fn div_table2() -> &'static [i32; 2048] {
        &trig().div_table2
    }

    pub fn sin_table() -> &'static [i32; 2048] {
        &trig().sin_table
    }

    pub fn cos_table() -> &'static [i32; 2048] {
        &trig().cos_table
    }

    /// Builds the 65536-entry HSL→RGB colour table once (Java `Pix3D`
    /// allocates it with `new int[65536]`). The TS adds
    /// `Math.random() * 0.03 - 0.015` jitter to `brightness`; this port
    /// keeps the table deterministic (deviation documented). The table is
    /// heap-allocated like Java's `new int[65536]`, and `build_colour_table`
    /// fills it through a `&mut` slice: building an inline `[i32; 65536]`
    /// passed the 256 KB table by value several times through the `OnceLock`
    /// machinery (~1.9 MB of stack in debug), leaving `Client::new` within
    /// ~16 KB of the 2 MB test-thread stack.
    pub fn init_colour_table(brightness: f64) {
        let slot = brightness_slot(brightness);
        COLOUR_TABLES[slot].get_or_init(|| {
            let mut table: Box<[i32; 65536]> =
                vec![0i32; 65536].into_boxed_slice().try_into().unwrap();
            build_colour_table(&mut table[..], slot_brightness(slot));
            table
        });
        COLOUR_TABLE_SLOT.store(slot, Ordering::Relaxed);
    }

    pub fn colour_table() -> &'static [i32; 65536] {
        let slot = COLOUR_TABLE_SLOT.load(Ordering::Relaxed);
        COLOUR_TABLES[slot]
            .get()
            .or_else(|| COLOUR_TABLES[1].get())
            .expect("Pix3D::init_colour_table must be called before colour_table()")
            .as_ref()
    }

    /// The brightness (0.9/0.8/0.7/0.6) the current colour table was built
    /// at, so the GPU scene shader's `hslToRgb` applies the same gamma the
    /// CPU flat-face `colour_table()` baked in.
    pub fn colour_brightness() -> f64 {
        let slot = COLOUR_TABLE_SLOT.load(Ordering::Relaxed);
        slot_brightness(slot)
    }

    fn gamma_correct(rgb: i32, gamma: f64) -> i32 {
        let r = (rgb >> 16) as f64 / 256.0;
        let g = ((rgb >> 8) & 0xff) as f64 / 256.0;
        let b = (rgb & 0xff) as f64 / 256.0;

        let pow_r = r.powf(gamma);
        let pow_g = g.powf(gamma);
        let pow_b = b.powf(gamma);

        let int_r = (pow_r * 256.0) as i32;
        let int_g = (pow_g * 256.0) as i32;
        let int_b = (pow_b * 256.0) as i32;
        (int_r << 16) + (int_g << 8) + int_b
    }
}

fn build_colour_table(table: &mut [i32], brightness: f64) {
    let mut offset = 0;
    for y in 0..512 {
        let hue = ((y / 8) as f64) / 64.0 + 0.0078125;
        let saturation = ((y & 0x7) as f64) / 8.0 + 0.0625;
        for x in 0..128 {
            let lightness = x as f64 / 128.0;
            let mut r = lightness;
            let mut g = lightness;
            let mut b = lightness;

            if saturation != 0.0 {
                let q = if lightness < 0.5 {
                    lightness * (saturation + 1.0)
                } else {
                    lightness + saturation - lightness * saturation
                };
                let p = lightness * 2.0 - q;
                let mut t = hue + 0.3333333333333333;
                if t > 1.0 {
                    t -= 1.0;
                }
                let mut d11 = hue - 0.3333333333333333;
                if d11 < 0.0 {
                    d11 += 1.0;
                }

                if t * 6.0 < 1.0 {
                    r = p + (q - p) * 6.0 * t;
                } else if t * 2.0 < 1.0 {
                    r = q;
                } else if t * 3.0 < 2.0 {
                    r = p + (q - p) * (0.6666666666666666 - t) * 6.0;
                } else {
                    r = p;
                }

                if hue * 6.0 < 1.0 {
                    g = p + (q - p) * 6.0 * hue;
                } else if hue * 2.0 < 1.0 {
                    g = q;
                } else if hue * 3.0 < 2.0 {
                    g = p + (q - p) * (0.6666666666666666 - hue) * 6.0;
                } else {
                    g = p;
                }

                if d11 * 6.0 < 1.0 {
                    b = p + (q - p) * 6.0 * d11;
                } else if d11 * 2.0 < 1.0 {
                    b = q;
                } else if d11 * 3.0 < 2.0 {
                    b = p + (q - p) * (0.6666666666666666 - d11) * 6.0;
                } else {
                    b = p;
                }
            }

            let int_r = (r * 256.0) as i32;
            let int_g = (g * 256.0) as i32;
            let int_b = (b * 256.0) as i32;
            let rgb = (int_r << 16) + (int_g << 8) + int_b;
            table[offset] = Pix3D::gamma_correct(rgb, brightness);
            offset += 1;
        }
    }
}

/// The TS `Model` render statics (`vertexScreenX/Y/Z`, `vertexViewSpaceX/
/// Y/Z`, `faceNearClipped`/`faceClippedX`, the depth/priority buckets and
/// the near-clip output): the shared per-frame scratch `Model.worldRender`/
/// `objRender` project into. Per-client like the rest of `Pix3DDraw` (the TS
/// statics are process-wide; N clients need N copies). Sizes match the TS
/// typed arrays; out-of-range access (a model past the 4096-vertex arrays,
/// or a depth bucket past 1500) is a guarded no-op exactly like a TS
/// typed-array write.
pub struct ModelScratch {
    pub vertex_screen_x: Vec<i32>,
    pub vertex_screen_y: Vec<i32>,
    pub vertex_screen_z: Vec<i32>,
    pub vertex_view_space_x: Vec<i32>,
    pub vertex_view_space_y: Vec<i32>,
    pub vertex_view_space_z: Vec<i32>,
    pub face_near_clipped: Vec<bool>,
    pub face_clipped_x: Vec<bool>,
    pub tmp_depth_face_count: Vec<i32>,
    pub tmp_depth_faces: Vec<i32>,
    pub tmp_priority_face_count: Vec<i32>,
    pub tmp_priority_faces: Vec<i32>,
    pub tmp_priority10_face_depth: Vec<i32>,
    pub tmp_priority11_face_depth: Vec<i32>,
    pub tmp_priority_depth_sum: Vec<i32>,
    pub clipped_x: Vec<i32>,
    pub clipped_y: Vec<i32>,
    pub clipped_colour: Vec<i32>,
}

impl Default for ModelScratch {
    fn default() -> Self {
        ModelScratch {
            vertex_screen_x: vec![0; 4096],
            vertex_screen_y: vec![0; 4096],
            vertex_screen_z: vec![0; 4096],
            vertex_view_space_x: vec![0; 4096],
            vertex_view_space_y: vec![0; 4096],
            vertex_view_space_z: vec![0; 4096],
            face_near_clipped: vec![false; 4096],
            face_clipped_x: vec![false; 4096],
            tmp_depth_face_count: vec![0; 1500],
            tmp_depth_faces: vec![0; 1500 * 512],
            tmp_priority_face_count: vec![0; 12],
            tmp_priority_faces: vec![0; 12 * 2000],
            tmp_priority10_face_depth: vec![0; 2000],
            tmp_priority11_face_depth: vec![0; 2000],
            tmp_priority_depth_sum: vec![0; 12],
            clipped_x: vec![0; 10],
            clipped_y: vec![0; 10],
            clipped_colour: vec![0; 10],
        }
    }
}

/// Per-client raster state, the mutable half of the TS Pix3D statics
/// (`scanline`, `originX/Y`, `trans`, `cycle`, `hclip`, `lowMem`/
/// `lowDetail`, and the texture pool) plus the TS `Model` render statics
/// (the `ModelScratch` projection buffers and the `mouseCheck` pick state),
/// which the 3D pass reads and fills. One instance per `Client`; a raster
/// pass binds a target surface (`Pix2D`) and calls `set_clipping` (or
/// `set_render_clipping`) before drawing into it, exactly as TS binds
/// `Pix2D.setPixels` then `setClipping`/`setRenderClipping`.
pub struct Pix3DDraw {
    /// Row offset of each scanline, `scanline[y] = width * y` (TS `scanline`).
    pub scanline: Vec<i32>,
    /// Screen origin, `(width / 2, height / 2)` of the current target.
    pub origin_x: i32,
    pub origin_y: i32,
    /// `trans` from TS: 0 opaque, otherwise 256 - alpha for the raster spans.
    pub trans: i32,
    /// Texture-use counter (`cycle`); `get_texels` stamps `tex_cycle` with it.
    pub cycle: i32,
    /// `hclip`: when true the raster spans clip x to the surface `size_x`
    /// instead of using the div-table gradient (set per face by Model/World).
    pub hclip: bool,
    /// `lowMem` from TS (client config): 128×128 textures halved, 4096-texel
    /// pool rows, coarser texture interpolation.
    pub low_mem: bool,
    /// `lowDetail` from TS (default true; ObjType turns it off while drawing
    /// 3D model sprites): gouraud raster packs 4 pixels per step.
    pub low_detail: bool,
    /// `textures` from TS: the 50 depacked `Pix8` textures.
    pub textures: Vec<Option<Pix8>>,
    /// `texTrans` from TS (private): whether each texture has a transparent
    /// (0) texel; drives the non-opaque texture raster.
    tex_trans: [bool; 50],
    /// `texAverage` from TS (private): cached `getTextureAverage` results.
    /// `pub` so the renderer can mirror it onto `Client` for the sim's
    /// `finish_build` overlay read.
    pub tex_average: [i32; 50],
    /// `activeTexels` from TS: the current unpacked texel rows per texture.
    pub active_texels: Vec<Option<Vec<i32>>>,
    /// `texCycle` from TS: last `cycle` stamp per texture (LRU eviction).
    pub tex_cycle: [i32; 50],
    /// `texPal` from TS: the gamma-corrected texture palettes.
    pub tex_pal: Vec<Option<Vec<i32>>>,
    /// `numTextures` from TS: count of successfully depacked textures.
    pub num_textures: i32,
    /// `texelPool` from TS: pooled texel rows (16384 long in lowMem,
    /// 65536 otherwise), used as a stack of free rows.
    pub texel_pool: Option<Vec<Vec<i32>>>,
    /// `poolSize` from TS: number of free rows left in `texel_pool`.
    pub pool_size: i32,
    /// `opaque` from TS (private): whether the current texture raster writes
    /// every pixel (vs. skipping transparent ones).
    opaque: bool,
    /// TS `Model.mouseCheck`/`mouseX`/`mouseY`/`pickedCount`/
    /// `pickedEntityTypecode`: the mouse-picking state `world_render` reads
    /// and fills. `gameDrawMain` sets `mouse_check=true`, zeroes
    /// `picked_count` and stores the viewport mouse before `render_all`.
    pub mouse_check: bool,
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub picked_count: i32,
    pub picked_entity_typecode: Vec<i32>,
    /// TS `Model` render statics: the per-frame projection/depth scratch
    /// shared by `Model.worldRender`/`objRender`.
    pub model_scratch: ModelScratch,
}

impl Default for Pix3DDraw {
    fn default() -> Self {
        Pix3DDraw {
            scanline: Vec::new(),
            origin_x: 0,
            origin_y: 0,
            trans: 0,
            cycle: 0,
            hclip: false,
            low_mem: false,
            low_detail: true,
            textures: vec![None; 50],
            tex_trans: [false; 50],
            tex_average: [0; 50],
            active_texels: vec![None; 50],
            tex_cycle: [0; 50],
            tex_pal: vec![None; 50],
            num_textures: 0,
            texel_pool: None,
            pool_size: 0,
            opaque: false,
            mouse_check: false,
            mouse_x: 0,
            mouse_y: 0,
            picked_count: 0,
            picked_entity_typecode: vec![0; 1000],
            model_scratch: ModelScratch::default(),
        }
    }
}

impl Pix3DDraw {
    /// TS `setRenderClipping`: bind clipping to the current target surface,
    /// rebuilding `scanline` and the origin for its size.
    pub fn set_render_clipping(&mut self, surface: &Pix2D) {
        self.set_clipping(surface.width, surface.height);
    }

    /// TS `setClipping(width, height)`: rebuild the row-offset `scanline`
    /// and the screen origin for a new 3D target size.
    pub fn set_clipping(&mut self, width: i32, height: i32) {
        self.scanline = (0..height).map(|y| width * y).collect();
        self.origin_x = width / 2;
        self.origin_y = height / 2;
    }

    /// TS `clearTexels`: drop the texel pool and every active row.
    pub fn clear_texels(&mut self) {
        self.texel_pool = None;
        self.active_texels.fill(None);
    }

    /// TS `initPool(size)`: allocate `size` texel rows (16384 in lowMem,
    /// 65536 otherwise). A no-op once the pool exists.
    pub fn init_pool(&mut self, size: i32) {
        if self.texel_pool.is_some() {
            return;
        }
        self.pool_size = size;
        let row_len = if self.low_mem { 16384 } else { 65536 };
        self.texel_pool = Some(vec![vec![0; row_len]; size as usize]);
        self.active_texels.fill(None);
    }

    /// Re-size the texel pool for a live lowmem/highmem flip. `init_pool`
    /// is one-shot by design, so the live toggle needs a fresh pool: the
    /// high-mem raster writes four 16384-texel blocks (65536 total) and a
    /// low-mem-sized row crashes with `index 16384` on the second block.
    pub fn reset_pool(&mut self, size: i32) {
        self.texel_pool = None;
        self.pool_size = 0;
        self.active_texels.fill(None);
        self.init_pool(size);
    }

    /// TS `unpackTextures`: depack textures 0..49 from the `textures` jag.
    /// In lowMem, 128×128 textures are halved first; otherwise they are
    /// trimmed back to their original size. Failed depacks are skipped.
    pub fn unpack_textures(&mut self, textures: &JagFile) {
        self.num_textures = 0;
        for id in 0..50 {
            if let Ok(mut texture) = Pix8::depack(textures, &id.to_string(), 0) {
                if self.low_mem && texture.owi == 128 {
                    texture.halve_size();
                } else {
                    texture.trim();
                }
                self.textures[id] = Some(texture);
                self.num_textures += 1;
            }
        }
    }

    /// The texture-palette half of TS `initColourTable` (the colour table
    /// itself is the process-wide `Pix3D::init_colour_table`): gamma-correct
    /// every texture's `bpal` into `tex_pal` at the same `brightness`, then
    /// return every texture to the pool. The TS brightness jitter is dropped,
    /// matching `Pix3D`.
    pub fn init_texture_palettes(&mut self, brightness: f64) {
        for id in 0..50 {
            let Some(texture) = &self.textures[id] else {
                continue;
            };
            let mut pal = vec![0i32; texture.bpal.len()];
            for i in 0..texture.bpal.len() {
                pal[i] = Pix3D::gamma_correct(texture.bpal[i], brightness);
            }
            self.tex_pal[id] = Some(pal);
        }
        for id in 0..50 {
            self.push_texture(id);
        }
    }

    /// TS `getTextureAverage`: average of the gamma-corrected palette
    /// entries, re-gamma'd at 1.4, cached in `tex_average`. 0 for textures
    /// without a palette (and for missing ids, where TS returns undefined).
    pub fn get_texture_average(&mut self, id: i32) -> i32 {
        if id < 0 || id as usize >= self.tex_average.len() {
            return 0;
        }
        let id = id as usize;
        if self.tex_average[id] != 0 {
            return self.tex_average[id];
        }
        let Some(palette) = self.tex_pal[id].as_ref() else {
            return 0;
        };
        if palette.is_empty() {
            // TS: 0/0 → NaN → `| 0` → 0, then the `rgb === 0` bump → 1.
            self.tex_average[id] = 1;
            return 1;
        }
        let mut r = 0;
        let mut g = 0;
        let mut b = 0;
        for &rgb in palette {
            r += (rgb >> 16) & 0xff;
            g += (rgb >> 8) & 0xff;
            b += rgb & 0xff;
        }
        let length = palette.len() as i32;
        let mut rgb = ((r / length) << 16) + ((g / length) << 8) + (b / length);
        rgb = Pix3D::gamma_correct(rgb, 1.4);
        if rgb == 0 {
            rgb = 1;
        }
        self.tex_average[id] = rgb;
        rgb
    }

    /// Force-compute every `getTextureAverage` into `tex_average`, for the
    /// sim's scene build (`finish_build` reads the overlay through the
    /// `Client` mirror the renderer copies to).
    pub fn refresh_texture_averages(&mut self) {
        for id in 0..50 {
            self.get_texture_average(id);
        }
    }

    /// Process-wide texture averages for `finish_build`, keyed by cache
    /// dir + lowmem (halving 128×128 textures changes the average).
    /// Unheaded slots never run `prepare_game`; they still need this
    /// before the first `map_build` or textured overlay rgb is 0.
    pub fn cached_averages(cache_dir: &str, low_mem: bool) -> [i32; 50] {
        static CELL: OnceLock<Mutex<Option<(String, bool, [i32; 50])>>> = OnceLock::new();
        let cell = CELL.get_or_init(|| Mutex::new(None));
        let mut guard = cell.lock().unwrap();
        if let Some((dir, mem, avg)) = &*guard {
            if dir == cache_dir && *mem == low_mem {
                return *avg;
            }
        }
        let mut pix = Pix3DDraw::default();
        pix.low_mem = low_mem;
        if let Ok(bytes) = std::fs::read(format!("{cache_dir}/textures")) {
            pix.unpack_textures(&JagFile::new(bytes));
            pix.init_texture_palettes(0.8);
            pix.refresh_texture_averages();
        }
        let avg = pix.tex_average;
        *guard = Some((cache_dir.to_string(), low_mem, avg));
        avg
    }

    /// TS `pushTexture`: return `id`'s active texel row to the pool.
    pub fn push_texture(&mut self, id: i32) {
        if id < 0 || id as usize >= self.active_texels.len() {
            return;
        }
        let id = id as usize;
        if self.active_texels[id].is_some() && self.texel_pool.is_some() {
            let row = self.active_texels[id].take().unwrap();
            let pool = self.texel_pool.as_mut().unwrap();
            // TS writes `texelPool[poolSize++]` and ignores an out-of-bounds
            // write while still bumping `poolSize`; guard the slot the same
            // way (the invariant keeps poolSize within the pool anyway).
            if let Some(slot) = pool.get_mut(self.pool_size as usize) {
                *slot = row;
            }
            self.pool_size += 1;
        }
    }

    /// TS `getTexels`: the unpacked texel row for `id`, built from the
    /// texture on first use this cycle (pool pop or LRU eviction). The TS
    /// keeps the row in `activeTexels` and hands out a reference; here the
    /// caller (`texture_triangle`) owns the row for the span of the call and
    /// puts it back into `active_texels[id]` when done — behaviourally
    /// identical because nothing else touches the row mid-triangle. `opaque`
    /// is set here too: TS sets it in `textureTriangle` immediately after
    /// `getTexels`, and `getTexels` is only called from there, so folding it
    /// in is equivalent. Returns `None` for ids without a texture/palette.
    fn get_texels(&mut self, id: usize) -> Option<Vec<i32>> {
        if id >= 50 {
            return None;
        }
        self.tex_cycle[id] = self.cycle;
        self.cycle += 1;
        if self.active_texels[id].is_some() {
            self.opaque = !self.tex_trans[id];
            return self.active_texels[id].take();
        }

        let mut texels: Option<Vec<i32>> = None;
        if self.pool_size > 0 {
            if let Some(pool) = self.texel_pool.as_mut() {
                self.pool_size -= 1;
                // `take` leaves an empty Vec in the slot; the row is
                // returned to the same slot by `push_texture`.
                texels = Some(std::mem::take(&mut pool[self.pool_size as usize]));
            }
        }
        if texels.is_none() {
            // Pool empty (or not initialised): evict the least-recently-used
            // active texture.
            let mut oldest = 0;
            let mut selected = -1;
            for t in 0..self.num_textures {
                if self.active_texels[t as usize].is_some()
                    && (self.tex_cycle[t as usize] < oldest || selected == -1)
                {
                    oldest = self.tex_cycle[t as usize];
                    selected = t;
                }
            }
            if selected != -1 {
                texels = self.active_texels[selected as usize].take();
            }
        }
        let Some(mut texels) = texels else {
            return None;
        };

        if self.textures[id].is_none() || self.tex_pal[id].is_none() {
            // TS leaves the unfilled row in `activeTexels` and returns null.
            self.active_texels[id] = Some(texels);
            return None;
        }
        let texture = self.textures[id].as_ref().unwrap();
        let palette = self.tex_pal[id].as_ref().unwrap();

        if self.low_mem {
            self.tex_trans[id] = false;
            for i in 0..4096 {
                let data = texture.data.get(i).copied().unwrap_or(0);
                let rgb = Self::palette_lookup(palette, data) & 0xf8f8ff;
                texels[i] = rgb;
                if rgb == 0 {
                    self.tex_trans[id] = true;
                }
                texels[i + 4096] = (rgb - (rgb >> 3)) & 0xf8f8ff;
                texels[i + 8192] = (rgb - (rgb >> 2)) & 0xf8f8ff;
                texels[i + 12288] = (rgb - (rgb >> 2) - (rgb >> 3)) & 0xf8f8ff;
            }
        } else {
            if texture.wi == 64 {
                for y in 0..128 {
                    for x in 0..128 {
                        let data = texture
                            .data
                            .get((x >> 1) + ((y >> 1) << 6))
                            .copied()
                            .unwrap_or(0);
                        texels[(x + (y << 7)) as usize] = Self::palette_lookup(palette, data);
                    }
                }
            } else {
                for i in 0..16384 {
                    let data = texture.data.get(i).copied().unwrap_or(0);
                    texels[i] = Self::palette_lookup(palette, data);
                }
            }
            self.tex_trans[id] = false;
            for i in 0..16384 {
                texels[i] &= 0xf8f8ff;
                let rgb = texels[i];
                if rgb == 0 {
                    self.tex_trans[id] = true;
                }
                texels[i + 16384] = (rgb - (rgb >> 3)) & 0xf8f8ff;
                texels[i + 32768] = (rgb - (rgb >> 2)) & 0xf8f8ff;
                texels[i + 49152] = (rgb - (rgb >> 2) - (rgb >> 3)) & 0xf8f8ff;
            }
        }
        self.opaque = !self.tex_trans[id];
        Some(texels)
    }

    /// Palette lookup following TS typed-array semantics: an index outside
    /// the palette (negative `i8`, or past the end) is `undefined` → 0.
    fn palette_lookup(palette: &[i32], data: i8) -> i32 {
        let idx = data as i32;
        if idx < 0 {
            0
        } else {
            palette.get(idx as usize).copied().unwrap_or(0)
        }
    }

    /// TS typed-array pixel read: an out-of-bounds offset (including a
    /// negative `off`) is `undefined` → 0, a silent no-op.
    #[inline]
    fn pixel(surface: &Pix2D, off: i32) -> i32 {
        surface.pixels.get(off as usize).copied().unwrap_or(0)
    }

    /// TS typed-array pixel write: an out-of-bounds offset (including a
    /// negative `off`) is silently ignored, exactly like `dst[off] = rgb`.
    #[inline]
    fn put_pixel(surface: &mut Pix2D, off: i32, rgb: i32) {
        if let Some(p) = surface.pixels.get_mut(off as usize) {
            *p = rgb;
            // GPU overlay coverage: TYPE_MODEL rasters through Pix3D, not
            // Pix2D fill/plot. Unmarked pixels are a 3D hole in the scene
            // window (the mysterious-cube random event).
            surface.mark_pixel(off);
        }
    }

    /// TS `gouraudTriangle`: shaded triangle into `surface`, clipping against
    /// `surface.clip_max_y` and (with `hclip`) `surface.size_x`. Colours are
    /// 16-bit shade indices into the colour table.
    #[allow(clippy::too_many_arguments)]
    pub fn gouraud_triangle(
        &mut self,
        surface: &mut Pix2D,
        x_a: i32,
        x_b: i32,
        x_c: i32,
        y_a: i32,
        y_b: i32,
        y_c: i32,
        colour_a: i32,
        colour_b: i32,
        colour_c: i32,
    ) {
        debug_assert!(
            !self.scanline.is_empty(),
            "set_clipping/set_render_clipping must be called before rasterising"
        );
        let (mut x_a, mut x_b, mut x_c) = (x_a, x_b, x_c);
        let (mut y_a, mut y_b, mut y_c) = (y_a, y_b, y_c);
        let (mut colour_a, mut colour_b, mut colour_c) = (colour_a, colour_b, colour_c);

        let mut x_step_ab = 0;
        let mut colour_step_ab = 0;
        if y_b != y_a {
            x_step_ab = (x_b - x_a).wrapping_shl(16).wrapping_div(y_b - y_a);
            colour_step_ab = (colour_b - colour_a)
                .wrapping_shl(15)
                .wrapping_div(y_b - y_a);
        }

        let mut x_step_bc = 0;
        let mut colour_step_bc = 0;
        if y_c != y_b {
            x_step_bc = (x_c - x_b).wrapping_shl(16).wrapping_div(y_c - y_b);
            colour_step_bc = (colour_c - colour_b)
                .wrapping_shl(15)
                .wrapping_div(y_c - y_b);
        }

        let mut x_step_ac = 0;
        let mut colour_step_ac = 0;
        if y_c != y_a {
            x_step_ac = (x_a - x_c).wrapping_shl(16).wrapping_div(y_a - y_c);
            colour_step_ac = (colour_a - colour_c)
                .wrapping_shl(15)
                .wrapping_div(y_a - y_c);
        }

        if y_a <= y_b && y_a <= y_c {
            if y_a >= surface.clip_max_y {
                return;
            }
            if y_b > surface.clip_max_y {
                y_b = surface.clip_max_y;
            }
            if y_c > surface.clip_max_y {
                y_c = surface.clip_max_y;
            }
            if y_b < y_c {
                x_c = x_a.wrapping_shl(16);
                x_a = x_a.wrapping_shl(16);
                colour_c = colour_a.wrapping_shl(15);
                colour_a = colour_a.wrapping_shl(15);
                if y_a < 0 {
                    x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                    x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                    colour_c = colour_c.wrapping_sub(colour_step_ac.wrapping_mul(y_a));
                    colour_a = colour_a.wrapping_sub(colour_step_ab.wrapping_mul(y_a));
                    y_a = 0;
                }
                x_b = x_b.wrapping_shl(16);
                colour_b = colour_b.wrapping_shl(15);
                if y_b < 0 {
                    x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                    colour_b = colour_b.wrapping_sub(colour_step_bc.wrapping_mul(y_b));
                    y_b = 0;
                }
                if (y_a != y_b && x_step_ac < x_step_ab) || (y_a == y_b && x_step_ac > x_step_bc) {
                    y_c -= y_b;
                    y_b -= y_a;
                    y_a = self.scanline[y_a as usize];
                    'outer: loop {
                        y_b -= 1;
                        if y_b < 0 {
                            loop {
                                y_c -= 1;
                                if y_c < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_c >> 16,
                                    x_b >> 16,
                                    colour_c >> 7,
                                    colour_b >> 7,
                                    y_a,
                                );
                                x_c = x_c.wrapping_add(x_step_ac);
                                x_b = x_b.wrapping_add(x_step_bc);
                                colour_c = colour_c.wrapping_add(colour_step_ac);
                                colour_b = colour_b.wrapping_add(colour_step_bc);
                                y_a = y_a.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_c >> 16,
                            x_a >> 16,
                            colour_c >> 7,
                            colour_a >> 7,
                            y_a,
                        );
                        x_c = x_c.wrapping_add(x_step_ac);
                        x_a = x_a.wrapping_add(x_step_ab);
                        colour_c = colour_c.wrapping_add(colour_step_ac);
                        colour_a = colour_a.wrapping_add(colour_step_ab);
                        y_a = y_a.wrapping_add(surface.width);
                    }
                } else {
                    y_c -= y_b;
                    y_b -= y_a;
                    y_a = self.scanline[y_a as usize];
                    'outer: loop {
                        y_b -= 1;
                        if y_b < 0 {
                            loop {
                                y_c -= 1;
                                if y_c < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_b >> 16,
                                    x_c >> 16,
                                    colour_b >> 7,
                                    colour_c >> 7,
                                    y_a,
                                );
                                x_c = x_c.wrapping_add(x_step_ac);
                                x_b = x_b.wrapping_add(x_step_bc);
                                colour_c = colour_c.wrapping_add(colour_step_ac);
                                colour_b = colour_b.wrapping_add(colour_step_bc);
                                y_a = y_a.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_a >> 16,
                            x_c >> 16,
                            colour_a >> 7,
                            colour_c >> 7,
                            y_a,
                        );
                        x_c = x_c.wrapping_add(x_step_ac);
                        x_a = x_a.wrapping_add(x_step_ab);
                        colour_c = colour_c.wrapping_add(colour_step_ac);
                        colour_a = colour_a.wrapping_add(colour_step_ab);
                        y_a = y_a.wrapping_add(surface.width);
                    }
                }
            } else {
                x_b = x_a.wrapping_shl(16);
                x_a = x_a.wrapping_shl(16);
                colour_b = colour_a.wrapping_shl(15);
                colour_a = colour_a.wrapping_shl(15);
                if y_a < 0 {
                    x_b = x_b.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                    x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                    colour_b = colour_b.wrapping_sub(colour_step_ac.wrapping_mul(y_a));
                    colour_a = colour_a.wrapping_sub(colour_step_ab.wrapping_mul(y_a));
                    y_a = 0;
                }
                x_c = x_c.wrapping_shl(16);
                colour_c = colour_c.wrapping_shl(15);
                if y_c < 0 {
                    x_c = x_c.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                    colour_c = colour_c.wrapping_sub(colour_step_bc.wrapping_mul(y_c));
                    y_c = 0;
                }
                if (y_a != y_c && x_step_ac < x_step_ab) || (y_a == y_c && x_step_bc > x_step_ab) {
                    y_b -= y_c;
                    y_c -= y_a;
                    y_a = self.scanline[y_a as usize];
                    'outer: loop {
                        y_c -= 1;
                        if y_c < 0 {
                            loop {
                                y_b -= 1;
                                if y_b < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_c >> 16,
                                    x_a >> 16,
                                    colour_c >> 7,
                                    colour_a >> 7,
                                    y_a,
                                );
                                x_c = x_c.wrapping_add(x_step_bc);
                                x_a = x_a.wrapping_add(x_step_ab);
                                colour_c = colour_c.wrapping_add(colour_step_bc);
                                colour_a = colour_a.wrapping_add(colour_step_ab);
                                y_a = y_a.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_b >> 16,
                            x_a >> 16,
                            colour_b >> 7,
                            colour_a >> 7,
                            y_a,
                        );
                        x_b = x_b.wrapping_add(x_step_ac);
                        x_a = x_a.wrapping_add(x_step_ab);
                        colour_b = colour_b.wrapping_add(colour_step_ac);
                        colour_a = colour_a.wrapping_add(colour_step_ab);
                        y_a = y_a.wrapping_add(surface.width);
                    }
                } else {
                    y_b -= y_c;
                    y_c -= y_a;
                    y_a = self.scanline[y_a as usize];
                    'outer: loop {
                        y_c -= 1;
                        if y_c < 0 {
                            loop {
                                y_b -= 1;
                                if y_b < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_a >> 16,
                                    x_c >> 16,
                                    colour_a >> 7,
                                    colour_c >> 7,
                                    y_a,
                                );
                                x_c = x_c.wrapping_add(x_step_bc);
                                x_a = x_a.wrapping_add(x_step_ab);
                                colour_c = colour_c.wrapping_add(colour_step_bc);
                                colour_a = colour_a.wrapping_add(colour_step_ab);
                                y_a = y_a.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_a >> 16,
                            x_b >> 16,
                            colour_a >> 7,
                            colour_b >> 7,
                            y_a,
                        );
                        x_b = x_b.wrapping_add(x_step_ac);
                        x_a = x_a.wrapping_add(x_step_ab);
                        colour_b = colour_b.wrapping_add(colour_step_ac);
                        colour_a = colour_a.wrapping_add(colour_step_ab);
                        y_a = y_a.wrapping_add(surface.width);
                    }
                }
            }
        } else if y_b <= y_c {
            if y_b >= surface.clip_max_y {
                return;
            }
            if y_c > surface.clip_max_y {
                y_c = surface.clip_max_y;
            }
            if y_a > surface.clip_max_y {
                y_a = surface.clip_max_y;
            }
            if y_c < y_a {
                x_a = x_b.wrapping_shl(16);
                x_b = x_b.wrapping_shl(16);
                colour_a = colour_b.wrapping_shl(15);
                colour_b = colour_b.wrapping_shl(15);
                if y_b < 0 {
                    x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                    x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                    colour_a = colour_a.wrapping_sub(colour_step_ab.wrapping_mul(y_b));
                    colour_b = colour_b.wrapping_sub(colour_step_bc.wrapping_mul(y_b));
                    y_b = 0;
                }
                x_c = x_c.wrapping_shl(16);
                colour_c = colour_c.wrapping_shl(15);
                if y_c < 0 {
                    x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                    colour_c = colour_c.wrapping_sub(colour_step_ac.wrapping_mul(y_c));
                    y_c = 0;
                }
                if (y_b != y_c && x_step_ab < x_step_bc) || (y_b == y_c && x_step_ab > x_step_ac) {
                    y_a -= y_c;
                    y_c -= y_b;
                    y_b = self.scanline[y_b as usize];
                    'outer: loop {
                        y_c -= 1;
                        if y_c < 0 {
                            loop {
                                y_a -= 1;
                                if y_a < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_a >> 16,
                                    x_c >> 16,
                                    colour_a >> 7,
                                    colour_c >> 7,
                                    y_b,
                                );
                                x_a = x_a.wrapping_add(x_step_ab);
                                x_c = x_c.wrapping_add(x_step_ac);
                                colour_a = colour_a.wrapping_add(colour_step_ab);
                                colour_c = colour_c.wrapping_add(colour_step_ac);
                                y_b = y_b.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_a >> 16,
                            x_b >> 16,
                            colour_a >> 7,
                            colour_b >> 7,
                            y_b,
                        );
                        x_a = x_a.wrapping_add(x_step_ab);
                        x_b = x_b.wrapping_add(x_step_bc);
                        colour_a = colour_a.wrapping_add(colour_step_ab);
                        colour_b = colour_b.wrapping_add(colour_step_bc);
                        y_b = y_b.wrapping_add(surface.width);
                    }
                } else {
                    y_a -= y_c;
                    y_c -= y_b;
                    y_b = self.scanline[y_b as usize];
                    'outer: loop {
                        y_c -= 1;
                        if y_c < 0 {
                            loop {
                                y_a -= 1;
                                if y_a < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_c >> 16,
                                    x_a >> 16,
                                    colour_c >> 7,
                                    colour_a >> 7,
                                    y_b,
                                );
                                x_a = x_a.wrapping_add(x_step_ab);
                                x_c = x_c.wrapping_add(x_step_ac);
                                colour_a = colour_a.wrapping_add(colour_step_ab);
                                colour_c = colour_c.wrapping_add(colour_step_ac);
                                y_b = y_b.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_b >> 16,
                            x_a >> 16,
                            colour_b >> 7,
                            colour_a >> 7,
                            y_b,
                        );
                        x_a = x_a.wrapping_add(x_step_ab);
                        x_b = x_b.wrapping_add(x_step_bc);
                        colour_a = colour_a.wrapping_add(colour_step_ab);
                        colour_b = colour_b.wrapping_add(colour_step_bc);
                        y_b = y_b.wrapping_add(surface.width);
                    }
                }
            } else {
                x_c = x_b.wrapping_shl(16);
                x_b = x_b.wrapping_shl(16);
                colour_c = colour_b.wrapping_shl(15);
                colour_b = colour_b.wrapping_shl(15);
                if y_b < 0 {
                    x_c = x_c.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                    x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                    colour_c = colour_c.wrapping_sub(colour_step_ab.wrapping_mul(y_b));
                    colour_b = colour_b.wrapping_sub(colour_step_bc.wrapping_mul(y_b));
                    y_b = 0;
                }
                x_a = x_a.wrapping_shl(16);
                colour_a = colour_a.wrapping_shl(15);
                if y_a < 0 {
                    x_a = x_a.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                    colour_a = colour_a.wrapping_sub(colour_step_ac.wrapping_mul(y_a));
                    y_a = 0;
                }
                y_c -= y_a;
                y_a -= y_b;
                y_b = self.scanline[y_b as usize];
                if x_step_ab < x_step_bc {
                    'outer: loop {
                        y_a -= 1;
                        if y_a < 0 {
                            loop {
                                y_c -= 1;
                                if y_c < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_a >> 16,
                                    x_b >> 16,
                                    colour_a >> 7,
                                    colour_b >> 7,
                                    y_b,
                                );
                                x_a = x_a.wrapping_add(x_step_ac);
                                x_b = x_b.wrapping_add(x_step_bc);
                                colour_a = colour_a.wrapping_add(colour_step_ac);
                                colour_b = colour_b.wrapping_add(colour_step_bc);
                                y_b = y_b.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_c >> 16,
                            x_b >> 16,
                            colour_c >> 7,
                            colour_b >> 7,
                            y_b,
                        );
                        x_c = x_c.wrapping_add(x_step_ab);
                        x_b = x_b.wrapping_add(x_step_bc);
                        colour_c = colour_c.wrapping_add(colour_step_ab);
                        colour_b = colour_b.wrapping_add(colour_step_bc);
                        y_b = y_b.wrapping_add(surface.width);
                    }
                } else {
                    'outer: loop {
                        y_a -= 1;
                        if y_a < 0 {
                            loop {
                                y_c -= 1;
                                if y_c < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_b >> 16,
                                    x_a >> 16,
                                    colour_b >> 7,
                                    colour_a >> 7,
                                    y_b,
                                );
                                x_a = x_a.wrapping_add(x_step_ac);
                                x_b = x_b.wrapping_add(x_step_bc);
                                colour_a = colour_a.wrapping_add(colour_step_ac);
                                colour_b = colour_b.wrapping_add(colour_step_bc);
                                y_b = y_b.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_b >> 16,
                            x_c >> 16,
                            colour_b >> 7,
                            colour_c >> 7,
                            y_b,
                        );
                        x_c = x_c.wrapping_add(x_step_ab);
                        x_b = x_b.wrapping_add(x_step_bc);
                        colour_c = colour_c.wrapping_add(colour_step_ab);
                        colour_b = colour_b.wrapping_add(colour_step_bc);
                        y_b = y_b.wrapping_add(surface.width);
                    }
                }
            }
        } else {
            if y_c >= surface.clip_max_y {
                return;
            }
            if y_a > surface.clip_max_y {
                y_a = surface.clip_max_y;
            }
            if y_b > surface.clip_max_y {
                y_b = surface.clip_max_y;
            }
            if y_a < y_b {
                x_b = x_c.wrapping_shl(16);
                x_c = x_c.wrapping_shl(16);
                colour_b = colour_c.wrapping_shl(15);
                colour_c = colour_c.wrapping_shl(15);
                if y_c < 0 {
                    x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                    x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                    colour_b = colour_b.wrapping_sub(colour_step_bc.wrapping_mul(y_c));
                    colour_c = colour_c.wrapping_sub(colour_step_ac.wrapping_mul(y_c));
                    y_c = 0;
                }
                x_a = x_a.wrapping_shl(16);
                colour_a = colour_a.wrapping_shl(15);
                if y_a < 0 {
                    x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                    colour_a = colour_a.wrapping_sub(colour_step_ab.wrapping_mul(y_a));
                    y_a = 0;
                }
                y_b -= y_a;
                y_a -= y_c;
                y_c = self.scanline[y_c as usize];
                if x_step_bc < x_step_ac {
                    'outer: loop {
                        y_a -= 1;
                        if y_a < 0 {
                            loop {
                                y_b -= 1;
                                if y_b < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_b >> 16,
                                    x_a >> 16,
                                    colour_b >> 7,
                                    colour_a >> 7,
                                    y_c,
                                );
                                x_b = x_b.wrapping_add(x_step_bc);
                                x_a = x_a.wrapping_add(x_step_ab);
                                colour_b = colour_b.wrapping_add(colour_step_bc);
                                colour_a = colour_a.wrapping_add(colour_step_ab);
                                y_c = y_c.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_b >> 16,
                            x_c >> 16,
                            colour_b >> 7,
                            colour_c >> 7,
                            y_c,
                        );
                        x_b = x_b.wrapping_add(x_step_bc);
                        x_c = x_c.wrapping_add(x_step_ac);
                        colour_b = colour_b.wrapping_add(colour_step_bc);
                        colour_c = colour_c.wrapping_add(colour_step_ac);
                        y_c = y_c.wrapping_add(surface.width);
                    }
                } else {
                    'outer: loop {
                        y_a -= 1;
                        if y_a < 0 {
                            loop {
                                y_b -= 1;
                                if y_b < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_a >> 16,
                                    x_b >> 16,
                                    colour_a >> 7,
                                    colour_b >> 7,
                                    y_c,
                                );
                                x_b = x_b.wrapping_add(x_step_bc);
                                x_a = x_a.wrapping_add(x_step_ab);
                                colour_b = colour_b.wrapping_add(colour_step_bc);
                                colour_a = colour_a.wrapping_add(colour_step_ab);
                                y_c = y_c.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_c >> 16,
                            x_b >> 16,
                            colour_c >> 7,
                            colour_b >> 7,
                            y_c,
                        );
                        x_b = x_b.wrapping_add(x_step_bc);
                        x_c = x_c.wrapping_add(x_step_ac);
                        colour_b = colour_b.wrapping_add(colour_step_bc);
                        colour_c = colour_c.wrapping_add(colour_step_ac);
                        y_c = y_c.wrapping_add(surface.width);
                    }
                }
            } else {
                x_a = x_c.wrapping_shl(16);
                x_c = x_c.wrapping_shl(16);
                colour_a = colour_c.wrapping_shl(15);
                colour_c = colour_c.wrapping_shl(15);
                if y_c < 0 {
                    x_a = x_a.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                    x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                    colour_a = colour_a.wrapping_sub(colour_step_bc.wrapping_mul(y_c));
                    colour_c = colour_c.wrapping_sub(colour_step_ac.wrapping_mul(y_c));
                    y_c = 0;
                }
                x_b = x_b.wrapping_shl(16);
                colour_b = colour_b.wrapping_shl(15);
                if y_b < 0 {
                    x_b = x_b.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                    colour_b = colour_b.wrapping_sub(colour_step_ab.wrapping_mul(y_b));
                    y_b = 0;
                }
                y_a -= y_b;
                y_b -= y_c;
                y_c = self.scanline[y_c as usize];
                if x_step_bc < x_step_ac {
                    'outer: loop {
                        y_b -= 1;
                        if y_b < 0 {
                            loop {
                                y_a -= 1;
                                if y_a < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_b >> 16,
                                    x_c >> 16,
                                    colour_b >> 7,
                                    colour_c >> 7,
                                    y_c,
                                );
                                x_b = x_b.wrapping_add(x_step_ab);
                                x_c = x_c.wrapping_add(x_step_ac);
                                colour_b = colour_b.wrapping_add(colour_step_ab);
                                colour_c = colour_c.wrapping_add(colour_step_ac);
                                y_c = y_c.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_a >> 16,
                            x_c >> 16,
                            colour_a >> 7,
                            colour_c >> 7,
                            y_c,
                        );
                        x_a = x_a.wrapping_add(x_step_bc);
                        x_c = x_c.wrapping_add(x_step_ac);
                        colour_a = colour_a.wrapping_add(colour_step_bc);
                        colour_c = colour_c.wrapping_add(colour_step_ac);
                        y_c = y_c.wrapping_add(surface.width);
                    }
                } else {
                    'outer: loop {
                        y_b -= 1;
                        if y_b < 0 {
                            loop {
                                y_a -= 1;
                                if y_a < 0 {
                                    break 'outer;
                                }
                                self.gouraud_raster(
                                    surface,
                                    x_c >> 16,
                                    x_b >> 16,
                                    colour_c >> 7,
                                    colour_b >> 7,
                                    y_c,
                                );
                                x_b = x_b.wrapping_add(x_step_ab);
                                x_c = x_c.wrapping_add(x_step_ac);
                                colour_b = colour_b.wrapping_add(colour_step_ab);
                                colour_c = colour_c.wrapping_add(colour_step_ac);
                                y_c = y_c.wrapping_add(surface.width);
                            }
                        }
                        self.gouraud_raster(
                            surface,
                            x_c >> 16,
                            x_a >> 16,
                            colour_c >> 7,
                            colour_a >> 7,
                            y_c,
                        );
                        x_a = x_a.wrapping_add(x_step_bc);
                        x_c = x_c.wrapping_add(x_step_ac);
                        colour_a = colour_a.wrapping_add(colour_step_bc);
                        colour_c = colour_c.wrapping_add(colour_step_ac);
                        y_c = y_c.wrapping_add(surface.width);
                    }
                }
            }
        }
    }

    /// TS `gouraudRaster`: a gouraud span of `surface.pixels` starting at
    /// `off`, `colourA/B` are shade values (the colour table is indexed by
    /// `colour >> 8`). The TS `len` parameter is dropped: it is always 0 and
    /// recomputed here.
    ///
    /// Writes go through `put_pixel`, so an out-of-bounds offset (negative
    /// `off`, or past the buffer) is a silent no-op like a TS typed-array
    /// write; `hclip` still clamps spans to `[0, size_x]` when set.
    fn gouraud_raster(
        &self,
        surface: &mut Pix2D,
        mut x_a: i32,
        mut x_b: i32,
        mut colour_a: i32,
        colour_b: i32,
        mut off: i32,
    ) {
        if self.low_detail {
            let mut colour_step;
            if self.hclip {
                if x_b - x_a > 3 {
                    colour_step = (colour_b - colour_a) / (x_b - x_a);
                } else {
                    colour_step = 0;
                }
                if x_b > surface.size_x {
                    x_b = surface.size_x;
                }
                if x_a < 0 {
                    colour_a = colour_a.wrapping_sub(x_a.wrapping_mul(colour_step));
                    x_a = 0;
                }
                if x_a >= x_b {
                    return;
                }
                off += x_a;
                let mut len = (x_b - x_a) >> 2;
                colour_step = colour_step.wrapping_shl(2);
                if self.trans == 0 {
                    loop {
                        len -= 1;
                        if len < 0 {
                            len = (x_b - x_a) & 0x3;
                            if len > 0 {
                                let rgb = Pix3D::colour_table()
                                    .get((colour_a >> 8) as usize)
                                    .copied()
                                    .unwrap_or(0);
                                loop {
                                    Self::put_pixel(surface, off, rgb);
                                    off += 1;
                                    len -= 1;
                                    if len <= 0 {
                                        return;
                                    }
                                }
                            }
                            break;
                        }
                        let rgb = Pix3D::colour_table()
                            .get((colour_a >> 8) as usize)
                            .copied()
                            .unwrap_or(0);
                        colour_a = colour_a.wrapping_add(colour_step);
                        Self::put_pixel(surface, off, rgb);
                        off += 1;
                        Self::put_pixel(surface, off, rgb);
                        off += 1;
                        Self::put_pixel(surface, off, rgb);
                        off += 1;
                        Self::put_pixel(surface, off, rgb);
                        off += 1;
                    }
                } else {
                    let alpha = self.trans;
                    let inv_alpha = 256 - self.trans;
                    loop {
                        len -= 1;
                        if len < 0 {
                            len = (x_b - x_a) & 0x3;
                            if len > 0 {
                                let rgb = Pix3D::colour_table()
                                    .get((colour_a >> 8) as usize)
                                    .copied()
                                    .unwrap_or(0);
                                let src = ((((rgb & 0xff00ff).wrapping_mul(inv_alpha)) >> 8)
                                    & 0xff00ff)
                                    + ((((rgb & 0xff00).wrapping_mul(inv_alpha)) >> 8) & 0xff00);
                                loop {
                                    let d = Self::pixel(surface, off);
                                    Self::put_pixel(surface, off, src
                                        + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                                        + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                                    off += 1;
                                    len -= 1;
                                    if len <= 0 {
                                        return;
                                    }
                                }
                            }
                            break;
                        }
                        let rgb = Pix3D::colour_table()
                            .get((colour_a >> 8) as usize)
                            .copied()
                            .unwrap_or(0);
                        colour_a = colour_a.wrapping_add(colour_step);
                        let src = ((((rgb & 0xff00ff).wrapping_mul(inv_alpha)) >> 8) & 0xff00ff)
                            + ((((rgb & 0xff00).wrapping_mul(inv_alpha)) >> 8) & 0xff00);
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, src
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, src
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, src
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, src
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                    }
                }
            } else if x_a < x_b {
                off += x_a;
                let mut len = (x_b - x_a) >> 2;
                let colour_step;
                if len > 0 {
                    colour_step = ((colour_b - colour_a)
                        .wrapping_mul(Pix3D::div_table().get(len as usize).copied().unwrap_or(0)))
                        >> 15;
                } else {
                    colour_step = 0;
                }
                if self.trans == 0 {
                    loop {
                        len -= 1;
                        if len < 0 {
                            len = (x_b - x_a) & 0x3;
                            if len > 0 {
                                let rgb = Pix3D::colour_table()
                                    .get((colour_a >> 8) as usize)
                                    .copied()
                                    .unwrap_or(0);
                                loop {
                                    Self::put_pixel(surface, off, rgb);
                                    off += 1;
                                    len -= 1;
                                    if len <= 0 {
                                        return;
                                    }
                                }
                            }
                            break;
                        }
                        let rgb = Pix3D::colour_table()
                            .get((colour_a >> 8) as usize)
                            .copied()
                            .unwrap_or(0);
                        colour_a = colour_a.wrapping_add(colour_step);
                        Self::put_pixel(surface, off, rgb);
                        off += 1;
                        Self::put_pixel(surface, off, rgb);
                        off += 1;
                        Self::put_pixel(surface, off, rgb);
                        off += 1;
                        Self::put_pixel(surface, off, rgb);
                        off += 1;
                    }
                } else {
                    let alpha = self.trans;
                    let inv_alpha = 256 - self.trans;
                    loop {
                        len -= 1;
                        if len < 0 {
                            len = (x_b - x_a) & 0x3;
                            if len > 0 {
                                let rgb = Pix3D::colour_table()
                                    .get((colour_a >> 8) as usize)
                                    .copied()
                                    .unwrap_or(0);
                                let src = ((((rgb & 0xff00ff).wrapping_mul(inv_alpha)) >> 8)
                                    & 0xff00ff)
                                    + ((((rgb & 0xff00).wrapping_mul(inv_alpha)) >> 8) & 0xff00);
                                loop {
                                    let d = Self::pixel(surface, off);
                                    Self::put_pixel(surface, off, src
                                        + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                                        + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                                    off += 1;
                                    len -= 1;
                                    if len <= 0 {
                                        return;
                                    }
                                }
                            }
                            break;
                        }
                        let rgb = Pix3D::colour_table()
                            .get((colour_a >> 8) as usize)
                            .copied()
                            .unwrap_or(0);
                        colour_a = colour_a.wrapping_add(colour_step);
                        let src = ((((rgb & 0xff00ff).wrapping_mul(inv_alpha)) >> 8) & 0xff00ff)
                            + ((((rgb & 0xff00).wrapping_mul(inv_alpha)) >> 8) & 0xff00);
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, src
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, src
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, src
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, src
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                    }
                }
            } else {
                return;
            }
        } else if x_a < x_b {
            let colour_step = (colour_b - colour_a) / (x_b - x_a);
            if self.hclip {
                if x_b > surface.size_x {
                    x_b = surface.size_x;
                }
                if x_a < 0 {
                    colour_a = colour_a.wrapping_sub(x_a.wrapping_mul(colour_step));
                    x_a = 0;
                }
                if x_a >= x_b {
                    return;
                }
            }
            off += x_a;
            let mut len = x_b - x_a;
            if self.trans == 0 {
                loop {
                    Self::put_pixel(surface, off, Pix3D::colour_table()
                        .get((colour_a >> 8) as usize)
                        .copied()
                        .unwrap_or(0));
                    off += 1;
                    colour_a = colour_a.wrapping_add(colour_step);
                    len -= 1;
                    if len <= 0 {
                        break;
                    }
                }
            } else {
                let alpha = self.trans;
                let inv_alpha = 256 - self.trans;
                loop {
                    let rgb = Pix3D::colour_table()
                        .get((colour_a >> 8) as usize)
                        .copied()
                        .unwrap_or(0);
                    colour_a = colour_a.wrapping_add(colour_step);
                    let src = ((((rgb & 0xff00ff).wrapping_mul(inv_alpha)) >> 8) & 0xff00ff)
                        + ((((rgb & 0xff00).wrapping_mul(inv_alpha)) >> 8) & 0xff00);
                    let d = Self::pixel(surface, off);
                    Self::put_pixel(surface, off, src
                        + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                        + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                    off += 1;
                    len -= 1;
                    if len <= 0 {
                        break;
                    }
                }
            }
        }
    }

    /// TS `flatTriangle`: flat-coloured triangle into `surface`.
    #[allow(clippy::too_many_arguments)]
    pub fn flat_triangle(
        &mut self,
        surface: &mut Pix2D,
        x_a: i32,
        x_b: i32,
        x_c: i32,
        y_a: i32,
        y_b: i32,
        y_c: i32,
        colour: i32,
    ) {
        debug_assert!(
            !self.scanline.is_empty(),
            "set_clipping/set_render_clipping must be called before rasterising"
        );
        let (mut x_a, mut x_b, mut x_c) = (x_a, x_b, x_c);
        let (mut y_a, mut y_b, mut y_c) = (y_a, y_b, y_c);

        let mut x_step_ab = 0;
        if y_b != y_a {
            x_step_ab = (x_b - x_a).wrapping_shl(16).wrapping_div(y_b - y_a);
        }

        let mut x_step_bc = 0;
        if y_c != y_b {
            x_step_bc = (x_c - x_b).wrapping_shl(16).wrapping_div(y_c - y_b);
        }

        let mut x_step_ac = 0;
        if y_c != y_a {
            x_step_ac = (x_a - x_c).wrapping_shl(16).wrapping_div(y_a - y_c);
        }

        if y_a <= y_b && y_a <= y_c {
            if y_a >= surface.clip_max_y {
                return;
            }
            if y_b > surface.clip_max_y {
                y_b = surface.clip_max_y;
            }
            if y_c > surface.clip_max_y {
                y_c = surface.clip_max_y;
            }
            if y_b < y_c {
                x_c = x_a.wrapping_shl(16);
                x_a = x_a.wrapping_shl(16);
                if y_a < 0 {
                    x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                    x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                    y_a = 0;
                }
                x_b = x_b.wrapping_shl(16);
                if y_b < 0 {
                    x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                    y_b = 0;
                }
                if (y_a != y_b && x_step_ac < x_step_ab) || (y_a == y_b && x_step_ac > x_step_bc) {
                    y_c -= y_b;
                    y_b -= y_a;
                    y_a = self.scanline[y_a as usize];
                    'outer: loop {
                        y_b -= 1;
                        if y_b < 0 {
                            loop {
                                y_c -= 1;
                                if y_c < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_c >> 16, x_b >> 16, y_a, colour);
                                x_c = x_c.wrapping_add(x_step_ac);
                                x_b = x_b.wrapping_add(x_step_bc);
                                y_a = y_a.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_c >> 16, x_a >> 16, y_a, colour);
                        x_c = x_c.wrapping_add(x_step_ac);
                        x_a = x_a.wrapping_add(x_step_ab);
                        y_a = y_a.wrapping_add(surface.width);
                    }
                } else {
                    y_c -= y_b;
                    y_b -= y_a;
                    y_a = self.scanline[y_a as usize];
                    'outer: loop {
                        y_b -= 1;
                        if y_b < 0 {
                            loop {
                                y_c -= 1;
                                if y_c < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_b >> 16, x_c >> 16, y_a, colour);
                                x_c = x_c.wrapping_add(x_step_ac);
                                x_b = x_b.wrapping_add(x_step_bc);
                                y_a = y_a.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_a >> 16, x_c >> 16, y_a, colour);
                        x_c = x_c.wrapping_add(x_step_ac);
                        x_a = x_a.wrapping_add(x_step_ab);
                        y_a = y_a.wrapping_add(surface.width);
                    }
                }
            } else {
                x_b = x_a.wrapping_shl(16);
                x_a = x_a.wrapping_shl(16);
                if y_a < 0 {
                    x_b = x_b.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                    x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                    y_a = 0;
                }
                x_c = x_c.wrapping_shl(16);
                if y_c < 0 {
                    x_c = x_c.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                    y_c = 0;
                }
                if (y_a != y_c && x_step_ac < x_step_ab) || (y_a == y_c && x_step_bc > x_step_ab) {
                    y_b -= y_c;
                    y_c -= y_a;
                    y_a = self.scanline[y_a as usize];
                    'outer: loop {
                        y_c -= 1;
                        if y_c < 0 {
                            loop {
                                y_b -= 1;
                                if y_b < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_c >> 16, x_a >> 16, y_a, colour);
                                x_c = x_c.wrapping_add(x_step_bc);
                                x_a = x_a.wrapping_add(x_step_ab);
                                y_a = y_a.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_b >> 16, x_a >> 16, y_a, colour);
                        x_b = x_b.wrapping_add(x_step_ac);
                        x_a = x_a.wrapping_add(x_step_ab);
                        y_a = y_a.wrapping_add(surface.width);
                    }
                } else {
                    y_b -= y_c;
                    y_c -= y_a;
                    y_a = self.scanline[y_a as usize];
                    'outer: loop {
                        y_c -= 1;
                        if y_c < 0 {
                            loop {
                                y_b -= 1;
                                if y_b < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_a >> 16, x_c >> 16, y_a, colour);
                                x_c = x_c.wrapping_add(x_step_bc);
                                x_a = x_a.wrapping_add(x_step_ab);
                                y_a = y_a.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_a >> 16, x_b >> 16, y_a, colour);
                        x_b = x_b.wrapping_add(x_step_ac);
                        x_a = x_a.wrapping_add(x_step_ab);
                        y_a = y_a.wrapping_add(surface.width);
                    }
                }
            }
        } else if y_b <= y_c {
            if y_b >= surface.clip_max_y {
                return;
            }
            if y_c > surface.clip_max_y {
                y_c = surface.clip_max_y;
            }
            if y_a > surface.clip_max_y {
                y_a = surface.clip_max_y;
            }
            if y_c < y_a {
                x_a = x_b.wrapping_shl(16);
                x_b = x_b.wrapping_shl(16);
                if y_b < 0 {
                    x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                    x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                    y_b = 0;
                }
                x_c = x_c.wrapping_shl(16);
                if y_c < 0 {
                    x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                    y_c = 0;
                }
                if (y_b != y_c && x_step_ab < x_step_bc) || (y_b == y_c && x_step_ab > x_step_ac) {
                    y_a -= y_c;
                    y_c -= y_b;
                    y_b = self.scanline[y_b as usize];
                    'outer: loop {
                        y_c -= 1;
                        if y_c < 0 {
                            loop {
                                y_a -= 1;
                                if y_a < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_a >> 16, x_c >> 16, y_b, colour);
                                x_a = x_a.wrapping_add(x_step_ab);
                                x_c = x_c.wrapping_add(x_step_ac);
                                y_b = y_b.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_a >> 16, x_b >> 16, y_b, colour);
                        x_a = x_a.wrapping_add(x_step_ab);
                        x_b = x_b.wrapping_add(x_step_bc);
                        y_b = y_b.wrapping_add(surface.width);
                    }
                } else {
                    y_a -= y_c;
                    y_c -= y_b;
                    y_b = self.scanline[y_b as usize];
                    'outer: loop {
                        y_c -= 1;
                        if y_c < 0 {
                            loop {
                                y_a -= 1;
                                if y_a < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_c >> 16, x_a >> 16, y_b, colour);
                                x_a = x_a.wrapping_add(x_step_ab);
                                x_c = x_c.wrapping_add(x_step_ac);
                                y_b = y_b.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_b >> 16, x_a >> 16, y_b, colour);
                        x_a = x_a.wrapping_add(x_step_ab);
                        x_b = x_b.wrapping_add(x_step_bc);
                        y_b = y_b.wrapping_add(surface.width);
                    }
                }
            } else {
                x_c = x_b.wrapping_shl(16);
                x_b = x_b.wrapping_shl(16);
                if y_b < 0 {
                    x_c = x_c.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                    x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                    y_b = 0;
                }
                x_a = x_a.wrapping_shl(16);
                if y_a < 0 {
                    x_a = x_a.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                    y_a = 0;
                }
                y_c -= y_a;
                y_a -= y_b;
                y_b = self.scanline[y_b as usize];
                if x_step_ab < x_step_bc {
                    'outer: loop {
                        y_a -= 1;
                        if y_a < 0 {
                            loop {
                                y_c -= 1;
                                if y_c < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_a >> 16, x_b >> 16, y_b, colour);
                                x_a = x_a.wrapping_add(x_step_ac);
                                x_b = x_b.wrapping_add(x_step_bc);
                                y_b = y_b.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_c >> 16, x_b >> 16, y_b, colour);
                        x_c = x_c.wrapping_add(x_step_ab);
                        x_b = x_b.wrapping_add(x_step_bc);
                        y_b = y_b.wrapping_add(surface.width);
                    }
                } else {
                    'outer: loop {
                        y_a -= 1;
                        if y_a < 0 {
                            loop {
                                y_c -= 1;
                                if y_c < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_b >> 16, x_a >> 16, y_b, colour);
                                x_a = x_a.wrapping_add(x_step_ac);
                                x_b = x_b.wrapping_add(x_step_bc);
                                y_b = y_b.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_b >> 16, x_c >> 16, y_b, colour);
                        x_c = x_c.wrapping_add(x_step_ab);
                        x_b = x_b.wrapping_add(x_step_bc);
                        y_b = y_b.wrapping_add(surface.width);
                    }
                }
            }
        } else {
            if y_c >= surface.clip_max_y {
                return;
            }
            if y_a > surface.clip_max_y {
                y_a = surface.clip_max_y;
            }
            if y_b > surface.clip_max_y {
                y_b = surface.clip_max_y;
            }
            if y_a < y_b {
                x_b = x_c.wrapping_shl(16);
                x_c = x_c.wrapping_shl(16);
                if y_c < 0 {
                    x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                    x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                    y_c = 0;
                }
                x_a = x_a.wrapping_shl(16);
                if y_a < 0 {
                    x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                    y_a = 0;
                }
                y_b -= y_a;
                y_a -= y_c;
                y_c = self.scanline[y_c as usize];
                if x_step_bc < x_step_ac {
                    'outer: loop {
                        y_a -= 1;
                        if y_a < 0 {
                            loop {
                                y_b -= 1;
                                if y_b < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_b >> 16, x_a >> 16, y_c, colour);
                                x_b = x_b.wrapping_add(x_step_bc);
                                x_a = x_a.wrapping_add(x_step_ab);
                                y_c = y_c.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_b >> 16, x_c >> 16, y_c, colour);
                        x_b = x_b.wrapping_add(x_step_bc);
                        x_c = x_c.wrapping_add(x_step_ac);
                        y_c = y_c.wrapping_add(surface.width);
                    }
                } else {
                    'outer: loop {
                        y_a -= 1;
                        if y_a < 0 {
                            loop {
                                y_b -= 1;
                                if y_b < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_a >> 16, x_b >> 16, y_c, colour);
                                x_b = x_b.wrapping_add(x_step_bc);
                                x_a = x_a.wrapping_add(x_step_ab);
                                y_c = y_c.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_c >> 16, x_b >> 16, y_c, colour);
                        x_b = x_b.wrapping_add(x_step_bc);
                        x_c = x_c.wrapping_add(x_step_ac);
                        y_c = y_c.wrapping_add(surface.width);
                    }
                }
            } else {
                x_a = x_c.wrapping_shl(16);
                x_c = x_c.wrapping_shl(16);
                if y_c < 0 {
                    x_a = x_a.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                    x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                    y_c = 0;
                }
                x_b = x_b.wrapping_shl(16);
                if y_b < 0 {
                    x_b = x_b.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                    y_b = 0;
                }
                y_a -= y_b;
                y_b -= y_c;
                y_c = self.scanline[y_c as usize];
                if x_step_bc < x_step_ac {
                    'outer: loop {
                        y_b -= 1;
                        if y_b < 0 {
                            loop {
                                y_a -= 1;
                                if y_a < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_b >> 16, x_c >> 16, y_c, colour);
                                x_b = x_b.wrapping_add(x_step_ab);
                                x_c = x_c.wrapping_add(x_step_ac);
                                y_c = y_c.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_a >> 16, x_c >> 16, y_c, colour);
                        x_a = x_a.wrapping_add(x_step_bc);
                        x_c = x_c.wrapping_add(x_step_ac);
                        y_c = y_c.wrapping_add(surface.width);
                    }
                } else {
                    'outer: loop {
                        y_b -= 1;
                        if y_b < 0 {
                            loop {
                                y_a -= 1;
                                if y_a < 0 {
                                    break 'outer;
                                }
                                self.flat_raster(surface, x_c >> 16, x_b >> 16, y_c, colour);
                                x_b = x_b.wrapping_add(x_step_ab);
                                x_c = x_c.wrapping_add(x_step_ac);
                                y_c = y_c.wrapping_add(surface.width);
                            }
                        }
                        self.flat_raster(surface, x_c >> 16, x_a >> 16, y_c, colour);
                        x_a = x_a.wrapping_add(x_step_bc);
                        x_c = x_c.wrapping_add(x_step_ac);
                        y_c = y_c.wrapping_add(surface.width);
                    }
                }
            }
        }
    }

    /// TS `flatRaster`: a flat span of `surface.pixels` starting at `off`.
    ///
    /// Writes go through `put_pixel`, so an out-of-bounds offset (negative
    /// `off`, or past the buffer) is a silent no-op like a TS typed-array
    /// write; `hclip` still clamps spans to `[0, size_x]` when set.
    fn flat_raster(
        &self,
        surface: &mut Pix2D,
        mut x_a: i32,
        mut x_b: i32,
        mut off: i32,
        colour: i32,
    ) {
        if self.hclip {
            if x_b > surface.size_x {
                x_b = surface.size_x;
            }
            if x_a < 0 {
                x_a = 0;
            }
        }
        if x_a >= x_b {
            return;
        }
        off += x_a;
        let mut len = (x_b - x_a) >> 2;
        if self.trans == 0 {
            loop {
                len -= 1;
                if len < 0 {
                    len = (x_b - x_a) & 0x3;
                    loop {
                        len -= 1;
                        if len < 0 {
                            return;
                        }
                        Self::put_pixel(surface, off, colour);
                        off += 1;
                    }
                }
                Self::put_pixel(surface, off, colour);
                off += 1;
                Self::put_pixel(surface, off, colour);
                off += 1;
                Self::put_pixel(surface, off, colour);
                off += 1;
                Self::put_pixel(surface, off, colour);
                off += 1;
            }
        } else {
            let alpha = self.trans;
            let inv_alpha = 256 - self.trans;
            let colour = ((((colour & 0xff00ff).wrapping_mul(inv_alpha)) >> 8) & 0xff00ff)
                + ((((colour & 0xff00).wrapping_mul(inv_alpha)) >> 8) & 0xff00);
            loop {
                len -= 1;
                if len < 0 {
                    len = (x_b - x_a) & 0x3;
                    loop {
                        len -= 1;
                        if len < 0 {
                            return;
                        }
                        let d = Self::pixel(surface, off);
                        Self::put_pixel(surface, off, colour
                            + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                            + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                        off += 1;
                    }
                }
                let d = Self::pixel(surface, off);
                Self::put_pixel(surface, off, colour
                    + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                    + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                off += 1;
                let d = Self::pixel(surface, off);
                Self::put_pixel(surface, off, colour
                    + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                    + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                off += 1;
                let d = Self::pixel(surface, off);
                Self::put_pixel(surface, off, colour
                    + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                    + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                off += 1;
                let d = Self::pixel(surface, off);
                Self::put_pixel(surface, off, colour
                    + ((((d & 0xff00ff).wrapping_mul(alpha)) >> 8) & 0xff00ff)
                    + ((((d & 0xff00).wrapping_mul(alpha)) >> 8) & 0xff00));
                off += 1;
            }
        }
    }

    /// TS `textureTriangle`: texture-mapped triangle into `surface`. The
    /// `originX/Y/Z` parameters are the first texture-space vertex (they
    /// shadow the TS `originX`/`originY` statics exactly like the TS
    /// parameter names do); `self.origin_y` below is the screen origin.
    #[allow(clippy::too_many_arguments)]
    pub fn texture_triangle(
        &mut self,
        surface: &mut Pix2D,
        x_a: i32,
        x_b: i32,
        x_c: i32,
        y_a: i32,
        y_b: i32,
        y_c: i32,
        shade_a: i32,
        shade_b: i32,
        shade_c: i32,
        origin_x: i32,
        origin_y: i32,
        origin_z: i32,
        tx_b: i32,
        tx_c: i32,
        ty_b: i32,
        ty_c: i32,
        tz_b: i32,
        tz_c: i32,
        texture: i32,
    ) {
        debug_assert!(
            !self.scanline.is_empty(),
            "set_clipping/set_render_clipping must be called before rasterising"
        );
        let texels = match self.get_texels(texture as usize) {
            Some(t) => t,
            None => return,
        };
        let (mut x_a, mut x_b, mut x_c) = (x_a, x_b, x_c);
        let (mut y_a, mut y_b, mut y_c) = (y_a, y_b, y_c);
        let (mut shade_a, mut shade_b, mut shade_c) = (shade_a, shade_b, shade_c);

        let vertical_x = origin_x - tx_b;
        let vertical_y = origin_y - ty_b;
        let vertical_z = origin_z - tz_b;
        let horizontal_x = tx_c - origin_x;
        let horizontal_y = ty_c - origin_y;
        let horizontal_z = tz_c - origin_z;

        let mut u = (horizontal_x.wrapping_mul(origin_y))
            .wrapping_sub(horizontal_y.wrapping_mul(origin_x))
            .wrapping_shl(14);
        let u_stride = (horizontal_y.wrapping_mul(origin_z))
            .wrapping_sub(horizontal_z.wrapping_mul(origin_y))
            .wrapping_shl(8);
        let u_step_vertical = (horizontal_z.wrapping_mul(origin_x))
            .wrapping_sub(horizontal_x.wrapping_mul(origin_z))
            .wrapping_shl(5);

        let mut v = (vertical_x.wrapping_mul(origin_y))
            .wrapping_sub(vertical_y.wrapping_mul(origin_x))
            .wrapping_shl(14);
        let v_stride = (vertical_y.wrapping_mul(origin_z))
            .wrapping_sub(vertical_z.wrapping_mul(origin_y))
            .wrapping_shl(8);
        let v_step_vertical = (vertical_z.wrapping_mul(origin_x))
            .wrapping_sub(vertical_x.wrapping_mul(origin_z))
            .wrapping_shl(5);

        let mut w = (vertical_y.wrapping_mul(horizontal_x))
            .wrapping_sub(vertical_x.wrapping_mul(horizontal_y))
            .wrapping_shl(14);
        let w_stride = (vertical_z.wrapping_mul(horizontal_y))
            .wrapping_sub(vertical_y.wrapping_mul(horizontal_z))
            .wrapping_shl(8);
        let w_step_vertical = (vertical_x.wrapping_mul(horizontal_z))
            .wrapping_sub(vertical_z.wrapping_mul(horizontal_x))
            .wrapping_shl(5);

        let mut x_step_ab = 0;
        let mut shade_step_ab = 0;
        if y_b != y_a {
            x_step_ab = (x_b - x_a).wrapping_shl(16).wrapping_div(y_b - y_a);
            shade_step_ab = (shade_b - shade_a).wrapping_shl(16).wrapping_div(y_b - y_a);
        }

        let mut x_step_bc = 0;
        let mut shade_step_bc = 0;
        if y_c != y_b {
            x_step_bc = (x_c - x_b).wrapping_shl(16).wrapping_div(y_c - y_b);
            shade_step_bc = (shade_c - shade_b).wrapping_shl(16).wrapping_div(y_c - y_b);
        }

        let mut x_step_ac = 0;
        let mut shade_step_ac = 0;
        if y_c != y_a {
            x_step_ac = (x_a - x_c).wrapping_shl(16).wrapping_div(y_a - y_c);
            shade_step_ac = (shade_a - shade_c).wrapping_shl(16).wrapping_div(y_a - y_c);
        }

        // Every TS `return` exits the whole method; wrap the body in a
        // labelled block so the texel row is put back into `active_texels`
        // (the TS never removes it) on every exit path.
        'run: {
            if y_a <= y_b && y_a <= y_c {
                if y_a >= surface.clip_max_y {
                    break 'run;
                }
                if y_b > surface.clip_max_y {
                    y_b = surface.clip_max_y;
                }
                if y_c > surface.clip_max_y {
                    y_c = surface.clip_max_y;
                }
                if y_b < y_c {
                    x_c = x_a.wrapping_shl(16);
                    x_a = x_a.wrapping_shl(16);
                    shade_c = shade_a.wrapping_shl(16);
                    shade_a = shade_a.wrapping_shl(16);
                    if y_a < 0 {
                        x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                        x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                        shade_c = shade_c.wrapping_sub(shade_step_ac.wrapping_mul(y_a));
                        shade_a = shade_a.wrapping_sub(shade_step_ab.wrapping_mul(y_a));
                        y_a = 0;
                    }
                    x_b = x_b.wrapping_shl(16);
                    shade_b = shade_b.wrapping_shl(16);
                    if y_b < 0 {
                        x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                        shade_b = shade_b.wrapping_sub(shade_step_bc.wrapping_mul(y_b));
                        y_b = 0;
                    }
                    let dy = y_a - self.origin_y;
                    u = u.wrapping_add(u_step_vertical.wrapping_mul(dy));
                    v = v.wrapping_add(v_step_vertical.wrapping_mul(dy));
                    w = w.wrapping_add(w_step_vertical.wrapping_mul(dy));
                    if (y_a != y_b && x_step_ac < x_step_ab)
                        || (y_a == y_b && x_step_ac > x_step_bc)
                    {
                        y_c -= y_b;
                        y_b -= y_a;
                        y_a = self.scanline[y_a as usize];
                        'outer: loop {
                            y_b -= 1;
                            if y_b < 0 {
                                loop {
                                    y_c -= 1;
                                    if y_c < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_c >> 16,
                                        x_b >> 16,
                                        y_a,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_c >> 8,
                                        shade_b >> 8,
                                    );
                                    x_c = x_c.wrapping_add(x_step_ac);
                                    x_b = x_b.wrapping_add(x_step_bc);
                                    shade_c = shade_c.wrapping_add(shade_step_ac);
                                    shade_b = shade_b.wrapping_add(shade_step_bc);
                                    y_a = y_a.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_c >> 16,
                                x_a >> 16,
                                y_a,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_c >> 8,
                                shade_a >> 8,
                            );
                            x_c = x_c.wrapping_add(x_step_ac);
                            x_a = x_a.wrapping_add(x_step_ab);
                            shade_c = shade_c.wrapping_add(shade_step_ac);
                            shade_a = shade_a.wrapping_add(shade_step_ab);
                            y_a = y_a.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    } else {
                        y_c -= y_b;
                        y_b -= y_a;
                        y_a = self.scanline[y_a as usize];
                        'outer: loop {
                            y_b -= 1;
                            if y_b < 0 {
                                loop {
                                    y_c -= 1;
                                    if y_c < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_b >> 16,
                                        x_c >> 16,
                                        y_a,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_b >> 8,
                                        shade_c >> 8,
                                    );
                                    x_c = x_c.wrapping_add(x_step_ac);
                                    x_b = x_b.wrapping_add(x_step_bc);
                                    shade_c = shade_c.wrapping_add(shade_step_ac);
                                    shade_b = shade_b.wrapping_add(shade_step_bc);
                                    y_a = y_a.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_a >> 16,
                                x_c >> 16,
                                y_a,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_a >> 8,
                                shade_c >> 8,
                            );
                            x_c = x_c.wrapping_add(x_step_ac);
                            x_a = x_a.wrapping_add(x_step_ab);
                            shade_c = shade_c.wrapping_add(shade_step_ac);
                            shade_a = shade_a.wrapping_add(shade_step_ab);
                            y_a = y_a.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    }
                } else {
                    x_b = x_a.wrapping_shl(16);
                    x_a = x_a.wrapping_shl(16);
                    shade_b = shade_a.wrapping_shl(16);
                    shade_a = shade_a.wrapping_shl(16);
                    if y_a < 0 {
                        x_b = x_b.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                        x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                        shade_b = shade_b.wrapping_sub(shade_step_ac.wrapping_mul(y_a));
                        shade_a = shade_a.wrapping_sub(shade_step_ab.wrapping_mul(y_a));
                        y_a = 0;
                    }
                    x_c = x_c.wrapping_shl(16);
                    shade_c = shade_c.wrapping_shl(16);
                    if y_c < 0 {
                        x_c = x_c.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                        shade_c = shade_c.wrapping_sub(shade_step_bc.wrapping_mul(y_c));
                        y_c = 0;
                    }
                    let dy = y_a - self.origin_y;
                    u = u.wrapping_add(u_step_vertical.wrapping_mul(dy));
                    v = v.wrapping_add(v_step_vertical.wrapping_mul(dy));
                    w = w.wrapping_add(w_step_vertical.wrapping_mul(dy));
                    if (y_a == y_c || x_step_ac >= x_step_ab)
                        && (y_a != y_c || x_step_bc <= x_step_ab)
                    {
                        y_b -= y_c;
                        y_c -= y_a;
                        y_a = self.scanline[y_a as usize];
                        'outer: loop {
                            y_c -= 1;
                            if y_c < 0 {
                                loop {
                                    y_b -= 1;
                                    if y_b < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_a >> 16,
                                        x_c >> 16,
                                        y_a,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_a >> 8,
                                        shade_c >> 8,
                                    );
                                    x_c = x_c.wrapping_add(x_step_bc);
                                    x_a = x_a.wrapping_add(x_step_ab);
                                    shade_c = shade_c.wrapping_add(shade_step_bc);
                                    shade_a = shade_a.wrapping_add(shade_step_ab);
                                    y_a = y_a.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_a >> 16,
                                x_b >> 16,
                                y_a,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_a >> 8,
                                shade_b >> 8,
                            );
                            x_b = x_b.wrapping_add(x_step_ac);
                            x_a = x_a.wrapping_add(x_step_ab);
                            shade_b = shade_b.wrapping_add(shade_step_ac);
                            shade_a = shade_a.wrapping_add(shade_step_ab);
                            y_a = y_a.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    } else {
                        y_b -= y_c;
                        y_c -= y_a;
                        y_a = self.scanline[y_a as usize];
                        'outer: loop {
                            y_c -= 1;
                            if y_c < 0 {
                                loop {
                                    y_b -= 1;
                                    if y_b < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_c >> 16,
                                        x_a >> 16,
                                        y_a,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_c >> 8,
                                        shade_a >> 8,
                                    );
                                    x_c = x_c.wrapping_add(x_step_bc);
                                    x_a = x_a.wrapping_add(x_step_ab);
                                    shade_c = shade_c.wrapping_add(shade_step_bc);
                                    shade_a = shade_a.wrapping_add(shade_step_ab);
                                    y_a = y_a.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_b >> 16,
                                x_a >> 16,
                                y_a,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_b >> 8,
                                shade_a >> 8,
                            );
                            x_b = x_b.wrapping_add(x_step_ac);
                            x_a = x_a.wrapping_add(x_step_ab);
                            shade_b = shade_b.wrapping_add(shade_step_ac);
                            shade_a = shade_a.wrapping_add(shade_step_ab);
                            y_a = y_a.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    }
                }
            } else if y_b <= y_c {
                if y_b >= surface.clip_max_y {
                    break 'run;
                }
                if y_c > surface.clip_max_y {
                    y_c = surface.clip_max_y;
                }
                if y_a > surface.clip_max_y {
                    y_a = surface.clip_max_y;
                }
                if y_c < y_a {
                    x_a = x_b.wrapping_shl(16);
                    x_b = x_b.wrapping_shl(16);
                    shade_a = shade_b.wrapping_shl(16);
                    shade_b = shade_b.wrapping_shl(16);
                    if y_b < 0 {
                        x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                        x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                        shade_a = shade_a.wrapping_sub(shade_step_ab.wrapping_mul(y_b));
                        shade_b = shade_b.wrapping_sub(shade_step_bc.wrapping_mul(y_b));
                        y_b = 0;
                    }
                    x_c = x_c.wrapping_shl(16);
                    shade_c = shade_c.wrapping_shl(16);
                    if y_c < 0 {
                        x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                        shade_c = shade_c.wrapping_sub(shade_step_ac.wrapping_mul(y_c));
                        y_c = 0;
                    }
                    let dy = y_b - self.origin_y;
                    u = u.wrapping_add(u_step_vertical.wrapping_mul(dy));
                    v = v.wrapping_add(v_step_vertical.wrapping_mul(dy));
                    w = w.wrapping_add(w_step_vertical.wrapping_mul(dy));
                    if (y_b != y_c && x_step_ab < x_step_bc)
                        || (y_b == y_c && x_step_ab > x_step_ac)
                    {
                        y_a -= y_c;
                        y_c -= y_b;
                        y_b = self.scanline[y_b as usize];
                        'outer: loop {
                            y_c -= 1;
                            if y_c < 0 {
                                loop {
                                    y_a -= 1;
                                    if y_a < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_a >> 16,
                                        x_c >> 16,
                                        y_b,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_a >> 8,
                                        shade_c >> 8,
                                    );
                                    x_a = x_a.wrapping_add(x_step_ab);
                                    x_c = x_c.wrapping_add(x_step_ac);
                                    shade_a = shade_a.wrapping_add(shade_step_ab);
                                    shade_c = shade_c.wrapping_add(shade_step_ac);
                                    y_b = y_b.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_a >> 16,
                                x_b >> 16,
                                y_b,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_a >> 8,
                                shade_b >> 8,
                            );
                            x_a = x_a.wrapping_add(x_step_ab);
                            x_b = x_b.wrapping_add(x_step_bc);
                            shade_a = shade_a.wrapping_add(shade_step_ab);
                            shade_b = shade_b.wrapping_add(shade_step_bc);
                            y_b = y_b.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    } else {
                        y_a -= y_c;
                        y_c -= y_b;
                        y_b = self.scanline[y_b as usize];
                        'outer: loop {
                            y_c -= 1;
                            if y_c < 0 {
                                loop {
                                    y_a -= 1;
                                    if y_a < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_c >> 16,
                                        x_a >> 16,
                                        y_b,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_c >> 8,
                                        shade_a >> 8,
                                    );
                                    x_a = x_a.wrapping_add(x_step_ab);
                                    x_c = x_c.wrapping_add(x_step_ac);
                                    shade_a = shade_a.wrapping_add(shade_step_ab);
                                    shade_c = shade_c.wrapping_add(shade_step_ac);
                                    y_b = y_b.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_b >> 16,
                                x_a >> 16,
                                y_b,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_b >> 8,
                                shade_a >> 8,
                            );
                            x_a = x_a.wrapping_add(x_step_ab);
                            x_b = x_b.wrapping_add(x_step_bc);
                            shade_a = shade_a.wrapping_add(shade_step_ab);
                            shade_b = shade_b.wrapping_add(shade_step_bc);
                            y_b = y_b.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    }
                } else {
                    x_c = x_b.wrapping_shl(16);
                    x_b = x_b.wrapping_shl(16);
                    shade_c = shade_b.wrapping_shl(16);
                    shade_b = shade_b.wrapping_shl(16);
                    if y_b < 0 {
                        x_c = x_c.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                        x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_b));
                        shade_c = shade_c.wrapping_sub(shade_step_ab.wrapping_mul(y_b));
                        shade_b = shade_b.wrapping_sub(shade_step_bc.wrapping_mul(y_b));
                        y_b = 0;
                    }
                    x_a = x_a.wrapping_shl(16);
                    shade_a = shade_a.wrapping_shl(16);
                    if y_a < 0 {
                        x_a = x_a.wrapping_sub(x_step_ac.wrapping_mul(y_a));
                        shade_a = shade_a.wrapping_sub(shade_step_ac.wrapping_mul(y_a));
                        y_a = 0;
                    }
                    let dy = y_b - self.origin_y;
                    u = u.wrapping_add(u_step_vertical.wrapping_mul(dy));
                    v = v.wrapping_add(v_step_vertical.wrapping_mul(dy));
                    w = w.wrapping_add(w_step_vertical.wrapping_mul(dy));
                    y_c -= y_a;
                    y_a -= y_b;
                    y_b = self.scanline[y_b as usize];
                    if x_step_ab < x_step_bc {
                        'outer: loop {
                            y_a -= 1;
                            if y_a < 0 {
                                loop {
                                    y_c -= 1;
                                    if y_c < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_a >> 16,
                                        x_b >> 16,
                                        y_b,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_a >> 8,
                                        shade_b >> 8,
                                    );
                                    x_a = x_a.wrapping_add(x_step_ac);
                                    x_b = x_b.wrapping_add(x_step_bc);
                                    shade_a = shade_a.wrapping_add(shade_step_ac);
                                    shade_b = shade_b.wrapping_add(shade_step_bc);
                                    y_b = y_b.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_c >> 16,
                                x_b >> 16,
                                y_b,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_c >> 8,
                                shade_b >> 8,
                            );
                            x_c = x_c.wrapping_add(x_step_ab);
                            x_b = x_b.wrapping_add(x_step_bc);
                            shade_c = shade_c.wrapping_add(shade_step_ab);
                            shade_b = shade_b.wrapping_add(shade_step_bc);
                            y_b = y_b.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    } else {
                        'outer: loop {
                            y_a -= 1;
                            if y_a < 0 {
                                loop {
                                    y_c -= 1;
                                    if y_c < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_b >> 16,
                                        x_a >> 16,
                                        y_b,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_b >> 8,
                                        shade_a >> 8,
                                    );
                                    x_a = x_a.wrapping_add(x_step_ac);
                                    x_b = x_b.wrapping_add(x_step_bc);
                                    shade_a = shade_a.wrapping_add(shade_step_ac);
                                    shade_b = shade_b.wrapping_add(shade_step_bc);
                                    y_b = y_b.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_b >> 16,
                                x_c >> 16,
                                y_b,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_b >> 8,
                                shade_c >> 8,
                            );
                            x_c = x_c.wrapping_add(x_step_ab);
                            x_b = x_b.wrapping_add(x_step_bc);
                            shade_c = shade_c.wrapping_add(shade_step_ab);
                            shade_b = shade_b.wrapping_add(shade_step_bc);
                            y_b = y_b.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    }
                }
            } else {
                if y_c >= surface.clip_max_y {
                    break 'run;
                }
                if y_a > surface.clip_max_y {
                    y_a = surface.clip_max_y;
                }
                if y_b > surface.clip_max_y {
                    y_b = surface.clip_max_y;
                }
                if y_a < y_b {
                    x_b = x_c.wrapping_shl(16);
                    x_c = x_c.wrapping_shl(16);
                    shade_b = shade_c.wrapping_shl(16);
                    shade_c = shade_c.wrapping_shl(16);
                    if y_c < 0 {
                        x_b = x_b.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                        x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                        shade_b = shade_b.wrapping_sub(shade_step_bc.wrapping_mul(y_c));
                        shade_c = shade_c.wrapping_sub(shade_step_ac.wrapping_mul(y_c));
                        y_c = 0;
                    }
                    x_a = x_a.wrapping_shl(16);
                    shade_a = shade_a.wrapping_shl(16);
                    if y_a < 0 {
                        x_a = x_a.wrapping_sub(x_step_ab.wrapping_mul(y_a));
                        shade_a = shade_a.wrapping_sub(shade_step_ab.wrapping_mul(y_a));
                        y_a = 0;
                    }
                    let dy = y_c - self.origin_y;
                    u = u.wrapping_add(u_step_vertical.wrapping_mul(dy));
                    v = v.wrapping_add(v_step_vertical.wrapping_mul(dy));
                    w = w.wrapping_add(w_step_vertical.wrapping_mul(dy));
                    y_b -= y_a;
                    y_a -= y_c;
                    y_c = self.scanline[y_c as usize];
                    if x_step_bc < x_step_ac {
                        'outer: loop {
                            y_a -= 1;
                            if y_a < 0 {
                                loop {
                                    y_b -= 1;
                                    if y_b < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_b >> 16,
                                        x_a >> 16,
                                        y_c,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_b >> 8,
                                        shade_a >> 8,
                                    );
                                    x_b = x_b.wrapping_add(x_step_bc);
                                    x_a = x_a.wrapping_add(x_step_ab);
                                    shade_b = shade_b.wrapping_add(shade_step_bc);
                                    shade_a = shade_a.wrapping_add(shade_step_ab);
                                    y_c = y_c.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_b >> 16,
                                x_c >> 16,
                                y_c,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_b >> 8,
                                shade_c >> 8,
                            );
                            x_b = x_b.wrapping_add(x_step_bc);
                            x_c = x_c.wrapping_add(x_step_ac);
                            shade_b = shade_b.wrapping_add(shade_step_bc);
                            shade_c = shade_c.wrapping_add(shade_step_ac);
                            y_c = y_c.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    } else {
                        'outer: loop {
                            y_a -= 1;
                            if y_a < 0 {
                                loop {
                                    y_b -= 1;
                                    if y_b < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_a >> 16,
                                        x_b >> 16,
                                        y_c,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_a >> 8,
                                        shade_b >> 8,
                                    );
                                    x_b = x_b.wrapping_add(x_step_bc);
                                    x_a = x_a.wrapping_add(x_step_ab);
                                    shade_b = shade_b.wrapping_add(shade_step_bc);
                                    shade_a = shade_a.wrapping_add(shade_step_ab);
                                    y_c = y_c.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_c >> 16,
                                x_b >> 16,
                                y_c,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_c >> 8,
                                shade_b >> 8,
                            );
                            x_b = x_b.wrapping_add(x_step_bc);
                            x_c = x_c.wrapping_add(x_step_ac);
                            shade_b = shade_b.wrapping_add(shade_step_bc);
                            shade_c = shade_c.wrapping_add(shade_step_ac);
                            y_c = y_c.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    }
                } else {
                    x_a = x_c.wrapping_shl(16);
                    x_c = x_c.wrapping_shl(16);
                    shade_a = shade_c.wrapping_shl(16);
                    shade_c = shade_c.wrapping_shl(16);
                    if y_c < 0 {
                        x_a = x_a.wrapping_sub(x_step_bc.wrapping_mul(y_c));
                        x_c = x_c.wrapping_sub(x_step_ac.wrapping_mul(y_c));
                        shade_a = shade_a.wrapping_sub(shade_step_bc.wrapping_mul(y_c));
                        shade_c = shade_c.wrapping_sub(shade_step_ac.wrapping_mul(y_c));
                        y_c = 0;
                    }
                    x_b = x_b.wrapping_shl(16);
                    shade_b = shade_b.wrapping_shl(16);
                    if y_b < 0 {
                        x_b = x_b.wrapping_sub(x_step_ab.wrapping_mul(y_b));
                        shade_b = shade_b.wrapping_sub(shade_step_ab.wrapping_mul(y_b));
                        y_b = 0;
                    }
                    let dy = y_c - self.origin_y;
                    u = u.wrapping_add(u_step_vertical.wrapping_mul(dy));
                    v = v.wrapping_add(v_step_vertical.wrapping_mul(dy));
                    w = w.wrapping_add(w_step_vertical.wrapping_mul(dy));
                    y_a -= y_b;
                    y_b -= y_c;
                    y_c = self.scanline[y_c as usize];
                    if x_step_bc < x_step_ac {
                        'outer: loop {
                            y_b -= 1;
                            if y_b < 0 {
                                loop {
                                    y_a -= 1;
                                    if y_a < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_b >> 16,
                                        x_c >> 16,
                                        y_c,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_b >> 8,
                                        shade_c >> 8,
                                    );
                                    x_b = x_b.wrapping_add(x_step_ab);
                                    x_c = x_c.wrapping_add(x_step_ac);
                                    shade_b = shade_b.wrapping_add(shade_step_ab);
                                    shade_c = shade_c.wrapping_add(shade_step_ac);
                                    y_c = y_c.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_a >> 16,
                                x_c >> 16,
                                y_c,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_a >> 8,
                                shade_c >> 8,
                            );
                            x_a = x_a.wrapping_add(x_step_bc);
                            x_c = x_c.wrapping_add(x_step_ac);
                            shade_a = shade_a.wrapping_add(shade_step_bc);
                            shade_c = shade_c.wrapping_add(shade_step_ac);
                            y_c = y_c.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    } else {
                        'outer: loop {
                            y_b -= 1;
                            if y_b < 0 {
                                loop {
                                    y_a -= 1;
                                    if y_a < 0 {
                                        break 'outer;
                                    }
                                    self.texture_raster(
                                        surface,
                                        x_c >> 16,
                                        x_b >> 16,
                                        y_c,
                                        &texels,
                                        u,
                                        v,
                                        w,
                                        u_stride,
                                        v_stride,
                                        w_stride,
                                        shade_c >> 8,
                                        shade_b >> 8,
                                    );
                                    x_b = x_b.wrapping_add(x_step_ab);
                                    x_c = x_c.wrapping_add(x_step_ac);
                                    shade_b = shade_b.wrapping_add(shade_step_ab);
                                    shade_c = shade_c.wrapping_add(shade_step_ac);
                                    y_c = y_c.wrapping_add(surface.width);
                                    u = u.wrapping_add(u_step_vertical);
                                    v = v.wrapping_add(v_step_vertical);
                                    w = w.wrapping_add(w_step_vertical);
                                }
                            }
                            self.texture_raster(
                                surface,
                                x_c >> 16,
                                x_a >> 16,
                                y_c,
                                &texels,
                                u,
                                v,
                                w,
                                u_stride,
                                v_stride,
                                w_stride,
                                shade_c >> 8,
                                shade_a >> 8,
                            );
                            x_a = x_a.wrapping_add(x_step_bc);
                            x_c = x_c.wrapping_add(x_step_ac);
                            shade_a = shade_a.wrapping_add(shade_step_bc);
                            shade_c = shade_c.wrapping_add(shade_step_ac);
                            y_c = y_c.wrapping_add(surface.width);
                            u = u.wrapping_add(u_step_vertical);
                            v = v.wrapping_add(v_step_vertical);
                            w = w.wrapping_add(w_step_vertical);
                        }
                    }
                }
            }
        }
        self.active_texels[texture as usize] = Some(texels);
    }

    /// TS `textureRaster`: a perspective-correct texture span of
    /// `surface.pixels` starting at `off`. The TS `curU`/`curV` parameters
    /// are dropped: they are always 0 and immediately recomputed here.
    ///
    /// Writes go through `put_pixel`, so an out-of-bounds offset (negative
    /// `off`, or past the buffer) is a silent no-op like a TS typed-array
    /// write; `hclip` still clamps spans to `[0, size_x]` when set.
    #[allow(clippy::too_many_arguments)]
    fn texture_raster(
        &self,
        surface: &mut Pix2D,
        mut x_a: i32,
        mut x_b: i32,
        mut off: i32,
        texels: &[i32],
        mut u: i32,
        mut v: i32,
        mut w: i32,
        u_stride: i32,
        v_stride: i32,
        w_stride: i32,
        mut shade_a: i32,
        shade_b: i32,
    ) {
        if x_a >= x_b {
            return;
        }

        let mut shade_strides;
        let mut strides;
        if self.hclip {
            shade_strides = (shade_b - shade_a) / (x_b - x_a);
            if x_b > surface.size_x {
                x_b = surface.size_x;
            }
            if x_a < 0 {
                shade_a = shade_a.wrapping_sub(x_a.wrapping_mul(shade_strides));
                x_a = 0;
            }
            if x_a >= x_b {
                return;
            }
            strides = (x_b - x_a) >> 3;
            shade_strides = shade_strides.wrapping_shl(12);
        } else {
            if x_b - x_a > 7 {
                strides = (x_b - x_a) >> 3;
                shade_strides = ((shade_b - shade_a).wrapping_mul(
                    Pix3D::div_table()
                        .get(strides as usize)
                        .copied()
                        .unwrap_or(0),
                )) >> 6;
            } else {
                strides = 0;
                shade_strides = 0;
            }
        }
        shade_a = shade_a.wrapping_shl(9);
        off += x_a;

        if self.low_mem {
            let mut next_u = 0;
            let mut next_v = 0;
            let mut step_u;
            let mut step_v;
            let mut shade_shift;

            let dx = x_a - self.origin_x;
            u = u.wrapping_add((u_stride >> 3).wrapping_mul(dx));
            v = v.wrapping_add((v_stride >> 3).wrapping_mul(dx));
            w = w.wrapping_add((w_stride >> 3).wrapping_mul(dx));

            let mut cur_w = w >> 12;
            let mut cur_u = 0;
            let mut cur_v = 0;
            if cur_w != 0 {
                cur_u = u.wrapping_div(cur_w);
                cur_v = v.wrapping_div(cur_w);
                if cur_u < 0 {
                    cur_u = 0;
                } else if cur_u > 4032 {
                    cur_u = 4032;
                }
            }

            u = u.wrapping_add(u_stride);
            v = v.wrapping_add(v_stride);
            w = w.wrapping_add(w_stride);

            cur_w = w >> 12;
            if cur_w != 0 {
                next_u = u.wrapping_div(cur_w);
                next_v = v.wrapping_div(cur_w);
                if next_u < 7 {
                    next_u = 7;
                } else if next_u > 4032 {
                    next_u = 4032;
                }
            }

            step_u = (next_u - cur_u) >> 3;
            step_v = next_v.wrapping_sub(cur_v) >> 3;
            cur_u = cur_u.wrapping_add((shade_a >> 3) & 0xc0000);
            shade_shift = shade_a >> 23;

            if self.opaque {
                while strides > 0 {
                    let mut i = 0;
                    while i < 8 {
                        Self::put_pixel(surface, off, Self::texel_low_mem(texels, cur_u, cur_v, shade_shift));
                        off += 1;
                        i += 1;
                        if i < 8 {
                            cur_u = cur_u.wrapping_add(step_u);
                            cur_v = cur_v.wrapping_add(step_v);
                        }
                    }
                    cur_u = next_u;
                    cur_v = next_v;

                    u = u.wrapping_add(u_stride);
                    v = v.wrapping_add(v_stride);
                    w = w.wrapping_add(w_stride);

                    cur_w = w >> 12;
                    if cur_w != 0 {
                        next_u = u.wrapping_div(cur_w);
                        next_v = v.wrapping_div(cur_w);
                        if next_u < 7 {
                            next_u = 7;
                        } else if next_u > 4032 {
                            next_u = 4032;
                        }
                    }
                    step_u = (next_u - cur_u) >> 3;
                    step_v = next_v.wrapping_sub(cur_v) >> 3;
                    shade_a = shade_a.wrapping_add(shade_strides);
                    cur_u = cur_u.wrapping_add((shade_a >> 3) & 0xc0000);
                    shade_shift = shade_a >> 23;
                    strides -= 1;
                }

                strides = (x_b - x_a) & 0x7;
                while strides > 0 {
                    Self::put_pixel(surface, off, Self::texel_low_mem(texels, cur_u, cur_v, shade_shift));
                    off += 1;
                    cur_u = cur_u.wrapping_add(step_u);
                    cur_v = cur_v.wrapping_add(step_v);
                    strides -= 1;
                }
            } else {
                while strides > 0 {
                    let mut i = 0;
                    while i < 8 {
                        let rgb = Self::texel_low_mem(texels, cur_u, cur_v, shade_shift);
                        if rgb != 0 {
                            Self::put_pixel(surface, off, rgb);
                        }
                        off += 1;
                        i += 1;
                        if i < 8 {
                            cur_u = cur_u.wrapping_add(step_u);
                            cur_v = cur_v.wrapping_add(step_v);
                        }
                    }
                    cur_u = next_u;
                    cur_v = next_v;

                    u = u.wrapping_add(u_stride);
                    v = v.wrapping_add(v_stride);
                    w = w.wrapping_add(w_stride);

                    cur_w = w >> 12;
                    if cur_w != 0 {
                        next_u = u.wrapping_div(cur_w);
                        next_v = v.wrapping_div(cur_w);
                        if next_u < 7 {
                            next_u = 7;
                        } else if next_u > 4032 {
                            next_u = 4032;
                        }
                    }
                    step_u = (next_u - cur_u) >> 3;
                    step_v = next_v.wrapping_sub(cur_v) >> 3;
                    shade_a = shade_a.wrapping_add(shade_strides);
                    cur_u = cur_u.wrapping_add((shade_a >> 3) & 0xc0000);
                    shade_shift = shade_a >> 23;
                    strides -= 1;
                }

                strides = (x_b - x_a) & 0x7;
                while strides > 0 {
                    let rgb = Self::texel_low_mem(texels, cur_u, cur_v, shade_shift);
                    if rgb != 0 {
                        Self::put_pixel(surface, off, rgb);
                    }
                    off += 1;
                    cur_u = cur_u.wrapping_add(step_u);
                    cur_v = cur_v.wrapping_add(step_v);
                    strides -= 1;
                }
            }
        } else {
            let mut next_u = 0;
            let mut next_v = 0;
            let mut step_u;
            let mut step_v;
            let mut shade_shift;

            let dx = x_a - self.origin_x;
            u = u.wrapping_add((u_stride >> 3).wrapping_mul(dx));
            v = v.wrapping_add((v_stride >> 3).wrapping_mul(dx));
            w = w.wrapping_add((w_stride >> 3).wrapping_mul(dx));

            let mut cur_w = w >> 14;
            let mut cur_u = 0;
            let mut cur_v = 0;
            if cur_w != 0 {
                cur_u = u.wrapping_div(cur_w);
                cur_v = v.wrapping_div(cur_w);
                if cur_u < 0 {
                    cur_u = 0;
                } else if cur_u > 16256 {
                    cur_u = 16256;
                }
            }

            u = u.wrapping_add(u_stride);
            v = v.wrapping_add(v_stride);
            w = w.wrapping_add(w_stride);

            cur_w = w >> 14;
            if cur_w != 0 {
                next_u = u.wrapping_div(cur_w);
                next_v = v.wrapping_div(cur_w);
                if next_u < 7 {
                    next_u = 7;
                } else if next_u > 16256 {
                    next_u = 16256;
                }
            }

            step_u = (next_u - cur_u) >> 3;
            step_v = next_v.wrapping_sub(cur_v) >> 3;
            cur_u = cur_u.wrapping_add(shade_a & 0x600000);
            shade_shift = shade_a >> 23;

            if self.opaque {
                while strides > 0 {
                    let mut i = 0;
                    while i < 8 {
                        Self::put_pixel(surface, off, Self::texel_high_mem(texels, cur_u, cur_v, shade_shift));
                        off += 1;
                        i += 1;
                        if i < 8 {
                            cur_u = cur_u.wrapping_add(step_u);
                            cur_v = cur_v.wrapping_add(step_v);
                        }
                    }
                    cur_u = next_u;
                    cur_v = next_v;

                    u = u.wrapping_add(u_stride);
                    v = v.wrapping_add(v_stride);
                    w = w.wrapping_add(w_stride);

                    cur_w = w >> 14;
                    if cur_w != 0 {
                        next_u = u.wrapping_div(cur_w);
                        next_v = v.wrapping_div(cur_w);
                        if next_u < 7 {
                            next_u = 7;
                        } else if next_u > 16256 {
                            next_u = 16256;
                        }
                    }
                    step_u = (next_u - cur_u) >> 3;
                    step_v = next_v.wrapping_sub(cur_v) >> 3;
                    shade_a = shade_a.wrapping_add(shade_strides);
                    cur_u = cur_u.wrapping_add(shade_a & 0x600000);
                    shade_shift = shade_a >> 23;
                    strides -= 1;
                }

                strides = (x_b - x_a) & 0x7;
                while strides > 0 {
                    Self::put_pixel(surface, off, Self::texel_high_mem(texels, cur_u, cur_v, shade_shift));
                    off += 1;
                    cur_u = cur_u.wrapping_add(step_u);
                    cur_v = cur_v.wrapping_add(step_v);
                    strides -= 1;
                }
            } else {
                while strides > 0 {
                    let mut i = 0;
                    while i < 8 {
                        let rgb = Self::texel_high_mem(texels, cur_u, cur_v, shade_shift);
                        if rgb != 0 {
                            Self::put_pixel(surface, off, rgb);
                        }
                        off += 1;
                        i += 1;
                        if i < 8 {
                            cur_u = cur_u.wrapping_add(step_u);
                            cur_v = cur_v.wrapping_add(step_v);
                        }
                    }
                    cur_u = next_u;
                    cur_v = next_v;

                    u = u.wrapping_add(u_stride);
                    v = v.wrapping_add(v_stride);
                    w = w.wrapping_add(w_stride);

                    cur_w = w >> 14;
                    if cur_w != 0 {
                        next_u = u.wrapping_div(cur_w);
                        next_v = v.wrapping_div(cur_w);
                        if next_u < 7 {
                            next_u = 7;
                        } else if next_u > 16256 {
                            next_u = 16256;
                        }
                    }
                    step_u = (next_u - cur_u) >> 3;
                    step_v = next_v.wrapping_sub(cur_v) >> 3;
                    shade_a = shade_a.wrapping_add(shade_strides);
                    cur_u = cur_u.wrapping_add(shade_a & 0x600000);
                    shade_shift = shade_a >> 23;
                    strides -= 1;
                }

                strides = (x_b - x_a) & 0x7;
                while strides > 0 {
                    let rgb = Self::texel_high_mem(texels, cur_u, cur_v, shade_shift);
                    if rgb != 0 {
                        Self::put_pixel(surface, off, rgb);
                    }
                    off += 1;
                    cur_u = cur_u.wrapping_add(step_u);
                    cur_v = cur_v.wrapping_add(step_v);
                    strides -= 1;
                }
            }
        }
    }

    /// LowMem texel fetch: 4096-texel texture blocks, shade-selected via the
    /// `shade_a` block offset in `cur_u`. The shift count is masked like TS
    /// `>>>` (mod 32); out-of-range reads follow TS `undefined >>> n` → 0.
    fn texel_low_mem(texels: &[i32], cur_u: i32, cur_v: i32, shade_shift: i32) -> i32 {
        let idx = ((cur_v & 0xfc0) + (cur_u >> 6)) as usize;
        texels.get(idx).copied().unwrap_or(0) >> (shade_shift & 31)
    }

    /// HighMem texel fetch: 16384-texel texture blocks, shade-selected via
    /// the `shade_a` block offset in `cur_u` (TS `undefined >>> n` → 0).
    fn texel_high_mem(texels: &[i32], cur_u: i32, cur_v: i32, shade_shift: i32) -> i32 {
        let idx = ((cur_v & 0x3f80) + (cur_u >> 7)) as usize;
        texels.get(idx).copied().unwrap_or(0) >> (shade_shift & 31)
    }
}
