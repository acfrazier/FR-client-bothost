// Port of `~/experiments/Server/webclient/src/dash3d/GroundObject.ts`.
//
// Task 3b: the stack's `ClientObj` models no longer live here (they are
// lightweight `(id, count)` descriptors; `render::world::RenderWorld`
// materialises `SceneModel::Obj(ClientObj::new(id, count))` from them on
// first draw). The `height` (stack offset from the tile's sprite
// `obj_raise`) is likewise computed by the render side at draw time, so the
// sim half stores only placement, the typecode and the stack descriptors.
pub struct GroundObject {
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub typecode: i32,
    /// The stack top / middle / bottom `(obj id, count)` descriptors
    /// `showObject` selected; `None` = no object on that stack level.
    pub top: Option<(i32, i32)>,
    pub middle: Option<(i32, i32)>,
    pub bottom: Option<(i32, i32)>,
}

impl GroundObject {
    pub fn new(
        y: i32,
        x: i32,
        z: i32,
        typecode: i32,
        top: Option<(i32, i32)>,
        middle: Option<(i32, i32)>,
        bottom: Option<(i32, i32)>,
    ) -> Self {
        GroundObject {
            y,
            x,
            z,
            typecode,
            top,
            middle,
            bottom,
        }
    }
}
