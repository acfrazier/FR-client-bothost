// Port of `~/experiments/Server/webclient/src/config/SpotType.ts`. `seq` is
// resolved by `Cache::unpack` once the seq table is loaded (TS links it in
// `init` when `SeqType.list` exists).
use crate::io::{JagFile, Packet};

pub struct SpotType {
    pub id: i32,
    pub model: i32,
    pub anim: i32,
    pub seq: Option<usize>,
    pub recol_s: [u16; 6],
    pub recol_d: [u16; 6],
    pub resizeh: i32,
    pub resizev: i32,
    pub angle: i32,
    pub ambient: i32,
    pub contrast: i32,
}

impl Default for SpotType {
    fn default() -> Self {
        SpotType {
            id: 0,
            model: 0,
            anim: -1,
            seq: None,
            recol_s: [0; 6],
            recol_d: [0; 6],
            resizeh: 128,
            resizev: 128,
            angle: 0,
            ambient: 0,
            contrast: 0,
        }
    }
}

impl SpotType {
    pub fn unpack(jag: &JagFile) -> Vec<SpotType> {
        let Some(data) = jag.read("spotanim.dat") else {
            return Vec::new();
        };
        let mut dat = Packet::new(data);
        let num = dat.g2();
        let mut list = Vec::with_capacity(num as usize);
        for id in 0..num {
            let mut spot = SpotType { id, ..SpotType::default() };
            spot.decode(&mut dat);
            list.push(spot);
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
                1 => self.model = dat.g2(),
                2 => self.anim = dat.g2(),
                4 => self.resizeh = dat.g2(),
                5 => self.resizev = dat.g2(),
                6 => self.angle = dat.g2(),
                7 => self.ambient = dat.g1(),
                8 => self.contrast = dat.g1(),
                40..=49 => self.recol_s[(code - 40) as usize] = dat.g2() as u16,
                50..=59 => self.recol_d[(code - 50) as usize] = dat.g2() as u16,
                _ => eprintln!("Error unrecognised spotanim config code: {code}"),
            }
        }
    }
}
