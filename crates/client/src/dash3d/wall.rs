// Port of `~/experiments/Server/webclient/src/dash3d/Wall.ts`.
use crate::dash3d::SceneModel;

pub struct Wall {
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub angle1: i32,
    pub angle2: i32,
    pub model1: Option<Box<SceneModel>>,
    pub model2: Option<Box<SceneModel>>,
    pub typecode: i32,
    pub typecode2: i32,
}

impl Wall {
    pub fn new(
        y: i32,
        x: i32,
        z: i32,
        angle1: i32,
        angle2: i32,
        model1: Option<Box<SceneModel>>,
        model2: Option<Box<SceneModel>>,
        typecode: i32,
        typecode2: i32,
    ) -> Self {
        Wall { y, x, z, angle1, angle2, model1, model2, typecode, typecode2 }
    }
}
