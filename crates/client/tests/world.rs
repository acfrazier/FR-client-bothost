// `World::render_all` / `update_mouse_picking` (Task 4). A synthetic 3×3
// flat world at ground height 2000 renders into a 512×334 viewport: the
// `groundh - eyeY >= 2000` gate in `renderAll` marks every tile drawable
// even with an unpopulated `visBacking`, so the whole scene raster path
// (fill → renderQuickGround → gouraud) runs without a pack. A mouse click
// on the projected ground must come back as `ground_x`/`ground_z`.
use client::config::Cache;
use client::dash3d::{Model, SceneModel, TerrainOverlayShape, World};
use client::graphics::{Pix2D, Pix3D, Pix3DDraw, PixMap};

/// Shade whose colour-table entry is non-zero (same constant as the model
/// tests: index y=200/x=100).
const SHADE: i32 = 200 * 128 + 100;

fn one_face_model() -> Model {
    let mut model = Model::default();
    model.num_points = 3;
    model.point_x = Some(vec![-50, 50, 50]);
    model.point_y = Some(vec![-50, -50, 50]);
    model.point_z = Some(vec![0, 0, 0]);
    model.num_faces = 1;
    model.face_vertex_a = Some(vec![0]);
    model.face_vertex_b = Some(vec![2]);
    model.face_vertex_c = Some(vec![1]);
    model.face_colour_a = Some(vec![SHADE]);
    model.face_colour_b = Some(vec![SHADE]);
    model.face_colour_c = Some(vec![SHADE]);
    model.calc_bounding_cylinder();
    model
}

/// 3×3 flat world at height 2000 with a plain-coloured tile on every cell.
fn flat_world() -> World {
    let max_level: i32 = 1;
    let max_tile_x: i32 = 3;
    let max_tile_z: i32 = 3;
    let groundh = vec![
        vec![vec![2000i32; max_tile_z as usize + 1]; max_tile_x as usize + 1];
        max_level as usize
    ];
    let mut world = World::new(groundh, max_tile_z, max_level, max_tile_x);
    world.fill_base_level(0);
    for x in 0..max_tile_x {
        for z in 0..max_tile_z {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }
    world
}

/// 512×334 viewport (the `area_game` size) bound as the render target.
fn viewport(pix: &mut Pix3DDraw, surface: &mut Pix2D) {
    pix.set_render_clipping(surface);
    pix.trans = 0;
    pix.low_mem = true;
}

#[test]
fn update_mouse_picking_sets_click_and_clears_ground() {
    let mut w = flat_world();
    w.ground_x = 3;
    w.update_mouse_picking(10, 20);
    assert!(w.click);
    assert_eq!(w.click_x, 10);
    assert_eq!(w.click_y, 20);
    assert_eq!(w.ground_x, -1);
    assert_eq!(w.ground_z, -1);
}

#[test]
fn render_all_writes_pixels_and_picks_ground_tile() {
    Pix3D::init_colour_table(0.6);
    let mut world = flat_world();
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        // Camera at (192, 0, 192) (tile 1,1), pitch 512 looking horizontal
        // at the height-2000 ground. Tile (1,2) projects to screen
        // (240..272, 118..151); (256, 134) is inside its first triangle.
        world.update_mouse_picking(256, 134);
        world.render_all(&mut pix, &mut surface, &Cache::default(), 0, 192, 0, 192, 0, 0, 512);
    }

    assert!(
        map.pixels.iter().any(|&p| p != 0),
        "render_all must draw the ground into the viewport"
    );
    assert_eq!(world.ground_x, 1);
    assert_eq!(world.ground_z, 2);
}

#[test]
fn render_all_renders_scenery_sprites() {
    Pix3D::init_colour_table(0.6);
    let mut world = flat_world();
    // A scenery sprite on tile (1,2), in front of the camera. The typecode
    // is a real loc typecode (bits 29-30 = 2, so get_scene finds it).
    let ok = world.add_scenery(
        0,
        1,
        2,
        2000,
        Some(SceneModel::Model(one_face_model())),
        0x40000000 + (5 << 14),
        0,
        1,
        1,
        0,
    );
    assert!(ok);

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        world.update_mouse_picking(256, 134);
        world.render_all(&mut pix, &mut surface, &Cache::default(), 0, 192, 0, 192, 0, 0, 512);
    }

    // The sprite's model is rendered this cycle (cycle stamped) and the
    // viewport got pixels from it and/or the ground.
    let sprite = world.get_scene(0, 1, 2).expect("sprite on tile (1,2)");
    assert_eq!(sprite.cycle, 1, "sprite must be rendered once this cycle");
    assert!(map.pixels.iter().any(|&p| p != 0));
}
