//! Revision-377 NPC cache records.
//!
//! Java source anchors (relative to the pinned client-java checkout):
//! `src/main/java/jagex2/config/NpcType.java:13-74` (field defaults),
//! `NpcType.java:124-220` (opcodes and morph sentinels), and
//! `NpcType.java:265-286` (morph availability uses the decoded array).

use super::{DecodeError377, DecodeErrorKind377, Reader377};

#[derive(Debug, PartialEq)]
pub struct NpcType377 {
    /// Cache record number; Java stores the requested id in `field1431`.
    pub id: i32,
    pub name: String,
    /// Java `field1463` is nullable raw `gjstrraw()` data.
    pub desc: Option<Vec<u8>>,
    /// Java `field1445` is a signed byte.
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
    /// Java `field1449`: null until an opcode allocates its five action slots.
    pub op: Option<[Option<String>; 5]>,
    pub minimap: bool,
    pub vislevel: i32,
    pub resizeh: i32,
    pub resizev: i32,
    pub alwaysontop: bool,
    pub ambient: i32,
    pub contrast: i32,
    pub headicon: i32,
    pub turnspeed: i32,
    /// Java `field1451`, `field1430`, `field1440` (opcodes 90, 91, 92).
    pub field_1451: i32,
    pub field_1430: i32,
    pub field_1440: i32,
    /// Java `field1434`, false only after opcode 107.
    pub field_1434: bool,
    /// Java `field1457`, `field1462`, and `field1425` (opcode 106).
    pub morph_varbit: i32,
    pub morph_varp: i32,
    pub morphs: Option<Vec<i32>>,
}

impl Default for NpcType377 {
    fn default() -> Self {
        Self {
            id: -1,
            name: "null".to_owned(),
            desc: None,
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
            op: None,
            minimap: true,
            vislevel: -1,
            resizeh: 128,
            resizev: 128,
            alwaysontop: false,
            ambient: 0,
            contrast: 0,
            headicon: -1,
            turnspeed: 32,
            field_1451: -1,
            field_1430: -1,
            field_1440: -1,
            field_1434: true,
            morph_varbit: -1,
            morph_varp: -1,
            morphs: None,
        }
    }
}

impl NpcType377 {
    /// Decode one complete `npc.dat` record. The record ordinal supplies its id.
    pub fn decode(reader: &mut Reader377<'_>) -> Result<Self, DecodeError377> {
        let mut npc = Self {
            id: reader.record as i32,
            ..Self::default()
        };

        loop {
            let opcode = reader.g1()?;
            if opcode == 0 {
                break;
            }
            match opcode {
                1 => {
                    let count = reader.g1()? as usize;
                    let mut models = Vec::with_capacity(count);
                    for _ in 0..count {
                        models.push(reader.g2()?);
                    }
                    npc.model = Some(models);
                }
                2 => npc.name = reader.gjstr()?,
                3 => npc.desc = Some(reader.gjstrraw()?),
                12 => npc.size = reader.g1b()?,
                13 => npc.readyanim = reader.g2()?,
                14 => npc.walkanim = reader.g2()?,
                17 => {
                    npc.walkanim = reader.g2()?;
                    npc.walkanim_b = reader.g2()?;
                    npc.walkanim_r = reader.g2()?;
                    npc.walkanim_l = reader.g2()?;
                }
                30..=39 => {
                    let value = reader.gjstr()?;
                    let index = (opcode - 30) as usize;
                    let Some(indexed_action) = (index < 5).then_some(index) else {
                        return Err(reader.error(DecodeErrorKind377::InvalidValue(
                            "NPC action opcode indexes beyond Java's five slots",
                        )));
                    };
                    let actions = npc.op.get_or_insert_with(|| std::array::from_fn(|_| None));
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
                    npc.recol_s = Some(recol_s);
                    npc.recol_d = Some(recol_d);
                }
                60 => {
                    let count = reader.g1()? as usize;
                    let mut head = Vec::with_capacity(count);
                    for _ in 0..count {
                        head.push(reader.g2()?);
                    }
                    npc.head = Some(head);
                }
                90 => npc.field_1451 = reader.g2()?,
                91 => npc.field_1430 = reader.g2()?,
                92 => npc.field_1440 = reader.g2()?,
                93 => npc.minimap = false,
                95 => npc.vislevel = reader.g2()?,
                97 => npc.resizeh = reader.g2()?,
                98 => npc.resizev = reader.g2()?,
                99 => npc.alwaysontop = true,
                100 => npc.ambient = reader.g1b()?,
                101 => npc.contrast = reader.g1b()? * 5,
                102 => npc.headicon = reader.g2()?,
                103 => npc.turnspeed = reader.g2()?,
                106 => {
                    npc.morph_varbit = reader.g2()?;
                    if npc.morph_varbit == 65_535 {
                        npc.morph_varbit = -1;
                    }
                    npc.morph_varp = reader.g2()?;
                    if npc.morph_varp == 65_535 {
                        npc.morph_varp = -1;
                    }
                    let count = reader.g1()? as usize + 1;
                    let mut morphs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let morph = reader.g2()?;
                        morphs.push(if morph == 65_535 { -1 } else { morph });
                    }
                    npc.morphs = Some(morphs);
                }
                107 => npc.field_1434 = false,
                _ => return Err(reader.unknown(opcode)),
            }
        }

        reader.finish()?;
        Ok(npc)
    }
}
