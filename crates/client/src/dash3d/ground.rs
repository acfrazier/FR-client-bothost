// Port of `~/experiments/Server/webclient/src/dash3d/Ground.ts`. The render
// draw buffers are static in the TS and only used by the render pass (the
// `World` instance carries them in this port).
//
// `GroundStamp` is the sim-side record of one `set_ground` call (rule 6 of
// the head design): the raw overlay inputs a headed build turns into a
// render-only `Ground`/`QuickGround` mesh. Unheaded builds store only this
// compact stamp (17 i32s, no heap); the renderer materializes the mesh from
// it on first paint after a build.
use crate::dash3d::{QuickGround, Square, TerrainOverlayShape};
//
// `Clone` is for the render pass: `World::fill` hands the tile's ground to
// `render_ground` by value because the scene borrow would otherwise
// outlive the `&mut self` raster call (the TS passes the shared object).
// A tile's ground is at most 6 vertices / 6 faces, so the copy is trivial.
const FULL_SQUARE: i32 = 128;
const HALF_SQUARE: i32 = FULL_SQUARE / 2;
const CORNER_SMALL: i32 = FULL_SQUARE / 4;
const CORNER_BIG: i32 = (FULL_SQUARE * 3) / 4;

// shape points
const DEF_SHAPE_P: [&[i8]; 13] = [
    &[1, 3, 5, 7],
    &[1, 3, 5, 7],
    &[1, 3, 5, 7],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 2, 6],
    &[1, 3, 5, 7, 2, 8],
    &[1, 3, 5, 7, 2, 8],
    &[1, 3, 5, 7, 11, 12],
    &[1, 3, 5, 7, 11, 12],
    &[1, 3, 5, 7, 13, 14],
];

// shape faces
const DEF_SHAPE_F: [&[i8]; 13] = [
    &[0, 1, 2, 3, 0, 0, 1, 3],
    &[1, 1, 2, 3, 1, 0, 1, 3],
    &[0, 1, 2, 3, 1, 0, 1, 3],
    &[0, 0, 1, 2, 0, 0, 2, 4, 1, 0, 4, 3],
    &[0, 0, 1, 4, 0, 0, 4, 3, 1, 1, 2, 4],
    &[0, 0, 4, 3, 1, 0, 1, 2, 1, 0, 2, 4],
    &[0, 1, 2, 4, 1, 0, 1, 4, 1, 0, 4, 3],
    &[0, 4, 1, 2, 0, 4, 2, 5, 1, 0, 4, 5, 1, 0, 5, 3],
    &[0, 4, 1, 2, 0, 4, 2, 3, 0, 4, 3, 5, 1, 0, 4, 5],
    &[0, 0, 4, 5, 1, 4, 1, 2, 1, 4, 2, 3, 1, 4, 3, 5],
    &[0, 0, 1, 5, 0, 1, 4, 5, 0, 1, 2, 4, 1, 0, 5, 3, 1, 5, 4, 3, 1, 4, 2, 3],
    &[1, 0, 1, 5, 1, 1, 4, 5, 1, 1, 2, 4, 0, 0, 5, 3, 0, 5, 4, 3, 0, 4, 2, 3],
    &[1, 0, 5, 4, 1, 0, 1, 5, 0, 0, 4, 3, 0, 4, 5, 3, 0, 5, 2, 3, 0, 1, 2, 5],
];

/// Overlay ground is at most 6 vertices / 6 faces (`DEF_SHAPE_*`). Inline
/// arrays avoid 9 heap Vecs per overlay tile (tens of thousands per scene
/// × N clients — macOS malloc does not return those pages).
const GROUND_MAX: usize = 6;

