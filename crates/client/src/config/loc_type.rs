// Port of `~/experiments/Server/webclient/src/config/LocType.ts` (decode and
// init only; `buildModel`/`getModel` need `dash3d` and land with Task 15).
use crate::io::{JagFile, Packet};

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
}
