// Port of `~/experiments/Server/webclient/src/dash3d/GroundDecor.ts`.
//
// Task 3b: the placed model no longer lives here; the sim half keeps the
// placement and typecodes, and `render::world::RenderWorld` decodes the
// model lazily on first draw.
pub struct GroundDecor {
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub typecode: i32,
    pub typecode2: i32,
    /// Decode heights fed to `getModel`/`ClientLocAnim` at placement.
    pub h_sw: i32,
    pub h_se: i32,
    pub h_ne: i32,
    pub h_nw: i32,
    /// LOC_ANIM override: the anim seq id, or -1 for the base model.
    pub anim_seq: i32,
    /// LOC_ANIM override: the info shape and rotation the anim applies.
    pub anim_shape: i32,
    pub anim_angle: i32,
}

impl GroundDecor {
    pub fn new(
        y: i32,
        x: i32,
        z: i32,
        typecode: i32,
        typecode2: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) -> Self {
        GroundDecor {
            y,
            x,
            z,
            typecode,
            typecode2,
            h_sw,
            h_se,
            h_ne,
            h_nw,
            anim_seq: -1,
            anim_shape: 0,
            anim_angle: 0,
        }
    }
}
