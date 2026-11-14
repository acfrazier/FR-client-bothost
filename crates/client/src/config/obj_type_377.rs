//! Revision-377 object cache records.
//!
//! Java source anchors (relative to the pinned client-java checkout):
//! `src/main/java/jagex2/config/ObjType.java:646-687` (reset defaults),
//! `ObjType.java:338-445` (record opcodes), `ObjType.java:179-203` and
//! `ObjType.java:262-285` (get-time certificate and membership postpasses).

use super::{DecodeError377, DecodeErrorKind377, Reader377};

#[derive(Debug, PartialEq)]
pub struct ObjType377 {
    /// Cache record number; Java stores it in `field845`.
    pub id: i32,
    pub model: i32,
    pub name: Option<String>,
    /// Java `field810` is a nullable raw `gjstrraw()` byte array.
    pub desc: Option<Vec<u8>>,
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
    /// Java `field820` / `field830`, null until their corresponding opcode.
    pub op: Option<[Option<String>; 5]>,
    pub iop: Option<[Option<String>; 5]>,
    pub manwear: i32,
    pub manwear2: i32,
    pub manwear_offset: i32,
    pub womanwear: i32,
    pub womanwear2: i32,
    pub womanwear_offset: i32,
    pub manwear3: i32,
    pub womanwear3: i32,
    pub manhead: i32,
    pub womanhead: i32,
    pub manhead2: i32,
    pub womanhead2: i32,
    /// Java allocates fixed ten-element arrays together on the first count opcode.
    pub countobj: Option<[u16; 10]>,
    pub countco: Option<[u16; 10]>,
    pub certlink: i32,
    pub certtemplate: i32,
    pub resizex: i32,
    pub resizey: i32,
    pub resizez: i32,
    pub ambient: i32,
    pub contrast: i32,
    pub team: i32,
    /// Java `field854`, consumed by opcode 10 and reset to -1.
    pub field_854: i32,
}

impl Default for ObjType377 {
    fn default() -> Self {
        Self {
            id: -1,
            model: 0,
            name: None,
            desc: None,
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
            op: None,
            iop: None,
            manwear: -1,
            manwear2: -1,
            manwear_offset: 0,
            womanwear: -1,
            womanwear2: -1,
            womanwear_offset: 0,
            manwear3: -1,
            womanwear3: -1,
            manhead: -1,
            womanhead: -1,
            manhead2: -1,
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
            team: 0,
            field_854: -1,
        }
    }
}

