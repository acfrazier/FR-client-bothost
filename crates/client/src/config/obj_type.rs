// Port of `~/experiments/Server/webclient/src/config/ObjType.ts` (decode,
// reset and `genCert`, plus the wear/head model methods and the 2D
// `getSprite` render).
use std::sync::{Mutex, OnceLock};

use crate::config::Cache;
use crate::dash3d::Model;
use crate::datastruct::LruCache;
use crate::graphics::{Pix2D, Pix32, Pix3D, Pix3DDraw};
use crate::io::{JagFile, Packet};

// Process-wide by design: an LRU of decoded, immutable models shared by
// every client (the TS `modelCache` static). Cache bookkeeping, not
// per-client draw state; eviction is LRU so clients only contend on the lock.
fn model_cache() -> &'static Mutex<LruCache<Model>> {
    static CACHE: OnceLock<Mutex<LruCache<Model>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(50)))
}

/// `ObjType.spriteCache` from the Java oracle (ObjType.java 85): the
/// process-wide 100-entry LRU of rendered 32x32 item sprites (same design
/// as `model_cache`).
fn sprite_cache() -> &'static Mutex<LruCache<Pix32>> {
    static CACHE: OnceLock<Mutex<LruCache<Pix32>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(100)))
}

#[derive(Clone)]
pub struct ObjType {
    pub id: i32,
    pub model: i32,
    pub name: String,
    pub desc: String,
    pub recol_s: Option<Vec<u16>>,
    pub recol_d: Option<Vec<u16>>,
    pub zoom2d: i32,
    pub xan2d: i32,
    pub yan2d: i32,
    pub zan2d: i32,
    pub xof2d: i32,
    pub yof2d: i32,
    pub stackable: bool,
    pub cost: i32,
    pub members: bool,
    pub op: [Option<String>; 5],
    pub iop: [Option<String>; 5],
    pub manwear: i32,
    pub manwear2: i32,
    pub manwear_offset: i32,
    pub womanwear: i32,
    pub womanwear2: i32,
    pub womanwear_offset: i32,
    pub manwear3: i32,
    pub womanwear3: i32,
    pub manhead: i32,
    pub manhead2: i32,
    pub womanhead: i32,
    pub womanhead2: i32,
    pub countobj: Option<Vec<u16>>,
    pub countco: Option<Vec<u16>>,
    pub certlink: i32,
    pub certtemplate: i32,
    pub resizex: i32,
    pub resizey: i32,
    pub resizez: i32,
    pub ambient: i32,
    pub contrast: i32,
}

impl Default for ObjType {
    fn default() -> Self {
        ObjType {
            id: -1,
            model: 0,
            name: String::new(),
            desc: String::new(),
            recol_s: None,
            recol_d: None,
            zoom2d: 2000,
            xan2d: 0,
            yan2d: 0,
            zan2d: 0,
            xof2d: 0,
            yof2d: 0,
            stackable: false,
            cost: 1,
            members: false,
            op: Default::default(),
            iop: Default::default(),
            manwear: -1,
            manwear2: -1,
            manwear_offset: 0,
            womanwear: -1,
            womanwear2: -1,
            womanwear_offset: 0,
            manwear3: -1,
            womanwear3: -1,
            manhead: -1,
            manhead2: -1,
            womanhead: -1,
            womanhead2: -1,
            countobj: None,
            countco: None,
            certlink: -1,
            certtemplate: -1,
            resizex: 128,
            resizey: 128,
            resizez: 128,
            ambient: 0,
            contrast: 0,
        }
    }
}

