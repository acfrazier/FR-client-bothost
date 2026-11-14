//! Revision-377 location cache records.
//!
//! Java source anchors (relative to the pinned client-java checkout):
//! `src/main/java/jagex2/config/LocType.java:341-379` (defaults),
//! `LocType.java:381-529` (opcodes and decode postpass), and
//! `LocType.java:510-526` (morph sentinel conversion).

use super::{DecodeError377, DecodeErrorKind377, Reader377};

#[derive(Debug, PartialEq)]
pub struct LocType377 {
    /// Cache record number; Java assigns it from the table index.
    pub id: i32,
    /// Java `field1617` and `field1643`.
    pub model: Option<Vec<i32>>,
    pub shape: Option<Vec<i32>>,
    /// Java `field1630`, whose reset value is the literal string `"null"`.
    pub name: String,
    /// Java `field1637` is a nullable raw byte array, not a decoded String.
    pub desc: Option<Vec<u8>>,
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
    /// Java `field1644`: null until an action opcode allocates its 5 slots.
    pub op: Option<[Option<String>; 5]>,
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
    /// Java `field1632`, `field1635`, and `field1659` (opcode 77).
    pub morph_varbit: i32,
    pub morph_varp: i32,
    pub morphs: Option<Vec<i32>>,
}

impl Default for LocType377 {
    fn default() -> Self {
        Self {
            id: -1,
            model: None,
            shape: None,
            name: "null".to_owned(),
            desc: None,
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
            op: None,
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
            morph_varbit: -1,
            morph_varp: -1,
            morphs: None,
        }
    }
}

impl LocType377 {
    /// Decode one complete `loc.dat` record. The record ordinal supplies its id.
    pub fn decode(reader: &mut Reader377<'_>) -> Result<Self, DecodeError377> {
        Self::decode_with_lowmem(reader, false)
    }

    /// Java LocType.lowMem permits later model opcodes to replace earlier ones.
    /// See LocType.java:408-416 and :423-434.
    pub fn decode_with_lowmem(
        reader: &mut Reader377<'_>,
        low_mem: bool,
    ) -> Result<Self, DecodeError377> {
        let mut loc = Self {
            id: reader.record as i32,
            ..Self::default()
        };
        let mut active_opcode_seen = false;

        loop {
            let opcode = reader.g1()?;
            if opcode == 0 {
                break;
            }
            match opcode {
                1 => {
                    let count = reader.g1()? as usize;
                    if loc.model.is_none() || low_mem {
                        let mut models = Vec::with_capacity(count);
                        let mut shapes = Vec::with_capacity(count);
                        for _ in 0..count {
                            models.push(reader.g2()?);
                            shapes.push(reader.g1()?);
                        }
                        loc.model = Some(models);
                        loc.shape = Some(shapes);
                    } else {
                        // Java's non-low-memory decoder retains its first model
                        // list and skips later opcode-1 payloads.
                        reader.take(count * 3)?;
                    }
                }
                2 => loc.name = reader.gjstr()?,
                3 => loc.desc = Some(reader.gjstrraw()?),
                5 => {
                    let count = reader.g1()? as usize;
                    if count > 0 {
                        if loc.model.is_none() || low_mem {
                            let mut models = Vec::with_capacity(count);
                            for _ in 0..count {
                                models.push(reader.g2()?);
                            }
                            loc.model = Some(models);
                            loc.shape = None;
                        } else {
                            // See opcode 1: Java skips a later model list when
                            // its low-memory override is not active.
                            reader.take(count * 2)?;
                        }
                    }
                }
                14 => loc.width = reader.g1()?,
                15 => loc.length = reader.g1()?,
                17 => loc.blockwalk = false,
                18 => loc.blockrange = false,
                19 => {
                    active_opcode_seen = true;
                    if reader.g1()? == 1 {
                        loc.active = true;
                    }
                }
                21 => loc.hillskew = true,
                22 => loc.sharelight = true,
                23 => loc.occlude = true,
                24 => {
                    loc.anim = reader.g2()?;
                    if loc.anim == 65_535 {
                        loc.anim = -1;
                    }
                }
                28 => loc.wallwidth = reader.g1()?,
                29 => loc.ambient = reader.g1b()?,
                39 => loc.contrast = reader.g1b()?,
                30..=38 => {
                    let value = reader.gjstr()?;
                    let index = (opcode - 30) as usize;
                    let Some(indexed_action) = (index < 5).then_some(index) else {
                        return Err(reader.error(DecodeErrorKind377::InvalidValue(
                            "location action opcode indexes beyond Java's five slots",
                        )));
                    };
                    let actions = loc.op.get_or_insert_with(|| std::array::from_fn(|_| None));
                    actions[indexed_action] = if value.eq_ignore_ascii_case("hidden") {
                        None
                    } else {
                        Some(value)
                    };
                }
                40 => {
                    let count = reader.g1()? as usize;
                    let mut recol_s = Vec::with_capacity(count);
                    let mut recol_d = Vec::with_capacity(count);
                    for _ in 0..count {
                        recol_s.push(reader.g2()? as u16);
                        recol_d.push(reader.g2()? as u16);
                    }
                    loc.recol_s = Some(recol_s);
                    loc.recol_d = Some(recol_d);
                }
                60 => loc.mapfunction = reader.g2()?,
                62 => loc.mirror = true,
                64 => loc.shadow = false,
                65 => loc.resizex = reader.g2()?,
                66 => loc.resizey = reader.g2()?,
                67 => loc.resizez = reader.g2()?,
                68 => loc.mapscene = reader.g2()?,
                69 => loc.forceapproach = reader.g1()?,
                70 => loc.offsetx = reader.g2b()?,
                71 => loc.offsety = reader.g2b()?,
                72 => loc.offsetz = reader.g2b()?,
                73 => loc.forcedecor = true,
                74 => loc.breakroutefinding = true,
                75 => loc.raiseobject = reader.g1()?,
                77 => {
                    loc.morph_varbit = reader.g2()?;
                    if loc.morph_varbit == 65_535 {
                        loc.morph_varbit = -1;
                    }
                    loc.morph_varp = reader.g2()?;
                    if loc.morph_varp == 65_535 {
                        loc.morph_varp = -1;
                    }
                    let count = reader.g1()? as usize + 1;
                    let mut morphs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let morph = reader.g2()?;
                        morphs.push(if morph == 65_535 { -1 } else { morph });
                    }
                    loc.morphs = Some(morphs);
                }
                _ => return Err(reader.unknown(opcode)),
            }
        }

        // Java method568's terminator postpass (LocType.java:387-403, 529).
        if !active_opcode_seen {
            loc.active = false;
            let is_centrepiece_straight =
                loc.shape.as_ref().and_then(|shapes| shapes.first()) == Some(&10);
            if loc.model.is_some() && (loc.shape.is_none() || is_centrepiece_straight) {
                loc.active = true;
            }
            if loc.op.is_some() {
                loc.active = true;
            }
        }
        if loc.breakroutefinding {
            loc.blockwalk = false;
            loc.blockrange = false;
        }
        if loc.raiseobject == -1 {
            loc.raiseobject = if loc.blockwalk { 1 } else { 0 };
        }

        reader.finish()?;
        Ok(loc)
    }
}