#[derive(Clone)]
pub struct Ground {
    pub vertex_x: [i32; GROUND_MAX],
    pub vertex_y: [i32; GROUND_MAX],
    pub vertex_z: [i32; GROUND_MAX],
    pub vertex_count: u8,
    pub face_colour_a: [i32; GROUND_MAX],
    pub face_colour_b: [i32; GROUND_MAX],
    pub face_colour_c: [i32; GROUND_MAX],
    pub face_vertex_a: [i32; GROUND_MAX],
    pub face_vertex_b: [i32; GROUND_MAX],
    pub face_vertex_c: [i32; GROUND_MAX],
    pub face_count: u8,
    pub face_texture: Option<[i32; GROUND_MAX]>,
    pub flat: bool,
    pub minimap_underlay: i32,
    pub minimap_overlay: i32,
    pub overlay_shape: i32,
    pub overlay_rotation: i32,
}

#[allow(clippy::too_many_arguments)]
impl Ground {
    pub fn vertices(&self) -> usize {
        self.vertex_count as usize
    }

    pub fn faces(&self) -> usize {
        self.face_count as usize
    }

    pub fn new(
        x: i32,
        z: i32,
        shape: i32,
        rotation: i32,
        texture: i32,
        height_sw: i32,
        height_se: i32,
        height_ne: i32,
        height_nw: i32,
        colour_sw: i32,
        colour_se: i32,
        colour_ne: i32,
        colour_nw: i32,
        colour2_sw: i32,
        colour2_se: i32,
        colour2_ne: i32,
        colour2_nw: i32,
        overlay: i32,
        underlay: i32,
    ) -> Self {
        let flat =
            !(height_sw != height_se || height_sw != height_ne || height_sw != height_nw);
        let overlay_shape = shape;
        let overlay_rotation = rotation;
        let minimap_overlay = overlay;
        let minimap_underlay = underlay;

        let points = DEF_SHAPE_P[shape as usize];
        let vertex_count = points.len();
        debug_assert!(vertex_count <= GROUND_MAX);
        let mut vertex_x = [0i32; GROUND_MAX];
        let mut vertex_y = [0i32; GROUND_MAX];
        let mut vertex_z = [0i32; GROUND_MAX];
        let mut primary_colours = [0i32; GROUND_MAX];
        let mut secondary_colours = [0i32; GROUND_MAX];

        let scene_x = x * FULL_SQUARE;
        let scene_z = z * FULL_SQUARE;

        for (v, &point) in points.iter().enumerate() {
            let mut r#type = point as i32;

            if r#type & 0x1 == 0 && r#type <= 8 {
                r#type = ((r#type - rotation - rotation - 1) & 0x7) + 1;
            }
            if r#type > 8 && r#type <= 12 {
                r#type = ((r#type - rotation - 9) & 0x3) + 9;
            }
            if r#type > 12 && r#type <= 16 {
                r#type = ((r#type - rotation - 13) & 0x3) + 13;
            }

            let (px, pz, py, colour1, colour2) = match r#type {
                1 => (scene_x, scene_z, height_sw, colour_sw, colour2_sw),
                2 => (
                    scene_x + HALF_SQUARE,
                    scene_z,
                    (height_sw + height_se) >> 1,
                    (colour_sw + colour_se) >> 1,
                    (colour2_sw + colour2_se) >> 1,
                ),
                3 => (scene_x + FULL_SQUARE, scene_z, height_se, colour_se, colour2_se),
                4 => (
                    scene_x + FULL_SQUARE,
                    scene_z + HALF_SQUARE,
                    (height_se + height_ne) >> 1,
                    (colour_se + colour_ne) >> 1,
                    (colour2_se + colour2_ne) >> 1,
                ),
                5 => (
                    scene_x + FULL_SQUARE,
                    scene_z + FULL_SQUARE,
                    height_ne,
                    colour_ne,
                    colour2_ne,
                ),
                6 => (
                    scene_x + HALF_SQUARE,
                    scene_z + FULL_SQUARE,
                    (height_ne + height_nw) >> 1,
                    (colour_ne + colour_nw) >> 1,
                    (colour2_ne + colour2_nw) >> 1,
                ),
                7 => (scene_x, scene_z + FULL_SQUARE, height_nw, colour_nw, colour2_nw),
                8 => (
                    scene_x,
                    scene_z + HALF_SQUARE,
                    (height_nw + height_sw) >> 1,
                    (colour_nw + colour_sw) >> 1,
                    (colour2_nw + colour2_sw) >> 1,
                ),
                9 => (
                    scene_x + HALF_SQUARE,
                    scene_z + CORNER_SMALL,
                    (height_sw + height_se) >> 1,
                    (colour_sw + colour_se) >> 1,
                    (colour2_sw + colour2_se) >> 1,
                ),
                10 => (
                    scene_x + CORNER_BIG,
                    scene_z + HALF_SQUARE,
                    (height_se + height_ne) >> 1,
                    (colour_se + colour_ne) >> 1,
                    (colour2_se + colour2_ne) >> 1,
                ),
                11 => (
                    scene_x + HALF_SQUARE,
                    scene_z + CORNER_BIG,
                    (height_ne + height_nw) >> 1,
                    (colour_ne + colour_nw) >> 1,
                    (colour2_ne + colour2_nw) >> 1,
                ),
                12 => (
                    scene_x + CORNER_SMALL,
                    scene_z + HALF_SQUARE,
                    (height_nw + height_sw) >> 1,
                    (colour_nw + colour_sw) >> 1,
                    (colour2_nw + colour2_sw) >> 1,
                ),
                13 => (
                    scene_x + CORNER_SMALL,
                    scene_z + CORNER_SMALL,
                    height_sw,
                    colour_sw,
                    colour2_sw,
                ),
                14 => (
                    scene_x + CORNER_BIG,
                    scene_z + CORNER_SMALL,
                    height_se,
                    colour_se,
                    colour2_se,
                ),
                15 => (
                    scene_x + CORNER_BIG,
                    scene_z + CORNER_BIG,
                    height_ne,
                    colour_ne,
                    colour2_ne,
                ),
                _ => (
                    scene_x + CORNER_SMALL,
                    scene_z + CORNER_BIG,
                    height_nw,
                    colour_nw,
                    colour2_nw,
                ),
            };

            vertex_x[v] = px;
            vertex_y[v] = py;
            vertex_z[v] = pz;
            primary_colours[v] = colour1;
            secondary_colours[v] = colour2;
        }

        let paths = DEF_SHAPE_F[shape as usize];
        let face_count = paths.len() / 4;
        debug_assert!(face_count <= GROUND_MAX);
        let mut face_vertex_a = [0i32; GROUND_MAX];
        let mut face_vertex_b = [0i32; GROUND_MAX];
        let mut face_vertex_c = [0i32; GROUND_MAX];
        let mut face_colour_a = [0i32; GROUND_MAX];
        let mut face_colour_b = [0i32; GROUND_MAX];
        let mut face_colour_c = [0i32; GROUND_MAX];

        let mut face_texture = if texture != -1 {
            Some([0i32; GROUND_MAX])
        } else {
            None
        };

        let mut index = 0;
        for t in 0..face_count {
            let colour = paths[index] as i32;
            let mut a = paths[index + 1] as i32;
            let mut b = paths[index + 2] as i32;
            let mut c = paths[index + 3] as i32;
            index += 4;

            if a < 4 {
                a = (a - rotation) & 0x3;
            }
            if b < 4 {
                b = (b - rotation) & 0x3;
            }
            if c < 4 {
                c = (c - rotation) & 0x3;
            }

            face_vertex_a[t] = a;
            face_vertex_b[t] = b;
            face_vertex_c[t] = c;

            if colour == 0 {
                face_colour_a[t] = primary_colours[a as usize];
                face_colour_b[t] = primary_colours[b as usize];
                face_colour_c[t] = primary_colours[c as usize];
                if let Some(ft) = face_texture.as_mut() {
                    ft[t] = -1;
                }
            } else {
                face_colour_a[t] = secondary_colours[a as usize];
                face_colour_b[t] = secondary_colours[b as usize];
                face_colour_c[t] = secondary_colours[c as usize];
                if let Some(ft) = face_texture.as_mut() {
                    ft[t] = texture;
                }
            }
        }

        Ground {
            vertex_x,
            vertex_y,
            vertex_z,
            vertex_count: vertex_count as u8,
            face_colour_a,
            face_colour_b,
            face_colour_c,
            face_vertex_a,
            face_vertex_b,
            face_vertex_c,
            face_count: face_count as u8,
            face_texture,
            flat,
            minimap_underlay,
            minimap_overlay,
            overlay_shape,
            overlay_rotation,
        }
    }
}

