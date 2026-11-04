//! Client-Java 377 `config/VarpType.java`: defaults 9-49, opcodes 80-118.
//! Opcode 3 appends the ID for each occurrence (90-92), not once per record.

use super::{DecodeError377, Reader377};

#[derive(Debug, PartialEq, Eq)]
pub struct VarpType377 {
    pub code1: i32,
    pub code2: i32,
    pub tracked: bool,
    pub code4: bool,
    pub clientcode: i32,
    pub code6: bool,
    pub code7: i32,
    pub scope: i32,
    pub transmit: bool,
    pub code12: i32,
    pub code14: bool,
    pub debugname: Option<String>,
}

impl Default for VarpType377 {
    fn default() -> Self {
        Self {
            code1: 0,
            code2: 0,
            tracked: false,
            code4: true,
            clientcode: 0,
            code6: false,
            code7: 0,
            scope: 0,
            transmit: false,
            code12: -1,
            code14: true,
            debugname: None,
        }
    }
}

impl VarpType377 {
    pub fn decode(
        dat: &mut Reader377<'_>,
        tracked_ids: &mut Vec<usize>,
    ) -> Result<Self, DecodeError377> {
        let mut varp = Self::default();
        loop {
            let code = dat.g1()?;
            match code {
                0 => return Ok(varp),
                1 => varp.code1 = dat.g1()?,
                2 => varp.code2 = dat.g1()?,
                3 => {
                    varp.tracked = true;
                    tracked_ids.push(dat.record);
                }
                4 => varp.code4 = false,
                5 => varp.clientcode = dat.g2()?,
                6 => varp.code6 = true,
                7 => varp.code7 = dat.g4()?,
                8 => {
                    varp.scope = 1;
                    varp.transmit = true;
                }
                10 => varp.debugname = Some(dat.gjstr()?),
                11 => varp.transmit = true,
                12 => varp.code12 = dat.g4()?,
                13 => {
                    varp.scope = 2;
                    varp.transmit = true;
                }
                14 => varp.code14 = false,
                _ => return Err(dat.unknown(code)),
            }
        }
    }
}
