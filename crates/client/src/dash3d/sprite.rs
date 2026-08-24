// Port of `~/experiments/Server/webclient/src/dash3d/Sprite.ts`.
//
// Sprites are an abstract entity - a model to be rendered. It can be a Loc,
// Player, NPC, or another renderable class.
//
// Task 3b: the placed `model` no longer lives here. The sim half keeps the
// placement (x/y/z/level, yaw, tile span, typecodes), the decode heights
// and the LOC_ANIM override state; `render::world::RenderWorld` holds the
// models in a parallel arena (`sprite_models`), decoded lazily on first
// draw. `cycle`/`distance` stay here: they are per-frame render-pass
// stamps that ride on the shared arena exactly like `Square.draw_front`/
// `draw_back` ride on the sim tile grid.
pub struct Sprite {
    pub level: i32,
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub yaw: i32,
    pub min_tile_x: i32,
    pub max_tile_x: i32,
    pub min_tile_z: i32,
    pub max_tile_z: i32,
    pub typecode: i32,
    pub typecode2: i32,
    pub distance: i32,
    pub cycle: i32,
    /// Decode heights fed to `getModel`/`ClientLocAnim` at placement.
    pub h_sw: i32,
    pub h_se: i32,
    pub h_ne: i32,
    pub h_nw: i32,
    /// LOC_ANIM override: the anim seq id, or -1 for the base model.
    pub anim_seq: i32,
    /// LOC_ANIM override: the info shape (11 remapped to 10) and rotation.
    pub anim_shape: i32,
    pub anim_angle: i32,
    /// Bumped by LOC_ANIM; the render side re-resolves the model when it
    /// changes (the arena slots are reused via `None` holes, so the stamp
    /// rides on the sprite itself).
    pub model_stamp: i32,
}

impl Sprite {
    pub fn new(
        level: i32,
        y: i32,
        x: i32,
        z: i32,
        yaw: i32,
        min_tile_x: i32,
        max_tile_x: i32,
        min_tile_z: i32,
        max_tile_z: i32,
        typecode: i32,
        typecode2: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) -> Self {
        Sprite {
            level,
            y,
            x,
            z,
            yaw,
            min_tile_x,
            max_tile_x,
            min_tile_z,
            max_tile_z,
            typecode,
            typecode2,
            distance: 0,
            cycle: 0,
            h_sw,
            h_se,
            h_ne,
            h_nw,
            anim_seq: -1,
            anim_shape: 0,
            anim_angle: 0,
            model_stamp: 0,
        }
    }
}