impl ObjType377 {
    /// Decode one complete `obj.dat` record. The record ordinal supplies its id.
    pub fn decode(reader: &mut Reader377<'_>) -> Result<Self, DecodeError377> {
        let mut obj = Self {
            id: reader.record as i32,
            ..Self::default()
        };

        loop {
            let opcode = reader.g1()?;
            if opcode == 0 {
                break;
            }
            match opcode {
                1 => obj.model = reader.g2()?,
                2 => obj.name = Some(reader.gjstr()?),
                3 => obj.desc = Some(reader.gjstrraw()?),
                4 => obj.zoom2d = reader.g2()?,
                5 => obj.xan2d = reader.g2()?,
                6 => obj.yan2d = reader.g2()?,
                7 => obj.xof2d = reader.g2b()?,
                8 => obj.yof2d = reader.g2b()?,
                10 => obj.field_854 = reader.g2()?,
                11 => obj.stackable = true,
                12 => obj.cost = reader.g4()?,
                16 => obj.members = true,
                23 => {
                    obj.manwear = reader.g2()?;
                    obj.manwear_offset = reader.g1b()?;
                }
                24 => obj.manwear2 = reader.g2()?,
                25 => {
                    obj.womanwear = reader.g2()?;
                    obj.womanwear_offset = reader.g1b()?;
                }
                26 => obj.womanwear2 = reader.g2()?,
                30..=34 => {
                    let action = reader.gjstr()?;
                    let index = (opcode - 30) as usize;
                    let actions = obj.op.get_or_insert_with(|| std::array::from_fn(|_| None));
                    actions[index] = if action.eq_ignore_ascii_case("hidden") {
                        None
                    } else {
                        Some(action)
                    };
                }
                35..=39 => {
                    let action = reader.gjstr()?;
                    let index = (opcode - 35) as usize;
                    let actions = obj.iop.get_or_insert_with(|| std::array::from_fn(|_| None));
                    actions[index] = Some(action);
                }
                40 => {
                    let count = reader.g1()? as usize;
                    let mut recol_s = Vec::with_capacity(count);
                    let mut recol_d = Vec::with_capacity(count);
                    for _ in 0..count {
                        recol_s.push(reader.g2()? as u16);
                        recol_d.push(reader.g2()? as u16);
                    }
                    obj.recol_s = Some(recol_s);
                    obj.recol_d = Some(recol_d);
                }
                78 => obj.manwear3 = reader.g2()?,
                79 => obj.womanwear3 = reader.g2()?,
                90 => obj.manhead = reader.g2()?,
                91 => obj.womanhead = reader.g2()?,
                92 => obj.manhead2 = reader.g2()?,
                93 => obj.womanhead2 = reader.g2()?,
                95 => obj.zan2d = reader.g2()?,
                97 => obj.certlink = reader.g2()?,
                98 => obj.certtemplate = reader.g2()?,
                100..=109 => {
                    let index = (opcode - 100) as usize;
                    let countobj = obj.countobj.get_or_insert([0; 10]);
                    let countco = obj.countco.get_or_insert([0; 10]);
                    countobj[index] = reader.g2()? as u16;
                    countco[index] = reader.g2()? as u16;
                }
                110 => obj.resizex = reader.g2()?,
                111 => obj.resizey = reader.g2()?,
                112 => obj.resizez = reader.g2()?,
                113 => obj.ambient = reader.g1b()?,
                114 => obj.contrast = reader.g1b()? * 5,
                115 => obj.team = reader.g1()?,
                _ => return Err(reader.unknown(opcode)),
            }
        }

        reader.finish()?;
        Ok(obj)
    }

    /// Apply Java `ObjType.method224()` to an already decoded certificate.
    /// This remains separate from `decode`, as Java performs it only through `get()`.
    pub fn apply_certificate(
        &mut self,
        template: &Self,
        link: &Self,
    ) -> Result<(), DecodeErrorKind377> {
        let link_name = link
            .name
            .as_deref()
            .ok_or(DecodeErrorKind377::InvalidValue(
                "certificate link has no name",
            ))?;
        let first = link_name
            .chars()
            .next()
            .ok_or(DecodeErrorKind377::InvalidValue(
                "certificate link name is empty",
            ))?;
        let article = if matches!(first, 'A' | 'E' | 'I' | 'O' | 'U') {
            "an"
        } else {
            "a"
        };

        self.model = template.model;
        self.zoom2d = template.zoom2d;
        self.xan2d = template.xan2d;
        self.yan2d = template.yan2d;
        self.zan2d = template.zan2d;
        self.xof2d = template.xof2d;
        self.yof2d = template.yof2d;
        self.recol_s.clone_from(&template.recol_s);
        self.recol_d.clone_from(&template.recol_d);
        self.name = Some(link_name.to_owned());
        self.members = link.members;
        self.cost = link.cost;
        self.desc =
            Some(format!("Swap this note at any bank for {article} {link_name}.").into_bytes());
        self.stackable = true;
        Ok(())
    }

    /// Apply Java `ObjType.get()` membership sanitization when its world is free.
    pub fn sanitize_membership(&mut self, members_world: bool) {
        if !members_world && self.members {
            self.name = Some("Members Object".to_owned());
            self.desc = Some(b"Login to a members' server to use this object.".to_vec());
            self.op = None;
            self.iop = None;
            self.team = 0;
        }
    }
}
