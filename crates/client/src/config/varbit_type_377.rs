//! Client-Java 377 `config/VarbitType.java`: defaults 9-34, opcodes 61-84.
//! Cache377 performs the referenced-varp transmit effect at Java lines 50-53.

use super::{DecodeError377, Reader377};

#[derive(Debug, PartialEq, Eq)]
pub struct VarbitType377 {
    pub basevar: i32,
    pub startbit: i32,
    pub endbit: i32,
    pub transmit: bool,
    pub code3: i32,
    pub code4: i32,
    pub code5: bool,
    pub debugname: Option<String>,
}

impl Default for VarbitType377 {
    fn default() -> Self {
        Self {
            basevar: 0,
            startbit: 0,
            endbit: 0,
            transmit: false,
            code3: -1,
            code4: 0,
            code5: true,
            debugname: None,
        }
    }
}

impl VarbitType377 {
    pub fn decode(dat: &mut Reader377<'_>) -> Result<Self, DecodeError377> {
        let mut varbit = Self::default();
        loop {
            let code = dat.g1()?;
            match code {
                0 => return Ok(varbit),
                1 => {
                    varbit.basevar = dat.g2()?;
                    varbit.startbit = dat.g1()?;
                    varbit.endbit = dat.g1()?;
                }
                2 => varbit.transmit = true,
                3 => varbit.code3 = dat.g4()?,
                4 => varbit.code4 = dat.g4()?,
                5 => varbit.code5 = false,
                10 => varbit.debugname = Some(dat.gjstr()?),
                _ => return Err(dat.unknown(code)),
            }
        }
    }
}
