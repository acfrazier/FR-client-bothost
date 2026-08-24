// Port of `~/experiments/Server/webclient/src/dash3d/Decor.ts`.
//
// Task 3b: the placed model no longer lives here. The sim half keeps the
// placement (y/x/z, wshape/angle, typecodes), the decode heights and the
// LOC_ANIM override state; `render::world::RenderWorld` decodes the model
// lazily on first draw. `typecode2` is the `info` byte the TS `Decor`
// stores (the caller passes the `typecode2` of the loc placement).
pub struct Decor {
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub wshape: i32,
    pub angle: i32,
    pub typecode: i32,
    pub typecode2: i32,
    /// Decode heights fed to `getModel`/`ClientLocAnim` at placement
    /// (post hill-skew swap for the walldecor branch, like `addLoc`).
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

impl Decor {
    pub fn new(
        y: i32,
        x: i32,
        z: i32,
        wshape: i32,
        angle: i32,
        typecode: i32,
        typecode2: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) -> Self {
        Decor {
            y,
            x,
            z,
            wshape,
            angle,
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
