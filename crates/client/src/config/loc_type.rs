// Port of `~/experiments/Server/webclient/src/config/LocType.ts` (decode and
// init plus the `getModel`/`buildModel` loc-model pipeline that
// `REBUILD_NORMAL` loc placement consumes). The TS statics `mc1`/`mc2` are
// process-wide `Mutex<LruCache>`es so the port stays `Send`.
use std::sync::{Mutex, OnceLock};

use crate::config::Cache;
use crate::dash3d::loc_angle::LocAngle;
use crate::dash3d::LocShape;
use crate::dash3d::{AnimFrame, Model};
use crate::datastruct::LruCache;
use crate::io::{JagFile, Packet};

// Process-wide by design: LRUs of decoded, immutable loc models shared by
// every client (the TS `mc1`/`mc2` statics). Cache bookkeeping, not
// per-client draw state; eviction is LRU so clients only contend on the lock.
fn mc1() -> &'static Mutex<LruCache<Model>> {
    static CACHE: OnceLock<Mutex<LruCache<Model>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(500)))
}

fn mc2() -> &'static Mutex<LruCache<Model>> {
    static CACHE: OnceLock<Mutex<LruCache<Model>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(30)))
}

/// `LocShape.CENTREPIECE_STRAIGHT` from `dash3d/LocShape.ts`; the only shape
/// the `active` fixup compares against.
const CENTREPIECE_STRAIGHT: i32 = 10;

pub struct LocType {
    pub id: i32,
    pub model: Option<Vec<i32>>,
    pub shape: Option<Vec<i32>>,
    pub name: String,
    pub desc: String,
    pub recol_s: Option<Vec<u16>>,
    pub recol_d: Option<Vec<u16>>,
    pub width: i32,
    pub length: i32,
    pub blockwalk: bool,
    pub blockrange: bool,
    pub active: bool,
    pub hillskew: bool,
    pub sharelight: bool,
    pub occlude: bool,
    pub anim: i32,
    pub wallwidth: i32,
    pub ambient: i32,
    pub contrast: i32,
    pub op: Vec<Option<String>>,
    pub mapfunction: i32,
    pub mapscene: i32,
    pub mirror: bool,
    pub shadow: bool,
    pub resizex: i32,
    pub resizey: i32,
    pub resizez: i32,
    pub offsetx: i32,
    pub offsety: i32,
    pub offsetz: i32,
    pub forceapproach: i32,
    pub forcedecor: bool,
    pub breakroutefinding: bool,
    pub raiseobject: i32,
}

impl Default for LocType {
    fn default() -> Self {
        LocType {
            id: -1,
            model: None,
            shape: None,
            name: String::new(),
            desc: String::new(),
            recol_s: None,
            recol_d: None,
            width: 1,
            length: 1,
            blockwalk: true,
            blockrange: true,
            active: false,
            hillskew: false,
            sharelight: false,
            occlude: false,
            anim: -1,
            wallwidth: 16,
            ambient: 0,
            contrast: 0,
            op: Vec::new(),
            mapfunction: -1,
            mapscene: -1,
            mirror: false,
            shadow: true,
            resizex: 128,
            resizey: 128,
            resizez: 128,
            offsetx: 0,
            offsety: 0,
            offsetz: 0,
            forceapproach: 0,
            forcedecor: false,
            breakroutefinding: false,
            raiseobject: -1,
        }
    }
}

impl LocType {
    pub fn unpack(jag: &JagFile) -> Vec<LocType> {
        let Some(data) = jag.read("loc.dat") else {
            return Vec::new();
        };
        let Some(idx_data) = jag.read("loc.idx") else {
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
            let mut loc = LocType { id, ..LocType::default() };
            loc.decode(&mut dat);
            list.push(loc);
            offset += size as usize;
        }
        list
    }

