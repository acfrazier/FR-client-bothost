// Port of `~/experiments/Server/webclient/src/config/IdkType.ts`.
use crate::io::{JagFile, Packet};

pub struct IdkType {
    pub part: i32,
    pub model: Option<Vec<i32>>,
    pub recol_s: [i32; 6],
    pub recol_d: [i32; 6],
    pub head: [i32; 5],
    pub disable: bool,
}

impl Default for IdkType {
    fn default() -> Self {
        IdkType {
            part: -1,
            model: None,
            recol_s: [0; 6],
            recol_d: [0; 6],
            head: [-1; 5],
            disable: false,
        }
    }
}

impl IdkType {
    pub fn unpack(jag: &JagFile) -> Vec<IdkType> {
        let Some(data) = jag.read("idk.dat") else {
            return Vec::new();
        };
        let mut dat = Packet::new(data);
        let num = dat.g2();
        let mut list = Vec::with_capacity(num as usize);
        for _ in 0..num {
            let mut idk = IdkType::default();
            idk.decode(&mut dat);
            list.push(idk);
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
                1 => self.part = dat.g1(),
                2 => {
                    let count = dat.g1();
                    let mut model = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        model.push(dat.g2());
                    }
                    self.model = Some(model);
                }
                3 => self.disable = true,
                40..=49 => self.recol_s[(code - 40) as usize] = dat.g2(),
                50..=59 => self.recol_d[(code - 50) as usize] = dat.g2(),
                60..=69 => self.head[(code - 60) as usize] = dat.g2(),
                _ => eprintln!("Error unrecognised idk config code: {code}"),
            }
        }
    }
}