/// The sim-side record of one `set_ground` call: the raw overlay inputs
/// `Ground::new`/`QuickGround::new` need to build the render-only mesh.
/// Unheaded `map_build` (rule 6 of the head design) writes only this stamp
/// per overlay tile — no `Ground` verts, no heap — and the first headed
/// paint calls `apply_to` to materialize the mesh from it. Heights are
/// kept here (not re-read from `groundh`) because a `push_down` moves the
/// tile to a level whose `groundh` row holds different values.
#[derive(Clone, Copy)]
pub struct GroundStamp {
    pub shape: i32,
    pub rotation: i32,
    pub texture: i32,
    pub height_sw: i32,
    pub height_se: i32,
    pub height_ne: i32,
    pub height_nw: i32,
    pub colour_sw: i32,
    pub colour_se: i32,
    pub colour_ne: i32,
    pub colour_nw: i32,
    pub colour2_sw: i32,
    pub colour2_se: i32,
    pub colour2_ne: i32,
    pub colour2_nw: i32,
    pub overlay: i32,
    pub underlay: i32,
}

#[allow(clippy::too_many_arguments)]
impl GroundStamp {
    /// Mirror of `World::set_ground`'s arguments; `x`/`z` are omitted
    /// because the owning `Square` already carries them.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shape: i32,
        rotation: i32,
        texture: i32,
        height_sw: i32,
        height_se: i32,
        height_ne: i32,
        height_nw: i32,
        colour_sw: i32,
        colour_se: i32,
        colour_ne: i32,
        colour_nw: i32,
        colour2_sw: i32,
        colour2_se: i32,
        colour2_ne: i32,
        colour2_nw: i32,
        overlay: i32,
        underlay: i32,
    ) -> Self {
        GroundStamp {
            shape,
            rotation,
            texture,
            height_sw,
            height_se,
            height_ne,
            height_nw,
            colour_sw,
            colour_se,
            colour_ne,
            colour_nw,
            colour2_sw,
            colour2_se,
            colour2_ne,
            colour2_nw,
            overlay,
            underlay,
        }
    }

    /// Build the render-only overlay mesh for `tile` from this stamp — the
    /// exact branch headed `set_ground` runs. Idempotent: a tile that
    /// already carries a mesh is left alone (a headed build wrote it
    /// directly, or an earlier attach materialized it).
    pub fn apply_to(&self, tile: &mut Square) {
        if tile.quick_ground.is_some() || tile.ground.is_some() {
            return;
        }
        let Self {
            shape,
            rotation,
            texture,
            height_sw,
            height_se,
            height_ne,
            height_nw,
            colour_sw,
            colour_se,
            colour_ne,
            colour_nw,
            colour2_sw,
            colour2_se,
            colour2_ne,
            colour2_nw,
            overlay,
            underlay,
        } = *self;
        if shape == TerrainOverlayShape::PLAIN {
            tile.quick_ground = Some(QuickGround::new(
                colour_sw, colour_se, colour_ne, colour_nw, -1, overlay, false,
            ));
        } else if shape == TerrainOverlayShape::DIAGONAL {
            tile.quick_ground = Some(QuickGround::new(
                colour2_sw,
                colour2_se,
                colour2_ne,
                colour2_nw,
                texture,
                underlay,
                height_sw == height_se && height_sw == height_ne && height_sw == height_nw,
            ));
        } else {
            tile.ground = Some(Box::new(Ground::new(
                tile.x, tile.z, shape, rotation, texture, height_sw, height_se, height_ne,
                height_nw, colour_sw, colour_se, colour_ne, colour_nw, colour2_sw, colour2_se,
                colour2_ne, colour2_nw, overlay, underlay,
            )));
        }
    }
}