impl ObjType {
    /// Eager form of the TS `init` + `list(id)`: `obj.idx` offsets the
    /// entries concatenated in `obj.dat` (both files lead with a g2 count,
    /// so entry `id` starts at `2 + sum(idx[0..id])`). The TS `list` also
    /// runs `genCert` for noted objects once the full table exists.
    pub fn unpack(jag: &JagFile) -> Vec<ObjType> {
        let Some(data) = jag.read("obj.dat") else {
            return Vec::new();
        };
        let Some(idx_data) = jag.read("obj.idx") else {
            return Vec::new();
        };
        let mut dat = Packet::new(data);
        let mut idx = Packet::new(idx_data);
        let num = idx.g2();
        let mut list = Vec::with_capacity(num as usize);
        let mut offset = 2usize;
        for id in 0..num {
            let size = idx.g2();
            dat.pos = offset;
            let mut obj = ObjType { id, ..ObjType::default() };
            obj.decode(&mut dat);
            list.push(obj);
            offset += size as usize;
        }
        // id order: a noted template (itself noted) copies pre-genCert state,
        // where the TS lazy `list` would resolve it first
        for i in 0..list.len() {
            if list[i].certtemplate == -1 {
                continue;
            }
            let (template, link) = {
                let ct = list[i].certtemplate as usize;
                let cl = list[i].certlink as usize;
                (list[ct].clone(), list[cl].clone())
            };
            list[i].gen_cert(&template, &link);
        }
        list
    }

