//! Client-Java 377 `config/SpotAnimType.java`: defaults 11-48,
//! ID assignment 60-65, opcodes and sequence link 70-100.

use super::{DecodeError377, DecodeErrorKind377, Reader377};

#[derive(Debug, PartialEq, Eq)]
pub struct SpotType377 {
    pub index: usize,
    pub model: i32,
    pub anim: i32,
    /// Index into Cache377.seqs instead of a Java object reference.
    pub seq: Option<usize>,
    pub recol_s: [i32; 6],
    pub recol_d: [i32; 6],
    pub resizeh: i32,
    pub resizev: i32,
    pub orientation: i32,
    pub ambient: i32,
    pub contrast: i32,
}

impl Default for SpotType377 {
    fn default() -> Self {
        Self {
            index: 0,
            model: 0,
            anim: -1,
            seq: None,
            recol_s: [0; 6],
            recol_d: [0; 6],
            resizeh: 128,
            resizev: 128,
            orientation: 0,
            ambient: 0,
            contrast: 0,
        }
    }
}

impl SpotType377 {
    pub fn decode(dat: &mut Reader377<'_>) -> Result<Self, DecodeError377> {
        let mut spot = Self {
            index: dat.record,
            ..Self::default()
        };
        loop {
            let code = dat.g1()?;
            match code {
                0 => return Ok(spot),
                1 => spot.model = dat.g2()?,
                2 => spot.anim = dat.g2()?,
                4 => spot.resizeh = dat.g2()?,
                5 => spot.resizev = dat.g2()?,
                6 => spot.orientation = dat.g2()?,
                7 => spot.ambient = dat.g1()?,
                8 => spot.contrast = dat.g1()?,
                40..=49 => {
                    let slot = spot.recol_s.get_mut((code - 40) as usize).ok_or_else(|| {
                        dat.error(DecodeErrorKind377::InvalidValue("spotanim recolour slot"))
                    })?;
                    *slot = dat.g2()?;
                }
                50..=59 => {
                    let slot = spot.recol_d.get_mut((code - 50) as usize).ok_or_else(|| {
                        dat.error(DecodeErrorKind377::InvalidValue("spotanim recolour slot"))
                    })?;
                    *slot = dat.g2()?;
                }
                _ => return Err(dat.unknown(code)),
            }
        }
    }
}
