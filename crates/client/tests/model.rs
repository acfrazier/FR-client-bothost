// `Model.worldRender` / `objRender` + mouse picking (Task 3). A tiny 1-face
// model in front of the camera with `mouse_check=true` must yield
// `picked_count >= 1` after `world_render` — the pick is geometric, not a
// signature check: the AABB pre-test in worldRender and the per-face
// triangle test in render2 both have to pass for the append to happen.
use client::dash3d::Model;
use client::graphics::{Pix2D, Pix3D, Pix3DDraw, PixMap};

/// 3-point, 1-face model at the origin spanning (-50,-50)..(50,50) on the
/// ground plane. Face (a=0, b=2, c=1) is front-facing (positive screen
/// winding) and gouraud-shaded with a constant shade whose colour-table
/// entry is non-zero (index y=200/x=100: a mid-lightness hue).
const SHADE: i32 = 200 * 128 + 100;

fn one_face_model() -> Model {
    let mut model = Model {
        num_points: 3,
        point_x: Some(vec![-50, 50, 50]),
        point_y: Some(vec![-50, -50, 50]),
        point_z: Some(vec![0, 0, 0]),
        num_faces: 1,
        face_vertex_a: Some(vec![0]),
        face_vertex_b: Some(vec![2]),
        face_vertex_c: Some(vec![1]),
        face_colour_a: Some(vec![SHADE]),
        face_colour_b: Some(vec![SHADE]),
        face_colour_c: Some(vec![SHADE]),
        ..Default::default()
    };
    model.calc_bounding_cylinder();
    model
}

/// 512×334 viewport (the `area_game` size) bound as the render target.
fn viewport(pix: &mut Pix3DDraw, surface: &mut Pix2D) {
    pix.set_render_clipping(surface);
    pix.trans = 0;
    pix.mouse_check = false;
    pix.picked_count = 0;
}

#[test]
fn mouse_check_defaults_false_and_picked_count_starts_zero() {
    let d = Pix3DDraw::default();
    assert!(!d.mouse_check);
    assert_eq!(d.picked_count, 0);
    assert_eq!(d.picked_entity_typecode.len(), 1000);
}

#[test]
fn world_render_with_mouse_check_picks_the_face_in_front_of_camera() {
    Pix3D::init_colour_table(0.6);
    let model = one_face_model();

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        // Mouse at the viewport centre-top: inside the projected triangle.
        pix.mouse_check = true;
        pix.mouse_x = 256;
        pix.mouse_y = 160;

        // Camera at the origin looking +z (eye yaw/pitch 0, model 500 away).
        model.world_render(&mut pix, &mut surface, 0, 0, 65536, 0, 65536, 0, 0, 500, 5);
    }

    assert!(pix.picked_count >= 1, "mouse over the model must pick it");
    assert_eq!(pix.picked_entity_typecode[0], 5);
    // The face is actually rasterised: pixels inside the projected triangle
    // carry the face's constant shade.
    let rgb = Pix3D::colour_table()[SHADE as usize];
    assert_ne!(rgb, 0);
    assert_eq!(map.pixels[160 * 512 + 256], rgb);
}

#[test]
fn world_render_with_aabb_mouse_check_picks_without_triangle_test() {
    Pix3D::init_colour_table(0.6);
    let mut model = one_face_model();
    model.use_aabb_mouse_check = true;

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        pix.mouse_check = true;
        pix.mouse_x = 256;
        pix.mouse_y = 160;
        model.world_render(&mut pix, &mut surface, 0, 0, 65536, 0, 65536, 0, 0, 500, 5);
    }

    assert!(pix.picked_count >= 1, "AABB pick must append the typecode");
    assert_eq!(pix.picked_entity_typecode[0], 5);
}

#[test]
fn world_render_miss_leaves_pick_unchanged() {
    Pix3D::init_colour_table(0.6);
    let model = one_face_model();

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        pix.mouse_check = true;
        pix.mouse_x = 0; // top-left corner, far from the model
        pix.mouse_y = 0;
        model.world_render(&mut pix, &mut surface, 0, 0, 65536, 0, 65536, 0, 0, 500, 5);
    }

    assert_eq!(pix.picked_count, 0, "a miss must not pick anything");
}

#[test]
fn world_render_off_screen_model_is_skipped() {
    Pix3D::init_colour_table(0.6);
    let model = one_face_model();

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        pix.mouse_check = true;
        pix.mouse_x = 256;
        pix.mouse_y = 160;
        // Model 20000 tiles left of the camera: outside the viewport cull.
        model.world_render(
            &mut pix,
            &mut surface,
            0,
            0,
            65536,
            0,
            65536,
            -20000 * 128,
            0,
            500,
            5,
        );
    }

    assert_eq!(pix.picked_count, 0);
    assert!(
        map.pixels.iter().all(|&p| p == 0),
        "culled model draws nothing"
    );
}

#[test]
fn world_render_skips_models_with_missing_points() {
    Pix3D::init_colour_table(0.6);
    let mut model = one_face_model();
    model.point_x = None; // undecoded geometry: must skip, not panic

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        pix.mouse_check = true;
        pix.mouse_x = 256;
        pix.mouse_y = 160;
        model.world_render(&mut pix, &mut surface, 0, 0, 65536, 0, 65536, 0, 0, 500, 5);
    }

    assert_eq!(pix.picked_count, 0);
    assert!(map.pixels.iter().all(|&p| p == 0));
}

