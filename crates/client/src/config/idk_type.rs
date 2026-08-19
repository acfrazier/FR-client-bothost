// Port of `~/experiments/Server/webclient/src/config/IdkType.ts` plus the
// model methods (Task 15; they consume the `dash3d` `Model` loader).
use crate::dash3d::Model;
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

    /// `checkModel()` from client-ts.
    pub fn check_model(&self) -> bool {
        let Some(model) = &self.model else { return true };

        let mut ready = true;
        for &m in model {
            if !Model::request_download(m) {
                ready = false;
            }
        }
        ready
    }

    /// `getModelNoCheck()` from client-ts.
    pub fn get_model_no_check(&self) -> Option<Model> {
        let models = self.model.as_ref()?;

        let mut loaded: Vec<Option<Model>> = Vec::with_capacity(models.len());
        for &m in models {
            loaded.push(Model::load(m));
        }

        let model = if models.len() == 1 {
            loaded.into_iter().next().flatten()
        } else {
            Some(Model::combine_for_anim(&loaded, loaded.len()))
        };

        let mut model = model?;
        for i in 0..6 {
            if self.recol_s[i] != 0 {
                model.recolour(self.recol_s[i], self.recol_d[i]);
            }
        }
        Some(model)
    }

    /// `checkHead()` from client-ts.
    pub fn check_head(&self) -> bool {
        let mut ready = true;
        for &h in &self.head {
            if h != -1 && !Model::request_download(h) {
                ready = false;
            }
        }
        ready
    }

    /// `getHeadNoCheck()` from client-ts.
    pub fn get_head_no_check(&self) -> Option<Model> {
        let mut models: Vec<Option<Model>> = Vec::new();
        for &h in &self.head {
            if h != -1 {
                models.push(Model::load(h));
            }
        }

        let mut model = Model::combine_for_anim(&models, models.len());
        for i in 0..6 {
            if self.recol_s[i] != 0 {
                model.recolour(self.recol_s[i], self.recol_d[i]);
            }
        }
        Some(model)
    }
}
