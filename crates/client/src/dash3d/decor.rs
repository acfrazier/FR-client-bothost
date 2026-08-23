// Port of `~/experiments/Server/webclient/src/dash3d/Decor.ts`.
use crate::dash3d::SceneModel;

pub struct Decor {
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub wshape: i32,
    pub angle: i32,
    pub model: Box<SceneModel>,
    pub typecode: i32,
    pub typecode2: i32,
}

impl Decor {
    pub fn new(
        y: i32,
        x: i32,
        z: i32,
        wshape: i32,
        angle: i32,
        model: Box<SceneModel>,
        typecode: i32,
        typecode2: i32,
    ) -> Self {
        Decor { y, x, z, wshape, angle, model, typecode, typecode2 }
    }
}
