// Port of `~/experiments/Server/webclient/src/config/VarpType.ts`.
use crate::io::{JagFile, Packet};

#[derive(Default)]
pub struct VarpType {
    pub clientcode: i32,
}

impl VarpType {
    pub fn unpack(jag: &JagFile) -> Vec<VarpType> {
        let Some(data) = jag.read("varp.dat") else {
            return Vec::new();
        };
        let mut dat = Packet::new(data);
        let num = dat.g2();
        let mut list = Vec::with_capacity(num as usize);
        for _ in 0..num {
            let mut v = VarpType::default();
            v.decode(&mut dat);
            list.push(v);
        }
        // TS logs `varptype load mismatch` when the file is not fully consumed
        if dat.pos != dat.length() {
            eprintln!("varptype load mismatch");
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
                1 | 2 => dat.pos += 1,
                // Java 289 VarpType.java:120-121; these fields have no client payload.
                3 | 4 | 6 | 8 | 11 | 13 => {}
                5 => self.clientcode = dat.g2(),
                7 => dat.pos += 4,
                10 => {
                    let _ = dat.gjstr();
                }
                // Java 289 VarpType.java:118-119 stores but never reads this g4.
                12 => {
                    let _ = dat.g4();
                }
                _ => eprintln!("Error unrecognised varp config code: {code}"),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::VarpType;
    use crate::io::Packet;

    #[test]
    fn varp12_legacy_opcode_payload_and_13_keep_next_record_aligned() {
        // Java 289 VarpType.java:118-121 consumes g4 for 12 and no payload for 13.
        let mut dat = Packet::new(vec![12, 0, 0, 0, 0, 13, 5, 0x12, 0x34, 0, 5, 0x56, 0x78, 0]);
        let mut first = VarpType::default();
        first.decode(&mut dat);
        let mut second = VarpType::default();
        second.decode(&mut dat);
        assert_eq!(first.clientcode, 0x1234);
        assert_eq!(second.clientcode, 0x5678);
        assert_eq!(dat.pos, 14);
    }
}