#[test]
fn world_render_skips_faces_with_missing_colours() {
    Pix3D::init_colour_table(0.6);
    let mut model = one_face_model();
    model.face_colour_a = None; // unlit face arrays absent: skip, no panic

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        pix.mouse_check = true;
        pix.mouse_x = 256;
        pix.mouse_y = 160;
        model.world_render(&mut pix, &mut surface, 0, 0, 65536, 0, 65536, 0, 0, 500, 5);
    }

    assert!(pix.picked_count >= 1, "the geometric pick still fires");
    assert!(map.pixels.iter().all(|&p| p == 0), "the face is not drawn");
}

#[test]
fn obj_render_vertex_on_camera_plane_does_not_panic() {
    Pix3D::init_colour_table(0.6);
    let mut model = Model {
        num_points: 1,
        point_x: Some(vec![10]),
        point_y: Some(vec![10]),
        point_z: Some(vec![0]),
        num_faces: 0,
        ..Default::default()
    };
    model.calc_bounding_cylinder();

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        // The vertex lands exactly on the camera plane (view z == 0): the TS
        // `((x << 9) / z) | 0` is 0, so the port must not divide by zero.
        model.obj_render(&mut pix, &mut surface, 0, 0, 0, 0, 0, 0, 0);
    }
    assert_eq!(pix.model_scratch.vertex_screen_x[0], 256);
    assert_eq!(pix.model_scratch.vertex_screen_y[0], 167);
}

#[test]
fn world_render_wide_winding_cross_does_not_overflow() {
    Pix3D::init_colour_table(0.6);
    let mut model = Model {
        num_points: 3,
        point_x: Some(vec![20000, -20000, 500]),
        point_y: Some(vec![10000, -10000, 500]),
        point_z: Some(vec![0, 0, 0]),
        num_faces: 1,
        face_vertex_a: Some(vec![0]),
        face_vertex_b: Some(vec![2]),
        face_vertex_c: Some(vec![1]),
        face_colour_a: Some(vec![SHADE]),
        face_colour_b: Some(vec![SHADE]),
        face_colour_c: Some(vec![SHADE]),
        ..Default::default()
    };
    // Vertices far from the camera axis: projected screen coordinates reach
    // ±100k, so the winding cross product overflows i32 (TS computes it in
    // doubles). relative_x = 20 * 65536 wraps to mid_x = 0 in the eye
    // transform, keeping the model on screen while its vertices spread.
    model.calc_bounding_cylinder();

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        pix.mouse_check = true;
        pix.mouse_x = 256;
        pix.mouse_y = 160;
        model.world_render(
            &mut pix,
            &mut surface,
            0,
            0,
            65536,
            0,
            65536,
            20 * 65536,
            0,
            100,
            5,
        );
    }

    // No panic, and the (culled, clockwise-on-screen) face draws nothing.
    assert_eq!(
        pix.picked_count, 1,
        "the pick pre-test fires before winding"
    );
    assert!(map.pixels.iter().all(|&p| p == 0));
}

#[test]
fn depth_bucket_overflow_does_not_spill_into_next_row() {
    Pix3D::init_colour_table(0.6);
    let mut model = Model {
        num_points: 3,
        point_x: Some(vec![-50, 50, 50]),
        point_y: Some(vec![-50, -50, 50]),
        point_z: Some(vec![0, 0, 0]),
        num_faces: 600,
        face_vertex_a: Some(vec![0; 600]),
        face_vertex_b: Some(vec![2; 600]),
        face_vertex_c: Some(vec![1; 600]),
        face_colour_a: Some(vec![SHADE; 600]),
        face_colour_b: Some(vec![SHADE; 600]),
        face_colour_c: Some(vec![SHADE; 600]),
        ..Default::default()
    };
    // 600 identical faces at one depth: more than the 512-slot bucket row.
    model.calc_bounding_cylinder();

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        model.world_render(&mut pix, &mut surface, 0, 0, 65536, 0, 65536, 0, 0, 500, 0);
    }

    // The count advances past the row width (like TS), but the face writes
    // past slot 511 must be dropped, not spilled into the next bucket.
    let bucket = model.min_depth as usize; // every vertex projects to z 0
    assert_eq!(pix.model_scratch.tmp_depth_face_count[bucket], 600);
    assert_eq!(pix.model_scratch.tmp_depth_faces[bucket * 512 + 512], 0);
}

#[test]
fn world_render_priority_model_draws_via_merge_buckets() {
    Pix3D::init_colour_table(0.6);
    let mut model = Model {
        num_points: 3,
        point_x: Some(vec![-50, 50, 50]),
        point_y: Some(vec![-50, -50, 50]),
        point_z: Some(vec![0, 0, 0]),
        num_faces: 2,
        face_vertex_a: Some(vec![0, 0]),
        face_vertex_b: Some(vec![2, 2]),
        face_vertex_c: Some(vec![1, 1]),
        face_colour_a: Some(vec![SHADE, SHADE]),
        face_colour_b: Some(vec![SHADE, SHADE]),
        face_colour_c: Some(vec![SHADE, SHADE]),
        face_priority: Some(vec![10, 10]),
        ..Default::default()
    };
    // Two identical faces both at priority 10: they route through the
    // bucket-10 merge loop (and its rollover to bucket 11).
    model.calc_bounding_cylinder();

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        model.world_render(&mut pix, &mut surface, 0, 0, 65536, 0, 65536, 0, 0, 500, 0);
    }

    let rgb = Pix3D::colour_table()[SHADE as usize];
    assert_eq!(map.pixels[160 * 512 + 256], rgb);
}
