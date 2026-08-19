// Port of `~/experiments/Server/webclient/src/config/NpcType.ts`. The eager
// `unpack` replaces the lazy `init` + `list(id)` ring buffer: `npc.idx`
// offsets the entries concatenated in `npc.dat` (both files lead with a g2
// count, so entry `id` starts at `2 + sum(idx[0..id])`).
use std::sync::{Mutex, OnceLock};

use crate::config::Cache;
use crate::dash3d::{AnimFrame, Model};
use crate::datastruct::LruCache;
use crate::io::{JagFile, Packet};

fn model_cache() -> &'static Mutex<LruCache<Model>> {
    static CACHE: OnceLock<Mutex<LruCache<Model>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(30)))
}

pub struct NpcType {
    pub id: i32,
    pub name: String,
    pub desc: String,
    pub size: i32,
    pub model: Option<Vec<i32>>,
    pub head: Option<Vec<i32>>,
    pub readyanim: i32,
    pub walkanim: i32,
    pub walkanim_b: i32,
    pub walkanim_r: i32,
    pub walkanim_l: i32,
    pub recol_s: Option<Vec<u16>>,
    pub recol_d: Option<Vec<u16>>,
    pub op: Vec<Option<String>>,
    pub minimap: bool,
    pub vislevel: i32,
    pub resizeh: i32,
    pub resizev: i32,
    pub alwaysontop: bool,
    pub ambient: i32,
    pub contrast: i32,
    pub headicon: i32,
    pub turnspeed: i32,
}

impl Default for NpcType {
    fn default() -> Self {
        NpcType {
            id: -1,
            name: String::new(),
            desc: String::new(),
            size: 1,
            model: None,
            head: None,
            readyanim: -1,
            walkanim: -1,
            walkanim_b: -1,
            walkanim_r: -1,
            walkanim_l: -1,
            recol_s: None,
            recol_d: None,
            op: Vec::new(),
            minimap: true,
            vislevel: -1,
            resizeh: 128,
            resizev: 128,
            alwaysontop: false,
            ambient: 0,
            contrast: 0,
            headicon: -1,
            turnspeed: 32,
        }
    }
}

impl NpcType {
    pub fn unpack(jag: &JagFile) -> Vec<NpcType> {
        let Some(data) = jag.read("npc.dat") else {
            return Vec::new();
        };
        let Some(idx_data) = jag.read("npc.idx") else {
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
            let mut npc = NpcType { id, ..NpcType::default() };
            npc.decode(&mut dat);
            list.push(npc);
            offset += size as usize;
        }
        list
    }

    fn decode(&mut self, dat: &mut Packet) {
        loop {
            let code = dat.g1();
            if code == 0 {
                break;
            }
            match code {
                1 => {
                    let count = dat.g1();
                    let mut model = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        model.push(dat.g2());
                    }
                    self.model = Some(model);
                }
                2 => self.name = dat.gjstr(),
                3 => self.desc = dat.gjstr(),
                12 => self.size = dat.g1b(),
                13 => self.readyanim = dat.g2(),
                14 => self.walkanim = dat.g2(),
                17 => {
                    self.walkanim = dat.g2();
                    self.walkanim_b = dat.g2();
                    self.walkanim_r = dat.g2();
                    self.walkanim_l = dat.g2();
                }
                30..=39 => {
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
                60 => {
                    let count = dat.g1();
                    let mut head = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        head.push(dat.g2());
                    }
                    self.head = Some(head);
                }
                // 90, 91, 92 are unused client-side model fields
                90..=92 => dat.pos += 2,
                93 => self.minimap = false,
                95 => self.vislevel = dat.g2(),
                97 => self.resizeh = dat.g2(),
                98 => self.resizev = dat.g2(),
                99 => self.alwaysontop = true,
                100 => self.ambient = dat.g1b(),
                101 => self.contrast = dat.g1b() * 5,
                102 => self.headicon = dat.g2(),
                103 => self.turnspeed = dat.g2(),
                _ => eprintln!("Error unrecognised npc config code: {code}"),
            }
        }
    }

    /// `getTempModel(primaryTransformId, secondaryTransformId, seqMask)`
    /// from client-ts.
    pub fn get_temp_model(
        &self,
        _cache: &Cache,
        primary_transform_id: i32,
        secondary_transform_id: i32,
        seq_mask: Option<&[i32]>,
    ) -> Option<Model> {
        let base = {
            let mut model_cache = model_cache().lock().unwrap();
            model_cache.find(self.id as i64).map(|m| m.clone())
        };

        let base = match base {
            Some(model) => Some(model),
            None => {
                let models = self.model.as_ref()?;
                let mut ready = false;
                for &m in models {
                    if !Model::request_download(m) {
                        ready = true;
                    }
                }
                if ready {
                    return None;
                }

                let mut loaded: Vec<Option<Model>> = Vec::with_capacity(models.len());
                for &m in models {
                    loaded.push(Model::load(m));
                }

                let mut model = if models.len() == 1 {
                    loaded.into_iter().next().flatten()
                } else {
                    Some(Model::combine_for_anim(&loaded, loaded.len()))
                };

                if let Some(model) = model.as_mut() {
                    if let (Some(rs), Some(rd)) = (&self.recol_s, &self.recol_d) {
                        for i in 0..rs.len() {
                            model.recolour(rs[i] as i32, rd[i] as i32);
                        }
                    }

                    model.prepare_anim();
                    model.calculate_normals(64, 850, -30, -50, -30, true);
                    model_cache().lock().unwrap().put(model.clone(), self.id as i64);
                }
                model
            }
        };

        let base = base?;

        let mut tmp = Model::temp_model();
        tmp.set(
            &base,
            AnimFrame::animate_transparencies(primary_transform_id)
                && AnimFrame::animate_transparencies(secondary_transform_id),
        );

        if primary_transform_id != -1 && secondary_transform_id != -1 {
            tmp.mask_animate(primary_transform_id, secondary_transform_id, seq_mask);
        } else if primary_transform_id != -1 {
            tmp.animate(primary_transform_id);
        }

        if self.resizeh != 128 || self.resizev != 128 {
            tmp.resize(self.resizeh, self.resizev, self.resizeh);
        }

        tmp.calc_bounding_cylinder();
        tmp.label_faces = None;
        tmp.label_vertices = None;

        if self.size == 1 {
            tmp.use_aabb_mouse_check = true;
        }

        Some(tmp)
    }
}
