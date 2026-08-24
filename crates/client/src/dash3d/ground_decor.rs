// Port of `~/experiments/Server/webclient/src/dash3d/GroundDecor.ts`.
use crate::dash3d::SceneModel;

pub struct GroundDecor {
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub model: Option<SceneModel>,
    pub typecode: i32,
    pub typecode2: i32,
}

impl GroundDecor {
    pub fn new(
        y: i32,
        x: i32,
        z: i32,
        model: Option<SceneModel>,
        typecode: i32,
        typecode2: i32,
    ) -> Self {
        GroundDecor { y, x, z, model, typecode, typecode2 }
    }
}
