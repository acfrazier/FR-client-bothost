// Port of `~/experiments/Server/webclient/src/dash3d/Ground.ts`. The render
// draw buffers are static in the TS and only used by the render pass (the
// `World` instance carries them in this port).
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

#[derive(Clone)]
pub struct Ground {
    pub vertex_x: Vec<i32>,
    pub vertex_y: Vec<i32>,
    pub vertex_z: Vec<i32>,
    pub face_colour_a: Vec<i32>,
    pub face_colour_b: Vec<i32>,
    pub face_colour_c: Vec<i32>,
    pub face_vertex_a: Vec<i32>,
    pub face_vertex_b: Vec<i32>,
    pub face_vertex_c: Vec<i32>,
    pub face_texture: Option<Vec<i32>>,
    pub flat: bool,
    pub minimap_underlay: i32,
    pub minimap_overlay: i32,
    pub overlay_shape: i32,
    pub overlay_rotation: i32,
}

#[allow(clippy::too_many_arguments)]
impl Ground {
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
        let mut vertex_x = vec![0i32; vertex_count];
        let mut vertex_y = vec![0i32; vertex_count];
        let mut vertex_z = vec![0i32; vertex_count];
        let mut primary_colours = vec![0i32; vertex_count];
        let mut secondary_colours = vec![0i32; vertex_count];

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
        let mut face_vertex_a = vec![0i32; face_count];
        let mut face_vertex_b = vec![0i32; face_count];
        let mut face_vertex_c = vec![0i32; face_count];
        let mut face_colour_a = vec![0i32; face_count];
        let mut face_colour_b = vec![0i32; face_count];
        let mut face_colour_c = vec![0i32; face_count];

        let mut face_texture = if texture != -1 {
            Some(vec![0i32; face_count])
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
            face_colour_a,
            face_colour_b,
            face_colour_c,
            face_vertex_a,
            face_vertex_b,
            face_vertex_c,
            face_texture,
            flat,
            minimap_underlay,
            minimap_overlay,
            overlay_shape,
            overlay_rotation,
        }
    }
}
