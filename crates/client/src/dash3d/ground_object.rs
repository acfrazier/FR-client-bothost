// Port of `~/experiments/Server/webclient/src/dash3d/GroundObject.ts`.
use crate::dash3d::SceneModel;

pub struct GroundObject {
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub top_obj: Option<Box<SceneModel>>,
    pub middle_obj: Option<Box<SceneModel>>,
    pub bottom_obj: Option<Box<SceneModel>>,
    pub typecode: i32,
    pub height: i32,
}

impl GroundObject {
    pub fn new(
        y: i32,
        x: i32,
        z: i32,
        top_obj: Option<Box<SceneModel>>,
        middle_obj: Option<Box<SceneModel>>,
        bottom_obj: Option<Box<SceneModel>>,
        typecode: i32,
        height: i32,
    ) -> Self {
        GroundObject { y, x, z, top_obj, middle_obj, bottom_obj, typecode, height }
    }
}
