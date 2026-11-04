//! Client-Java 377 `config/IdkType.java`: defaults 10-29, opcodes 50-76.

use super::{DecodeError377, DecodeErrorKind377, Reader377};

#[derive(Debug, PartialEq, Eq)]
pub struct IdkType377 {
    pub r#type: i32,
    pub models: Option<Vec<i32>>,
    pub recol_s: [i32; 6],
    pub recol_d: [i32; 6],
    pub heads: [i32; 5],
    pub disable: bool,
}

impl Default for IdkType377 {
    fn default() -> Self {
        Self {
            r#type: -1,
            models: None,
            recol_s: [0; 6],
            recol_d: [0; 6],
            heads: [-1; 5],
            disable: false,
        }
    }
}

impl IdkType377 {
    pub fn decode(dat: &mut Reader377<'_>) -> Result<Self, DecodeError377> {
        let mut kit = Self::default();
        loop {
            let code = dat.g1()?;
            match code {
                0 => return Ok(kit),
                1 => kit.r#type = dat.g1()?,
                2 => {
                    let count = dat.g1()?;
                    let mut models = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        models.push(dat.g2()?);
                    }
                    kit.models = Some(models);
                }
                3 => kit.disable = true,
                40..=49 => {
                    // Java's opcode range exceeds its six-slot array. Invalid
                    // indices are errors, not silently accepted extra fields.
                    let slot = kit.recol_s.get_mut((code - 40) as usize).ok_or_else(|| {
                        dat.error(DecodeErrorKind377::InvalidValue("idk recolour slot"))
                    })?;
                    *slot = dat.g2()?;
                }
                50..=59 => {
                    let slot = kit.recol_d.get_mut((code - 50) as usize).ok_or_else(|| {
                        dat.error(DecodeErrorKind377::InvalidValue("idk recolour slot"))
                    })?;
                    *slot = dat.g2()?;
                }
                60..=69 => {
                    let slot = kit.heads.get_mut((code - 60) as usize).ok_or_else(|| {
                        dat.error(DecodeErrorKind377::InvalidValue("idk head slot"))
                    })?;
                    *slot = dat.g2()?;
                }
                _ => return Err(dat.unknown(code)),
            }
        }
    }
}