    fn decode(&mut self, dat: &mut Packet) {
        let mut active = -1;
        loop {
            let code = dat.g1();
            if code == 0 {
                break;
            }
            match code {
                1 => {
                    let count = dat.g1();
                    let mut model = Vec::with_capacity(count as usize);
                    let mut shape = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        model.push(dat.g2());
                        shape.push(dat.g1());
                    }
                    self.model = Some(model);
                    self.shape = Some(shape);
                }
                2 => self.name = dat.gjstr(),
                3 => self.desc = dat.gjstr(),
                5 => {
                    let count = dat.g1();
                    let mut model = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        model.push(dat.g2());
                    }
                    self.model = Some(model);
                    self.shape = None;
                }
                14 => self.width = dat.g1(),
                15 => self.length = dat.g1(),
                17 => self.blockwalk = false,
                18 => self.blockrange = false,
                19 => {
                    active = dat.g1();
                    if active == 1 {
                        self.active = true;
                    }
                }
                21 => self.hillskew = true,
                22 => self.sharelight = true,
                23 => self.occlude = true,
                24 => {
                    self.anim = dat.g2();
                    if self.anim == 65535 {
                        self.anim = -1;
                    }
                }
                28 => self.wallwidth = dat.g1(),
                29 => self.ambient = dat.g1b(),
                39 => self.contrast = dat.g1b(),
                30..=38 => {
                    // TS array grows past its initial 5 slots on write
                    let index = (code - 30) as usize;
                    if self.op.len() <= index {
                        self.op.resize(index + 1, None);
                    }
                    let s = dat.gjstr();
                    self.op[index] = if s.eq_ignore_ascii_case("hidden") {
                        None
                    } else {
                        Some(s)
                    };
                }
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
                60 => self.mapfunction = dat.g2(),
                62 => self.mirror = true,
                64 => self.shadow = false,
                65 => self.resizex = dat.g2(),
                66 => self.resizey = dat.g2(),
                67 => self.resizez = dat.g2(),
                68 => self.mapscene = dat.g2(),
                69 => self.forceapproach = dat.g1(),
                70 => self.offsetx = dat.g2b(),
                71 => self.offsety = dat.g2b(),
                72 => self.offsetz = dat.g2b(),
                73 => self.forcedecor = true,
                74 => self.breakroutefinding = true,
                75 => self.raiseobject = dat.g1(),
                _ => eprintln!("Error unrecognised loc config code: {code}"),
            }
        }

        if active == -1 {
            self.active = false;
            let straight = self
                .shape
                .as_ref()
                .map(|s| s.first() == Some(&CENTREPIECE_STRAIGHT))
                .unwrap_or(false);
            if self.model.is_some() && (!self.shape.is_some() || straight) {
                self.active = true;
            }
            if !self.op.is_empty() {
                self.active = true;
            }
        }

        if self.breakroutefinding {
            self.blockwalk = false;
            self.blockrange = false;
        }

        if self.raiseobject == -1 {
            self.raiseobject = if self.blockwalk { 1 } else { 0 };
        }
    }

    /// `getModel(shape, angle, heightSW, heightSE, heightNE, heightNW,
    /// transformId)` from client-ts.
    #[allow(clippy::too_many_arguments)]
    pub fn get_model(
        &self,
        _cache: &Cache,
        shape: i32,
        angle: i32,
        height_sw: i32,
        height_se: i32,
        height_ne: i32,
        height_nw: i32,
        transform_id: i32,
    ) -> Option<Model> {
        let mut modified = self.build_model(shape, angle, transform_id)?;

        if self.hillskew || self.sharelight {
            modified = Model::hill_skew_copy(&modified, self.hillskew, self.sharelight);
        }

        if self.hillskew {
            let ground_y = (height_sw + height_se + height_ne + height_nw) / 4;

            let points = modified.point_y.as_ref()?;
            let mut points = points.clone();
            let num_points = modified.num_points as usize;
            let x = modified.point_x.as_ref()?;
            let z = modified.point_z.as_ref()?;

            for i in 0..num_points {
                let height_s = height_sw + (((height_se - height_sw) * (x[i] + 64)) / 128);
                let height_n = height_nw + (((height_ne - height_nw) * (x[i] + 64)) / 128);
                let y = height_s + (((height_n - height_s) * (z[i] + 64)) / 128);
                points[i] += y - ground_y;
            }

            modified.point_y = Some(points);
            modified.recalc_bounding_cylinder();
        }

        Some(modified)
    }

    /// `buildModel(shape, angle, transformId)` from client-ts. `typecode`
    /// (the `mc2` key) is computed once up front; the TS computes it in each
    /// branch with the same result.
    fn build_model(&self, shape: i32, angle: i32, transform_id: i32) -> Option<Model> {
        let typecode = if let Some(shapes) = &self.shape {
            let mut index = 0i64;
            for (i, &s) in shapes.iter().enumerate() {
                if s == shape {
                    index = i as i64;
                    break;
                }
            }
            (((transform_id as i64) + 1) << 32)
                + ((self.id as i64) << 6)
                + (index << 3)
                + angle as i64
        } else {
            (((transform_id as i64) + 1) << 32) + ((self.id as i64) << 6) + angle as i64
        };

        let mut model: Option<Model> = None;

        if self.shape.is_none() {
            if shape != LocShape::CENTREPIECE_STRAIGHT {
                return None;
            }

            {
                let mut cache = mc2().lock().unwrap();
                if let Some(cached) = cache.find(typecode) {
                    return Some(cached.clone());
                }
            }

            let model_ids = self.model.as_ref()?;
            let model_count = model_ids.len();
            let flip = self.mirror != (angle > 3);
            let mut temp: Vec<Option<Model>> = Vec::new();

            for mut model_id in model_ids.iter().copied() {
                if flip {
                    model_id += 65536;
                }

                let loaded = {
                    let mut cache = mc1().lock().unwrap();
                    if let Some(m) = cache.find(model_id as i64) {
                        m.clone()
                    } else {
                        let mut m = Model::load(model_id & 0xffff)?;
                        if flip {
                            m.mirror();
                        }
                        cache.put(m.clone(), model_id as i64);
                        m
                    }
                };

                if model_count > 1 {
                    temp.push(Some(loaded));
                } else {
                    model = Some(loaded);
                }
            }

            if model_count > 1 {
                model = Some(Model::combine_for_anim(&temp, model_count));
            }
        } else {
            let mut index = -1;
            let shapes = self.shape.as_ref()?;
            for (i, &s) in shapes.iter().enumerate() {
                if s == shape {
                    index = i as i32;
                    break;
                }
            }
            if index == -1 {
                return None;
            }

            {
                let mut cache = mc2().lock().unwrap();
                if let Some(cached) = cache.find(typecode) {
                    return Some(cached.clone());
                }
            }

            let model_ids = self.model.as_ref()?;
            if index as usize >= model_ids.len() {
                return None;
            }

            let mut model_id = model_ids[index as usize];
            if model_id == -1 {
                return None;
            }

            let flip = self.mirror != (angle > 3);
            if flip {
                model_id += 65536;
            }

            {
                let mut cache = mc1().lock().unwrap();
                if let Some(m) = cache.find(model_id as i64) {
                    model = Some(m.clone());
                } else {
                    let mut m = Model::load(model_id & 0xffff)?;
                    if flip {
                        m.mirror();
                    }
                    cache.put(m.clone(), model_id as i64);
                    model = Some(m);
                }
            }
        }

        let model = model?;

        let scaled = self.resizex != 128 || self.resizey != 128 || self.resizez != 128;
        let translated = self.offsetx != 0 || self.offsety != 0 || self.offsetz != 0;

        let mut modified = Model::copy_for_anim(
            &model,
            self.recol_s.is_none(),
            AnimFrame::animate_transparencies(transform_id),
            angle == LocAngle::WEST && transform_id == -1 && !scaled && !translated,
        );

        if transform_id != -1 {
            modified.prepare_anim();
            modified.animate(transform_id);
            modified.label_faces = None;
            modified.label_vertices = None;
        }

        for _ in 0..angle {
            modified.rotate90();
        }

        if let (Some(rs), Some(rd)) = (&self.recol_s, &self.recol_d) {
            for i in 0..rs.len() {
                modified.recolour(rs[i] as i32, rd[i] as i32);
            }
        }

        if scaled {
            modified.resize(self.resizex, self.resizey, self.resizez);
        }

        if translated {
            modified.translate(self.offsety, self.offsetx, self.offsetz);
        }

        modified.calculate_normals(
            (self.ambient & 0xff) + 64,
            (self.contrast & 0xff) * 5 + 768,
            -50,
            -10,
            -50,
            !self.sharelight,
        );

        if self.raiseobject == 1 {
            modified.obj_raise = modified.min_y;
        }

        mc2().lock().unwrap().put(modified.clone(), typecode);
        Some(modified)
    }
}
