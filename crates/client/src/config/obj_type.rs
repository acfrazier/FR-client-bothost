// Port of `~/experiments/Server/webclient/src/config/ObjType.ts` (decode,
// reset and `genCert`, plus the wear/head model methods; the sprite methods
// need `graphics` and land with the render task).
use std::sync::{Mutex, OnceLock};

use crate::config::Cache;
use crate::dash3d::Model;
use crate::datastruct::LruCache;
use crate::io::{JagFile, Packet};

fn model_cache() -> &'static Mutex<LruCache<Model>> {
    static CACHE: OnceLock<Mutex<LruCache<Model>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(50)))
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