    /// `reset()` from client-ts; the field defaults in `Default` already
    /// match, so decode only ever applies deltas on top of `Default`.
    fn decode(&mut self, dat: &mut Packet) {
        loop {
            let code = dat.g1();
            if code == 0 {
                break;
            }
            match code {
                1 => self.model = dat.g2(),
                2 => self.name = dat.gjstr(),
                3 => self.desc = dat.gjstr(),
                4 => self.zoom2d = dat.g2(),
                5 => self.xan2d = dat.g2(),
                6 => self.yan2d = dat.g2(),
                7 => self.xof2d = dat.g2b(),
                8 => self.yof2d = dat.g2b(),
                // 10 skips an unknown two-byte field
                10 => dat.pos += 2,
                11 => self.stackable = true,
                12 => self.cost = dat.g4(),
                16 => self.members = true,
                23 => {
                    self.manwear = dat.g2();
                    self.manwear_offset = dat.g1b();
                }
                24 => self.manwear2 = dat.g2(),
                25 => {
                    self.womanwear = dat.g2();
                    self.womanwear_offset = dat.g1b();
                }
                26 => self.womanwear2 = dat.g2(),
                30..=34 => {
                    let index = (code - 30) as usize;
                    let s = dat.gjstr();
                    self.op[index] = if s.eq_ignore_ascii_case("hidden") {
                        None
                    } else {
                        Some(s)
                    };
                }
                35..=39 => self.iop[(code - 35) as usize] = Some(dat.gjstr()),
                40 => {
                    let count = dat.g1();
                    let mut recol_s = Vec::with_capacity(count as usize);
                    let mut recol_d = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        recol_s.push(dat.g2() as u16);
                        recol_d.push(dat.g2() as u16);
                    }
                    self.recol_s = Some(recol_s);
                    self.recol_d = Some(recol_d);
                }
                78 => self.manwear3 = dat.g2(),
                79 => self.womanwear3 = dat.g2(),
                90 => self.manhead = dat.g2(),
                91 => self.womanhead = dat.g2(),
                92 => self.manhead2 = dat.g2(),
                93 => self.womanhead2 = dat.g2(),
                95 => self.zan2d = dat.g2(),
                97 => self.certlink = dat.g2(),
                98 => self.certtemplate = dat.g2(),
                100..=109 => {
                    // both arrays are allocated together in the TS
                    let countobj = self.countobj.get_or_insert_with(|| vec![0; 10]);
                    let countco = self.countco.get_or_insert_with(|| vec![0; 10]);
                    countobj[(code - 100) as usize] = dat.g2() as u16;
                    countco[(code - 100) as usize] = dat.g2() as u16;
                }
                110 => self.resizex = dat.g2(),
                111 => self.resizey = dat.g2(),
                112 => self.resizez = dat.g2(),
                113 => self.ambient = dat.g1b(),
                114 => self.contrast = dat.g1b() * 5,
                _ => eprintln!("Error unrecognised obj config code: {code}"),
            }
        }
    }

    /// `genCert()` from client-ts: copies render fields from the cert
    /// template and name/desc from the cert link.
    fn gen_cert(&mut self, template: &ObjType, link: &ObjType) {
        self.model = template.model;
        self.zoom2d = template.zoom2d;
        self.xan2d = template.xan2d;
        self.yan2d = template.yan2d;
        self.zan2d = template.zan2d;
        self.xof2d = template.xof2d;
        self.yof2d = template.yof2d;
        self.recol_s = template.recol_s.clone();
        self.recol_d = template.recol_d.clone();

        self.name = link.name.clone();
        self.members = link.members;
        self.cost = link.cost;

        let article = match link.name.chars().next() {
            Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
            _ => "a",
        };
        self.desc = format!("Swap this note at any bank for {article} {}.", link.name);

        self.stackable = true;
    }

    /// `getModelLit(count)` from client-ts.
    pub fn get_model_lit(&self, cache: &Cache, count: i32) -> Option<Model> {
        if let (Some(countobj), Some(countco)) = (&self.countobj, &self.countco) {
            if count > 1 {
                let mut id = -1;
                for i in 0..10 {
                    if count >= countco[i] as i32 && countco[i] != 0 {
                        id = countobj[i] as i32;
                    }
                }
                if id != -1 {
                    return cache.obj(id as usize).get_model_lit(cache, 1);
                }
            }
        }

        {
            let mut cache = model_cache().lock().unwrap();
            if let Some(model) = cache.find(self.id as i64) {
                return Some(model.clone());
            }
        }

        let mut model = Model::load(self.model)?;

        if self.resizex != 128 || self.resizey != 128 || self.resizez != 128 {
            model.resize(self.resizex, self.resizey, self.resizez);
        }

        if let (Some(rs), Some(rd)) = (&self.recol_s, &self.recol_d) {
            for i in 0..rs.len() {
                model.recolour(rs[i] as i32, rd[i] as i32);
            }
        }

        model.calculate_normals(self.ambient + 64, self.contrast + 768, -50, -10, -50, true);
        model.use_aabb_mouse_check = true;

        model_cache().lock().unwrap().put(model.clone(), self.id as i64);
        Some(model)
    }

    /// `ObjType.getSprite(id, outlineRgb, count)` from the Java oracle
    /// (ObjType.java 208-332; the arg order is `id, outlineRgb, count` — the
    /// TS `getSprite(id, count, outline)` swaps outline and count, and Java
    /// wins). Cache hit only when `outline_rgb == 0` and the cached `ohi`
    /// matches the requested count; otherwise the lit model is rendered into
    /// a fresh 32x32 `Pix32` (`get_model_lit(1)`, count-co walk for `count >
    /// 1`, cert-template overlay) and cached when `outline_rgb == 0`. The
    /// caller's `Pix2D` target is untouched (the 32x32 buffer is a private
    /// temporary surface), so only the `Pix3DDraw` raster state (`origin`,
    /// `scanline`, `low_detail`) needs save/restore.
    pub fn get_sprite(
        cache: &Cache,
        pix3d: &mut Pix3DDraw,
        id: i32,
        outline_rgb: i32,
        count: i32,
    ) -> Option<Pix32> {
        if outline_rgb == 0 {
            let mut sprite_cache = sprite_cache().lock().unwrap();
            if let Some(cached) = sprite_cache.find(id as i64) {
                if cached.ohi == count || cached.ohi == -1 {
                    return Some(cached.clone());
                }
                // Java `var4.unlink()` (ObjType.java 210-212): detach the
                // stale node from the hash-table chain so the re-put below
                // is the only table entry for this key (it stays in the LRU
                // history until evicted, as in Java).
                sprite_cache.unlink_key(id as i64);
            }
        }

        let mut obj = cache.objs.get(id as usize)?;
        let mut count = count;
        if obj.countobj.is_none() {
            count = -1;
        }
        if count > 1 {
            let mut variant = -1;
            if let (Some(countobj), Some(countco)) = (&obj.countobj, &obj.countco) {
                for i in 0..10 {
                    if count >= countco[i] as i32 && countco[i] != 0 {
                        variant = countobj[i] as i32;
                    }
                }
            }
            if variant != -1 {
                obj = cache.objs.get(variant as usize)?;
            }
        }

        let model = obj.get_model_lit(cache, 1)?;

        let mut var10 = Pix32::new(32, 32);

        let saved_origin_x = pix3d.origin_x;
        let saved_origin_y = pix3d.origin_y;
        let saved_scanline = std::mem::take(&mut pix3d.scanline);
        let saved_low_detail = pix3d.low_detail;
        pix3d.low_detail = false;

        {
            let mut target = Pix2D::with_pixels(&mut var10.data, 32, 32);
            target.fill_rect(0, 0, 32, 32, 0);
            pix3d.set_render_clipping(&target);

            let mut zoom = obj.zoom2d;
            if outline_rgb == -1 {
                zoom = (zoom as f64 * 1.5) as i32;
            }
            if outline_rgb > 0 {
                zoom = (zoom as f64 * 1.04) as i32;
            }
            let sin_xan = Pix3D::sin_table().get(obj.xan2d as usize).copied().unwrap_or(0);
            let cos_xan = Pix3D::cos_table().get(obj.xan2d as usize).copied().unwrap_or(0);
            let var22 = sin_xan.wrapping_mul(zoom) >> 16;
            let var23 = cos_xan.wrapping_mul(zoom) >> 16;
            model.obj_render(
                pix3d,
                &mut target,
                0,
                obj.yan2d,
                obj.zan2d,
                obj.xan2d,
                obj.xof2d,
                var22 + model.min_y / 2 + obj.yof2d,
                var23 + obj.yof2d,
            );

            // Java 270-282: the 1-value edge pass first, then either the
            // outline-colour or the 3153952 drop-shadow fill. The Java
            // same-body else-if chains collapse to an OR (every branch
            // writes the same pixel value).
            for y in (0..32).rev() {
                for x in (0..32).rev() {
                    let index = x + y * 32;
                    if target.pixels[index] == 0 {
                        let edge = (x > 0 && target.pixels[index - 1] > 1)
                            || (y > 0 && target.pixels[index - 32] > 1)
                            || (x < 31 && target.pixels[index + 1] > 1)
                            || (y < 31 && target.pixels[index + 32] > 1);
                        if edge {
                            target.pixels[index] = 1;
                        }
                    }
                }
            }
            if outline_rgb > 0 {
                for y in (0..32).rev() {
                    for x in (0..32).rev() {
                        let index = x + y * 32;
                        if target.pixels[index] == 0 {
                            let edge = (x > 0 && target.pixels[index - 1] == 1)
                                || (y > 0 && target.pixels[index - 32] == 1)
                                || (x < 31 && target.pixels[index + 1] == 1)
                                || (y < 31 && target.pixels[index + 32] == 1);
                            if edge {
                                target.pixels[index] = outline_rgb;
                            }
                        }
                    }
                }
            } else if outline_rgb == 0 {
                for y in (0..32).rev() {
                    for x in (0..32).rev() {
                        let index = x + y * 32;
                        if target.pixels[index] == 0
                            && x > 0
                            && y > 0
                            && target.pixels[index - 33] > 0
                        {
                            target.pixels[index] = 3153952;
                        }
                    }
                }
            }

            if obj.certtemplate != -1 {
                let cert = Self::get_sprite(cache, pix3d, obj.certlink, -1, 10)?;
                cert.plot_sprite(&mut target, 0, 0);
            }
        }

        if outline_rgb == 0 {
            let mut cached = var10.clone();
            cached.owi = if obj.stackable { 33 } else { 32 };
            cached.ohi = count;
            sprite_cache().lock().unwrap().put(cached, id as i64);
        }

        pix3d.origin_x = saved_origin_x;
        pix3d.origin_y = saved_origin_y;
        pix3d.scanline = saved_scanline;
        pix3d.low_detail = saved_low_detail;

        var10.owi = if obj.stackable { 33 } else { 32 };
        var10.ohi = count;
        Some(var10)
    }

    /// `checkWearModel(gender)` from client-ts.
    pub fn check_wear_model(&self, gender: i32) -> bool {
        let mut wear = self.manwear;
        let mut wear2 = self.manwear2;
        let mut wear3 = self.manwear3;
        if gender == 1 {
            wear = self.womanwear;
            wear2 = self.womanwear2;
            wear3 = self.womanwear3;
        }

        if wear == -1 {
            return true;
        }

        let mut ready = true;
        if !Model::request_download(wear) {
            ready = false;
        }
        if wear2 != -1 && !Model::request_download(wear2) {
            ready = false;
        }
        if wear3 != -1 && !Model::request_download(wear3) {
            ready = false;
        }
        ready
    }

    /// `getWearModelNoCheck(gender)` from client-ts.
    pub fn get_wear_model_no_check(&self, gender: i32) -> Option<Model> {
        let mut id1 = self.manwear;
        if gender == 1 {
            id1 = self.womanwear;
        }

        if id1 == -1 {
            return None;
        }

        let mut id2 = self.manwear2;
        let mut id3 = self.manwear3;
        if gender == 1 {
            id2 = self.womanwear2;
            id3 = self.womanwear3;
        }

        let mut model = Model::load(id1)?;

        if id2 != -1 {
            let model2 = Model::load(id2)?;

            if id3 == -1 {
                let models = [Some(model), Some(model2)];
                model = Model::combine_for_anim(&models, 2);
            } else {
                let model3 = Model::load(id3)?;
                let models = [Some(model), Some(model2), Some(model3)];
                model = Model::combine_for_anim(&models, 3);
            }
        }

        if gender == 0 && self.manwear_offset != 0 {
            model.translate(self.manwear_offset, 0, 0);
        } else if gender == 1 && self.womanwear_offset != 0 {
            model.translate(self.womanwear_offset, 0, 0);
        }

        if let (Some(rs), Some(rd)) = (&self.recol_s, &self.recol_d) {
            for i in 0..rs.len() {
                model.recolour(rs[i] as i32, rd[i] as i32);
            }
        }

        Some(model)
    }

    /// `checkHeadModel(gender)` from client-ts.
    pub fn check_head_model(&self, gender: i32) -> bool {
        let mut head = self.manhead;
        let mut head2 = self.manhead2;
        if gender == 1 {
            head = self.womanhead;
            head2 = self.womanhead2;
        }

        if head == -1 {
            return true;
        }

        let mut ready = true;
        if !Model::request_download(head) {
            ready = false;
        }
        if head2 != -1 && !Model::request_download(head2) {
            ready = false;
        }
        ready
    }

    /// `getHeadModelNoCheck(gender)` from client-ts.
    pub fn get_head_model_no_check(&self, gender: i32) -> Option<Model> {
        let mut head1 = self.manhead;
        if gender == 1 {
            head1 = self.womanhead;
        }

        if head1 == -1 {
            return None;
        }

        let mut head2 = self.manhead2;
        if gender == 1 {
            head2 = self.womanhead2;
        }

        let mut model = Model::load(head1)?;

        if head2 != -1 {
            let model2 = Model::load(head2)?;
            let models = [Some(model), Some(model2)];
            model = Model::combine_for_anim(&models, 2);
        }

        if let (Some(rs), Some(rd)) = (&self.recol_s, &self.recol_d) {
            for i in 0..rs.len() {
                model.recolour(rs[i] as i32, rd[i] as i32);
            }
        }

        Some(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    /// The sprite/model caches are process-wide statics shared by every test
    /// in this binary, so tests that clear them or depend on their contents
    /// must not interleave (a concurrent clear would evict a hit mid-test).
    /// A failed test poisons the lock; recover so one failure does not
    /// cascade into the rest.
    static CACHE_LOCK: Mutex<()> = Mutex::new(());

    fn lock_caches() -> MutexGuard<'static, ()> {
        CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One 3-vertex, 1-face triangle: v0=(0,40,0), v1=(40,40,0),
    /// v2=(0,0,0). Under `get_sprite` (zoom2d 2000, eye_z 2000, identity
    /// rotations) it projects to screen (16,26)/(26,26)/(16,16) — a ~10x10
    /// front-facing triangle fully inside the 32x32 sprite — with the face
    /// normal (-z) pointing at the light so the shade is bright.
    const MODEL: &[u8] = &[
        7, 7, 7, // vertex order: x+y+z deltas for each of 3 vertices
        1, // face index order: a,b,c are all deltas
        0x40, 0x41, 0x41, // face index deltas: a=0, b=1, c=2 (cumulative)
        0x00, 0xFF, // face colour (HSL 255)
        0x40, 0x68, 0x18, // vertexX deltas: 0, +40, -40
        0x68, 0x40, 0x18, // vertexY deltas: +40, 0, -40
        0x40, 0x40, 0x40, // vertexZ deltas: 0, 0, 0
        0, 3, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 3, 0, 3, 0, 3, // trailer
    ];

    fn reset_caches() {
        sprite_cache().lock().unwrap().clear();
        model_cache().lock().unwrap().clear();
        Model::unpack(0, Some(MODEL));
    }

    fn pix3d() -> Pix3DDraw {
        Pix3D::init_colour_table(0.8);
        Pix3DDraw::default()
    }

    /// A `Cache` with every slot up to `len` holding an obj whose id equals
    /// its index and whose model is the synthetic triangle.
    fn cache_of(len: usize) -> Cache {
        let mut cache = Cache::default();
        cache.objs.resize(len, ObjType::default());
        for (i, obj) in cache.objs.iter_mut().enumerate() {
            *obj = ObjType {
                id: i as i32,
                model: 0,
                countobj: Some(vec![0; 10]),
                ..ObjType::default()
            };
        }
        cache
    }

    #[test]
    fn get_sprite_resolves_owi_ohi_and_count_variants() {
        let _guard = lock_caches();
        reset_caches();
        let mut cache = cache_of(100);
        let mut pix = pix3d();

        // non-stackable without countobj: `countobj == null` forces count -1
        cache.objs[10] = ObjType {
            id: 10,
            model: 0,
            countobj: None,
            ..ObjType::default()
        };
        let s = ObjType::get_sprite(&cache, &mut pix, 10, 0, 5).unwrap();
        assert_eq!(s.owi, 32, "non-stackable -> owi 32");
        assert_eq!(s.ohi, -1, "countobj null forces ohi -1 regardless of the request");

        // stackable with countobj: owi 33, ohi = the requested count
        cache.objs[11].stackable = true;
        let s = ObjType::get_sprite(&cache, &mut pix, 11, 0, 1).unwrap();
        assert_eq!(s.owi, 33, "stackable -> owi 33");
        assert_eq!(s.ohi, 1, "ohi carries the requested count");

        // countco walk: base (non-stackable, countco[0]=5) with count 5
        // resolves the stackable variant 12; owi comes from the *resolved*
        // obj while ohi stays the requested count
        cache.objs[12].stackable = true;
        let mut countobj = vec![0u16; 10];
        let mut countco = vec![0u16; 10];
        countobj[0] = 12;
        countco[0] = 5;
        cache.objs[13] = ObjType {
            id: 13,
            model: 0,
            stackable: false,
            countobj: Some(countobj),
            countco: Some(countco),
            ..ObjType::default()
        };
        let s = ObjType::get_sprite(&cache, &mut pix, 13, 0, 5).unwrap();
        assert_eq!(s.owi, 33, "count >= threshold resolves the variant; owi from the resolved obj");
        assert_eq!(s.ohi, 5, "ohi stays the requested count");
    }

    #[test]
    fn sprite_cache_hits_same_key_and_unlinks_stale_counts() {
        let _guard = lock_caches();
        reset_caches();
        let cache = cache_of(100);
        let mut pix = pix3d();

        // same id/count: the second call must be a sprite-cache hit, so it
        // must not touch the model at all — with the model dropped from the
        // store (and the model cache cleared), a re-render would fail.
        assert!(ObjType::get_sprite(&cache, &mut pix, 20, 0, 5).is_some());
        model_cache().lock().unwrap().clear();
        Model::unload(0);
        let hit = ObjType::get_sprite(&cache, &mut pix, 20, 0, 5);
        assert!(hit.is_some(), "same id/count must hit the sprite cache, not re-render");
        assert_eq!(hit.unwrap().ohi, 5);
        assert!(
            ObjType::get_sprite(&cache, &mut pix, 21, 0, 5).is_none(),
            "a different key is a miss and must need the model"
        );

        // different count: the stale ohi=5 node must be unlinked before the
        // re-put, so the bucket chain holds only the ohi=10 entry (Java
        // `Linkable.unlink()` at ObjType.java 210-212). Without the unlink
        // the appended node is shadowed by the stale head of the chain and
        // this read returns 5.
        Model::unpack(0, Some(MODEL));
        assert!(ObjType::get_sprite(&cache, &mut pix, 22, 0, 5).is_some());
        assert!(ObjType::get_sprite(&cache, &mut pix, 22, 0, 10).is_some());
        let cached_ohi = sprite_cache().lock().unwrap().find(22).map(|s| s.ohi).unwrap();
        assert_eq!(cached_ohi, 10, "the ohi=5 node must not shadow the re-put");
    }

    #[test]
    fn sprite_cache_evicts_at_capacity_100() {
        let _guard = lock_caches();
        reset_caches();
        let cache = cache_of(150);
        let mut pix = pix3d();

        // capacity 100: the 101st put evicts the least-recently-used entry
        for i in 0..101 {
            assert!(
                ObjType::get_sprite(&cache, &mut pix, i, 0, 1).is_some(),
                "render for obj {i}"
            );
        }
        let mut sprite_cache = sprite_cache().lock().unwrap();
        assert!(sprite_cache.find(0).is_none(), "the first-put (LRU) entry must be evicted");
        assert!(sprite_cache.find(100).is_some(), "the most recent entry stays");
    }

    #[test]
    fn sprite_drop_shadow_only_when_outline_zero() {
        let _guard = lock_caches();
        reset_caches();
        let cache = cache_of(100);
        let mut pix = pix3d();

        // outline 0: the model must rasterise, and the `3153952` drop-shadow
        // fill applies (Java ObjType.java 290-306)
        let s0 = ObjType::get_sprite(&cache, &mut pix, 30, 0, 1).unwrap();
        assert!(s0.data.iter().any(|&p| p != 0), "the model must rasterise into the sprite");
        assert!(s0.data.contains(&3153952), "outline 0 draws the 3153952 drop shadow");

        // outline -1 (the cert-template zoom 1.5x): still renders, but the
        // `else if outline_rgb == 0` shadow fill is skipped
        let s1 = ObjType::get_sprite(&cache, &mut pix, 30, -1, 1).unwrap();
        assert!(s1.data.iter().any(|&p| p != 0), "outline -1 still renders at 1.5x zoom");
        assert!(!s1.data.contains(&3153952), "outline -1 must skip the 3153952 fill");
    }
}

