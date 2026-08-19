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

/// A 64×64 checkerboard texture (palette indices 1/2 in 32×32 blocks) so
/// the textured quick-ground path samples visibly different texels for the
/// TS flat vs non-flat texture-corner branches.
fn checkerboard_pix() -> Pix3DDraw {
    let mut texture = client::graphics::Pix8::new(64, 64, vec![0, 0xff0000, 0x0000ff]);
    for y in 0..64 {
        for x in 0..64 {
            texture.data[y * 64 + x] = if (x < 32) != (y < 32) { 1 } else { 2 };
        }
    }
    let mut pix = Pix3DDraw::default();
    pix.low_mem = false;
    pix.textures[0] = Some(texture);
    pix.tex_pal[0] = Some(vec![0, 0xff0000, 0x0000ff]);
    pix.num_textures = 1;
    pix.init_pool(1);
    pix
}

/// A world whose only tile content is a DIAGONAL quick ground at (1,2)
/// textured with id 0. `non_flat` flips only the `QuickGround.flat` flag
/// (via differing `set_ground` heights); `groundh` stays flat at 2000 in
/// both cases so the projection geometry is identical and only the
/// texture-corner branch can differ.
fn diagonal_world(non_flat: bool) -> World {
    let max_level = 1;
    let max_tile_x = 3;
    let max_tile_z = 3;
    let groundh = vec![vec![vec![2000i32; max_tile_z as usize + 1]; max_tile_x as usize + 1]; max_level as usize];
    let mut world = World::new(groundh, max_tile_z, max_level, max_tile_x);
    world.fill_base_level(0);
    let (h_sw, h_se, h_ne, h_nw) = if non_flat { (2000, 2000, 2000, 2100) } else { (2000, 2000, 2000, 2000) };
    world.set_ground(
        0,
        1,
        2,
        TerrainOverlayShape::DIAGONAL,
        0,
        0,
        h_sw,
        h_se,
        h_ne,
        h_nw,
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
    world
}

#[test]
fn render_all_non_flat_diagonal_quick_ground_uses_own_corners() {
    Pix3D::init_colour_table(0.6);

    let mut flat_world = diagonal_world(false);
    let mut flat_pix = checkerboard_pix();
    let mut flat_map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut flat_map.pixels, flat_map.width, flat_map.height);
        flat_pix.set_render_clipping(&surface);
        flat_pix.trans = 0;
        flat_world.render_all(&mut flat_pix, &mut surface, &Cache::default(), 0, 192, 0, 192, 0, 0, 512);
    }

    let mut nonflat_world = diagonal_world(true);
    let mut nonflat_pix = checkerboard_pix();
    let mut nonflat_map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut nonflat_map.pixels, nonflat_map.width, nonflat_map.height);
        nonflat_pix.set_render_clipping(&surface);
        nonflat_pix.trans = 0;
        nonflat_world.render_all(&mut nonflat_pix, &mut surface, &Cache::default(), 0, 192, 0, 192, 0, 0, 512);
    }

    // Both renders must actually draw the textured ground ...
    assert!(flat_map.pixels.iter().any(|&p| p != 0), "flat quick ground must draw");
    assert!(nonflat_map.pixels.iter().any(|&p| p != 0), "non-flat quick ground must draw");
    // ... and the non-flat tile must map its own corners, not the flat
    // permuted ones (TS 2004-2029): identical geometry, different texture
    // sampling, so the two frames differ.
    assert_ne!(
        flat_map.pixels, nonflat_map.pixels,
        "non-flat DIAGONAL quick ground must use the non-flat texture corners"
    );
}
