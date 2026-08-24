// Port of `~/experiments/Server/webclient/src/dash3d/Wall.ts`.
//
// Task 3b: the placed models (`model1`/`model2` in the TS) no longer live
// here. The sim half keeps the placement (y/x/z, angles, typecodes), the
// decode heights (`h_*`, the exact values `addLoc`/`changeLocUnchecked`/
// `locAnimChange` fed `getModel`), and the LOC_ANIM override state
// (`anim_seq` >= 0 = the tile's wall is animated with that seq id). The
// render half (`render::world::RenderWorld`) decodes the models from these
// small ints lazily on first draw.
pub struct Wall {
    pub y: i32,
    pub x: i32,
    pub z: i32,
    pub angle1: i32,
    pub angle2: i32,
    pub typecode: i32,
    pub typecode2: i32,
    /// Decode heights fed to `getModel`/`ClientLocAnim` at placement
    /// (`groundh[level][x][z]` with the `changeLocUnchecked` true-level /
    /// walldecor hill-skew corrections applied).
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

impl Wall {
    pub fn new(
        y: i32,
        x: i32,
        z: i32,
        angle1: i32,
        angle2: i32,
        typecode: i32,
        typecode2: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) -> Self {
        Wall {
            y,
            x,
            z,
            angle1,
            angle2,
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
