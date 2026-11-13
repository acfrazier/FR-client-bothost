// Port of `~/experiments/Server/webclient/src/config/VarBitType.ts`.
use crate::io::{JagFile, Packet};

pub struct VarBitType {
    pub basevar: i32,
    pub startbit: i32,
    pub endbit: i32,
    pub debugname: String,
}

impl Default for VarBitType {
    fn default() -> Self {
        VarBitType {
            basevar: -1,
            startbit: 0,
            endbit: 0,
            debugname: String::new(),
        }
    }
}

impl VarBitType {
    pub fn unpack(jag: &JagFile) -> Vec<VarBitType> {
        let Some(data) = jag.read("varbit.dat") else {
            return Vec::new();
        };
        let mut dat = Packet::new(data);
        let num = dat.g2();
        let mut list = Vec::with_capacity(num as usize);
        for _ in 0..num {
            let mut v = VarBitType::default();
            v.decode(&mut dat);
            list.push(v);
        }
        // TS logs `varbit load mismatch` when the file is not fully consumed
        if dat.pos != dat.length() {
            eprintln!("varbit load mismatch");
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
                    self.basevar = dat.g2();
                    self.startbit = dat.g1();
                    self.endbit = dat.g1();
                }
                10 => self.debugname = dat.gjstr(),
                // Java 289 VarbitType.java:74-79: code 2 is empty; 3/4 store g4.
                2 => {}
                3 | 4 => {
                    let _ = dat.g4();
                }
                _ => eprintln!("Error unrecognised varbit config code: {code}"),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::VarBitType;
    use crate::io::Packet;

    #[test]
    fn varbit_legacy_opcode_payloads_and_2_keep_next_record_aligned() {
        // Java 289 VarbitType.java:74-79; 2 has no payload, 3/4 each consume g4.
        let mut dat = Packet::new(vec![
            2, 3, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 1, 0x12, 0x34, 5, 6, 0,
        ]);
        let mut first = VarBitType::default();
        first.decode(&mut dat);
        let mut second = VarBitType::default();
        second.decode(&mut dat);
        assert_eq!((second.basevar, second.startbit, second.endbit), (0x1234, 5, 6));
        assert_eq!(dat.pos, 18);
    }
}
