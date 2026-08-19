// Port of `~/experiments/Server/webclient/src/dash3d/Sprite.ts`.
use crate::dash3d::SceneModel;

/// Sprites are an abstract entity - a model to be rendered. It can be a Loc,
/// Player, NPC, or another renderable class.
pub struct Sprite {
    pub level: i32,
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub model: Option<SceneModel>,
    pub yaw: i32,
    pub min_tile_x: i32,
    pub max_tile_x: i32,
    pub min_tile_z: i32,
    pub max_tile_z: i32,
    pub typecode: i32,
    pub typecode2: i32,
    pub distance: i32,
    pub cycle: i32,
}

impl Sprite {
    pub fn new(
        level: i32,
        y: i32,
        x: i32,
        z: i32,
        model: Option<SceneModel>,
        yaw: i32,
        min_tile_x: i32,
        max_tile_x: i32,
        min_tile_z: i32,
        max_tile_z: i32,
        typecode: i32,
        typecode2: i32,
    ) -> Self {
        Sprite {
            level,
            y,
            x,
            z,
            model,
            yaw,
            min_tile_x,
            max_tile_x,
            min_tile_z,
            max_tile_z,
            typecode,
            typecode2,
            distance: 0,
            cycle: 0,
        }
    }
}
