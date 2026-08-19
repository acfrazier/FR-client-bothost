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
                // TS branches for these are empty: server-side fields carry no
                // client payload, only the code byte is consumed
                3 | 4 | 6 | 8 | 11 => {}
                5 => self.clientcode = dat.g2(),
                7 => dat.pos += 4,
                10 => {
                    let _ = dat.gjstr();
                }
                // TS's else also consumes only the code byte
                _ => eprintln!("Error unrecognised varp config code: {code}"),
            }
        }
    }
}
