//! Frozen Pix2D nav-paint reference used only by GPU pixel-parity tests.
//! This is the renderer replaced by the direct GPU overlay: each primitive
//! writes source RGB and coverage alpha with last-writer-wins semantics, then
//! the chrome composite blends that one retained value over the 3D scene.
//!
//! Keeping the oracle under `cfg(test)` lets the GPU test run both paths with
//! the same camera and paint, rather than pinning an incidental screenshot.
//! Paint is scene-relative (the host already scene-clips collision cells);
//! tiles project at terrain height and hulls use the live loc AABB.

use crate::client::client::Client;
use crate::graphics::{Pix2D, Pix3D};
use crate::render::draw::get_av_h;
use crate::render::Renderer;

#[allow(unused_imports)]
use crate::render::nav_debug::{
    NavDebugCell, NavDebugColors, NavDebugHull, NavDebugPaint, CORNER_NE, CORNER_NW, CORNER_SE,
    CORNER_SW, FACE_E, FACE_N, FACE_S, FACE_W,
};

/// rs2b0t `resolveNavPathPaintTheme` alphas (0..256; `Pix3D.trans` is the
/// background weight, so the paint's src weight is `256 - trans`).
const WALK_FILL_ALPHA: i32 = 82; // 0.32
const HOP_FILL_ALPHA: i32 = 128; // 0.5
const STROKE_ALPHA: i32 = 230; // 0.9
const HOP_STROKE_ALPHA: i32 = 243; // 0.95

/// Scene units per tile (128ths of a tile, the engine's scene grid).
const TILE: i32 = 128;

/// A projected tile quad, drawn far-to-near so nearer tiles overdraw.
struct Quad {
    depth: i32,
    x: [i32; 4],
    y: [i32; 4],
    colour: i32,
    fill_alpha: i32,
    stroke_alpha: i32,
}

/// 5×5 dot glyphs for the NSEW face letters (bit 4 = leftmost column).
const GLYPH_N: [u8; 5] = [0b10001, 0b11001, 0b10101, 0b10011, 0b10001];
const GLYPH_S: [u8; 5] = [0b01110, 0b10000, 0b01110, 0b00001, 0b01110];
const GLYPH_E: [u8; 5] = [0b11111, 0b10000, 0b11110, 0b10000, 0b11111];
const GLYPH_W: [u8; 5] = [0b10001, 0b10001, 0b10101, 0b10101, 0b01010];

fn rgb(bytes: [u8; 3]) -> i32 {
    ((bytes[0] as i32) << 16) | ((bytes[1] as i32) << 8) | bytes[2] as i32
}

/// Draw the stored paint into the bound test surface using the retired
/// Pix2D raster and coverage semantics.
pub(crate) fn draw(client: &mut Client, r: &mut Renderer, surface: &mut Pix2D) {
    let Some(paint) = client.nav_debug_paint().cloned() else {
        return;
    };
    let show_any = paint.show_collision
        || paint.show_nsew
        || paint.show_path
        || paint.show_trail
        || paint.show_hulls
        || !paint.labels.is_empty()
        || paint.click.is_some();
    if !show_any {
        return;
    }

    // The projection origin the overlay passes read (`set_clipping` also
    // binds the scanline the fills borrow; nothing else is touched).
    r.pix3d.set_clipping(surface.width, surface.height);

    // Tile fills, far-to-near so nearer tiles overdraw.
    let mut quads: Vec<Quad> = Vec::new();
    if paint.show_collision {
        for cell in &paint.collision {
            let Some((colour, fill_alpha)) = cell_fill(cell, &paint.colors) else {
                continue;
            };
            if let Some(quad) = tile_quad(
                client,
                r,
                cell.lx,
                cell.lz,
                colour,
                fill_alpha,
                STROKE_ALPHA,
            ) {
                quads.push(quad);
            }
        }
    }
    if paint.show_path {
        for &(lx, lz, transport) in &paint.path {
            let (colour, fill, stroke) = if transport {
                (rgb(paint.colors.path_hop), HOP_FILL_ALPHA, HOP_STROKE_ALPHA)
            } else {
                (rgb(paint.colors.path), WALK_FILL_ALPHA, STROKE_ALPHA)
            };
            if let Some(quad) = tile_quad(client, r, lx, lz, colour, fill, stroke) {
                quads.push(quad);
            }
        }
    }
    if paint.show_trail {
        for &(lx, lz, run_alt) in &paint.trail {
            let colour = if run_alt {
                rgb(paint.colors.trail_run)
            } else {
                rgb(paint.colors.trail)
            };
            if let Some(quad) =
                tile_quad(client, r, lx, lz, colour, HOP_FILL_ALPHA, HOP_STROKE_ALPHA)
            {
                quads.push(quad);
            }
        }
    }
    quads.sort_by_key(|quad| std::cmp::Reverse(quad.depth));
    for quad in &quads {
        fill_quad(surface, quad);
        stroke_quad(surface, quad);
    }

    // Edges are independent of fills so face-only standable cells remain
    // visible. Diagonal bits are short corner markers, not full diagonals.
    if paint.show_collision {
        let mut edges = std::collections::HashSet::new();
        for cell in &paint.collision {
            for (bit, x, z, ex, ez) in cardinal_edges(cell) {
                if cell.bits & bit == 0 || !edges.insert((x, z, ex, ez)) {
                    continue;
                }
                let a = r.project_overlay(client, x, z, 0);
                let b = r.project_overlay(client, ex, ez, 0);
                if a.0 != -1 && b.0 != -1 {
                    stroke_line(
                        surface,
                        a.0,
                        a.1,
                        b.0,
                        b.1,
                        rgb(paint.colors.collision),
                        STROKE_ALPHA,
                    );
                }
            }
            for (bit, ax, az, bx, bz) in corner_strokes(cell) {
                if cell.bits & bit == 0 {
                    continue;
                }
                let a = r.project_overlay(client, ax, az, 0);
                let b = r.project_overlay(client, bx, bz, 0);
                if a.0 != -1 && b.0 != -1 {
                    stroke_line(
                        surface,
                        a.0,
                        a.1,
                        b.0,
                        b.1,
                        rgb(paint.colors.collision),
                        STROKE_ALPHA,
                    );
                }
            }
        }
    }

    if paint.show_nsew {
        for cell in &paint.collision {
            draw_nsew(client, r, surface, cell, rgb(paint.colors.nsew));
        }
    }

    if let Some((lx, lz)) = paint.click {
        stroke_tile(
            client,
            r,
            surface,
            lx,
            lz,
            rgb(paint.colors.click),
            STROKE_ALPHA,
        );
    }

    if paint.show_hulls {
        for hull in &paint.hulls {
            draw_hull(client, r, surface, hull, rgb(paint.colors.hull));
        }
    }

    if !paint.labels.is_empty() {
        let colour = rgb(paint.colors.nsew);
        for &(lx, lz, ref text) in &paint.labels {
            draw_hop_label(client, r, surface, lx, lz, text, colour);
        }
    }
}

/// Project a tile's four ground corners; `None` when any corner fails to
/// project (behind the camera or off the playable scene — the path may
/// include tiles the projection cannot see).
fn tile_quad(
    client: &Client,
    r: &Renderer,
    lx: i32,
    lz: i32,
    colour: i32,
    fill_alpha: i32,
    stroke_alpha: i32,
) -> Option<Quad> {
    let x0 = lx.wrapping_mul(TILE);
    let z0 = lz.wrapping_mul(TILE);
    let p0 = r.project_overlay(client, x0, z0, 0);
    let p1 = r.project_overlay(client, x0 + TILE, z0, 0);
    let p2 = r.project_overlay(client, x0 + TILE, z0 + TILE, 0);
    let p3 = r.project_overlay(client, x0, z0 + TILE, 0);
    if p0.0 == -1 || p1.0 == -1 || p2.0 == -1 || p3.0 == -1 {
        return None;
    }
    let depth = scene_depth(client, x0 + TILE / 2, z0 + TILE / 2, 0);
    Some(Quad {
        depth,
        x: [p0.0, p1.0, p2.0, p3.0],
        y: [p0.1, p1.1, p2.1, p3.1],
        colour,
        fill_alpha,
        stroke_alpha,
    })
}

/// Camera-space depth of a scene point at a height — the same fixed-point
/// projection `project_overlay` uses (`z'`), for the far-to-near order.
fn scene_depth(client: &Client, x: i32, z: i32, height: i32) -> i32 {
    let y = get_av_h(&client.groundh, &client.mapl, x, z, client.minusedlevel) - height;
    let dx = x - client.cam_x;
    let dy = y - client.cam_y;
    let dz = z - client.cam_z;
    let sin_pitch = Pix3D::sin_table()[(client.cam_pitch & 0x7ff) as usize];
    let cos_pitch = Pix3D::cos_table()[(client.cam_pitch & 0x7ff) as usize];
    let sin_yaw = Pix3D::sin_table()[(client.cam_yaw & 0x7ff) as usize];
    let cos_yaw = Pix3D::cos_table()[(client.cam_yaw & 0x7ff) as usize];
    let var14 = dz
        .wrapping_mul(cos_yaw)
        .wrapping_sub(dx.wrapping_mul(sin_yaw))
        >> 16;
    dy.wrapping_mul(sin_pitch)
        .wrapping_add(var14.wrapping_mul(cos_pitch))
        >> 16
}

/// Scanline-fill a convex projected quad. RGB is the layer colour; overlay
/// alpha is the coverage byte so the chrome composite blends over the 3D
/// scene (rs2b0t `fillQuadPix`).
fn fill_quad(surface: &mut Pix2D, quad: &Quad) {
    let [x0, x1, x2, x3] = quad.x;
    let [y0, y1, y2, y3] = quad.y;
    let min_y = y0.min(y1).min(y2).min(y3).max(0);
    let max_y = y0.max(y1).max(y2).max(y3).min(surface.height - 1);
    let width = surface.width;
    let edges = [
        (x0, y0, x1, y1),
        (x1, y1, x2, y2),
        (x2, y2, x3, y3),
        (x3, y3, x0, y0),
    ];
    let mut hits = [0f32; 2];
    for y in min_y..=max_y {
        let mut n = 0usize;
        for (ax, ay, bx, by) in edges {
            // Half-open edge rule: a horizontal scanline crosses an edge
            // once per vertex, so a convex quad yields exactly two hits.
            if (ay < by && (ay..by).contains(&y)) || (by < ay && (by..ay).contains(&y)) {
                let t = (y - ay) as f32 / (by - ay) as f32;
                let x = ax as f32 + t * (bx - ax) as f32;
                if n < 2 {
                    hits[n] = x;
                    n += 1;
                }
            }
        }
        if n != 2 {
            continue;
        }
        let lo = hits[0].min(hits[1]).ceil() as i32;
        let hi = hits[0].max(hits[1]).floor() as i32;
        for x in lo.max(0)..=hi.min(width - 1) {
            plot_overlay(surface, y * width + x, quad.colour, quad.fill_alpha);
        }
    }
}

/// Stroke the projected quad (rs2b0t `strokeQuadPix`).
fn stroke_quad(surface: &mut Pix2D, quad: &Quad) {
    for i in 0..4 {
        let a = (quad.x[i], quad.y[i]);
        let b = (quad.x[(i + 1) % 4], quad.y[(i + 1) % 4]);
        stroke_line(surface, a.0, a.1, b.0, b.1, quad.colour, quad.stroke_alpha);
    }
}

/// Write overlay RGB and coverage alpha. The GPU chrome pass blends this
/// over the 3D scene; do not pre-blend onto the cleared `area_game` black
/// (that is what made tiles look solid).
fn plot_overlay(surface: &mut Pix2D, off: i32, rgb: i32, alpha: i32) {
    surface.pixels[off as usize] = rgb & 0x00ff_ffff;
    surface.mark_pixel_alpha(off, alpha.clamp(0, 255) as u8);
}

/// The NSEW face letters of a collision cell, one per blocked face at the
/// face-centre projection (`project_overlay`).
fn draw_nsew(client: &Client, r: &Renderer, surface: &mut Pix2D, cell: &NavDebugCell, colour: i32) {
    let glyphs = [GLYPH_N, GLYPH_S, GLYPH_E, GLYPH_W];
    for ((bit, fx, fz), glyph) in nsew_centres(cell).into_iter().zip(glyphs) {
        if cell.bits & bit == 0 {
            continue;
        }
        let (px, py) = r.project_overlay(client, fx, fz, 0);
        if px == -1 {
            continue;
        }
        plot_glyph(surface, px - 2, py - 2, glyph, colour, STROKE_ALPHA);
    }
}

/// The fill a collision cell draws under `show_collision`: blocked ground
/// keeps the collision colour; standable ground the host's reach bitset
/// never marks (`!reach`) draws the optional reach colour (the walled-
/// courtyard puddle); a reached standable cell draws nothing.
fn cell_fill(cell: &NavDebugCell, colors: &NavDebugColors) -> Option<(i32, i32)> {
    if cell.blocked {
        Some((rgb(colors.collision), WALK_FILL_ALPHA))
    } else if !cell.reach {
        Some((rgb(colors.reach), WALK_FILL_ALPHA))
    } else {
        None
    }
}

/// NSEW letter centres in scene coords. +z is north: the N letter sits on
/// the far (`z + TILE`) edge, S on the near (`z`) edge, E/W on the +x/−x
/// mid edges.
fn nsew_centres(cell: &NavDebugCell) -> [(u8, i32, i32); 4] {
    let x = cell.lx.wrapping_mul(TILE);
    let z = cell.lz.wrapping_mul(TILE);
    [
        (FACE_N, x + TILE / 2, z + TILE),
        (FACE_S, x + TILE / 2, z),
        (FACE_E, x + TILE, z + TILE / 2),
        (FACE_W, x, z + TILE / 2),
    ]
}

fn cardinal_edges(cell: &NavDebugCell) -> [(u8, i32, i32, i32, i32); 4] {
    let x = cell.lx.wrapping_mul(TILE);
    let z = cell.lz.wrapping_mul(TILE);
    [
        (FACE_N, x, z + TILE, x + TILE, z + TILE),
        (FACE_S, x, z, x + TILE, z),
        (FACE_E, x + TILE, z, x + TILE, z + TILE),
        (FACE_W, x, z, x, z + TILE),
    ]
}

/// Short inward markers at blocked diagonal crossings. Each marker is
/// clipped to the corner and never implies that the whole tile is blocked.
fn corner_strokes(cell: &NavDebugCell) -> [(u8, i32, i32, i32, i32); 4] {
    let x = cell.lx.wrapping_mul(TILE);
    let z = cell.lz.wrapping_mul(TILE);
    let q = TILE / 4;
    [
        (CORNER_NE, x + TILE, z + TILE, x + TILE - q, z + TILE - q),
        (CORNER_SE, x + TILE, z, x + TILE - q, z + q),
        (CORNER_NW, x, z + TILE, x + q, z + TILE - q),
        (CORNER_SW, x, z, x + q, z + q),
    ]
}

/// Caption at the projected tile centre (rs2b0t `drawHopLabel`). Uses
/// `b12` when loaded; otherwise a 5×5 fallback is skipped (no tiny noise).
fn draw_hop_label(
    client: &Client,
    r: &Renderer,
    surface: &mut Pix2D,
    lx: i32,
    lz: i32,
    text: &str,
    colour: i32,
) {
    let x = lx.wrapping_mul(TILE) + TILE / 2;
    let z = lz.wrapping_mul(TILE) + TILE / 2;
    let (px, py) = r.project_overlay(client, x, z, 0);
    if px == -1 {
        return;
    }
    let Some(font) = r.media.b12.as_ref() else {
        return;
    };
    font.centre_string(surface, Some(text), px + 1, py + 1, 0);
    font.centre_string(surface, Some(text), px, py, colour);
    let w = font.string_wid(Some(text));
    let h = font.height.max(12);
    let x0 = px - w / 2 - 1;
    let y0 = py - h - 1;
    for row in y0..=py + 1 {
        for col in x0..=px + w / 2 + 1 {
            if col < 0 || row < 0 || col >= surface.width || row >= surface.height {
                continue;
            }
            let off = row * surface.width + col;
            if surface.pixels[off as usize] & 0x00ff_ffff != 0 {
                surface.mark_pixel_alpha(off, STROKE_ALPHA.clamp(0, 255) as u8);
            }
        }
    }
}

fn plot_glyph(surface: &mut Pix2D, x: i32, y: i32, glyph: [u8; 5], colour: i32, alpha: i32) {
    let width = surface.width;
    let height = surface.height;
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) == 0 {
                continue;
            }
            let px = x + col as i32;
            let py = y + row as i32;
            if px < 0 || py < 0 || px >= width || py >= height {
                continue;
            }
            plot_overlay(surface, py * width + px, colour, alpha);
        }
    }
}

/// Stroke a tile's projected outline (the click-target square).
fn stroke_tile(
    client: &Client,
    r: &Renderer,
    surface: &mut Pix2D,
    lx: i32,
    lz: i32,
    colour: i32,
    alpha: i32,
) {
    let x0 = lx.wrapping_mul(TILE);
    let z0 = lz.wrapping_mul(TILE);
    let p = [
        r.project_overlay(client, x0, z0, 0),
        r.project_overlay(client, x0 + TILE, z0, 0),
        r.project_overlay(client, x0 + TILE, z0 + TILE, 0),
        r.project_overlay(client, x0, z0 + TILE, 0),
    ];
    if p.iter().any(|&(px, _)| px == -1) {
        return;
    }
    for i in 0..4 {
        let a = p[i];
        let b = p[(i + 1) % 4];
        stroke_line(surface, a.0, a.1, b.0, b.1, colour, alpha);
    }
}

/// Bresenham line, clipped per pixel and coverage-marked.
fn stroke_line(surface: &mut Pix2D, x0: i32, y0: i32, x1: i32, y1: i32, colour: i32, alpha: i32) {
    let width = surface.width;
    let height = surface.height;
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x >= 0 && y >= 0 && x < width && y < height {
            plot_overlay(surface, y * width + x, colour, alpha);
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Stroke a loc hull's eight-corner AABB from the live model at the hull's
/// scene tile. Skips when the loc is not in the loaded scene; the loc's
/// pick path is never touched.
fn draw_hull(
    client: &mut Client,
    r: &mut Renderer,
    surface: &mut Pix2D,
    hull: &NavDebugHull,
    colour: i32,
) {
    let Some((pos_x, pos_y, pos_z, yaw, model)) = r.world.loc_model_at(
        &client.world,
        &client.cache,
        client.loop_cycle,
        client.minusedlevel,
        hull.scene_x,
        hull.scene_z,
        hull.loc_id,
    ) else {
        return;
    };
    let (Some(point_x), Some(point_y), Some(point_z)) =
        (&model.point_x, &model.point_y, &model.point_z)
    else {
        return;
    };
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    let mut min_z = i32::MAX;
    let mut max_z = i32::MIN;
    for i in 0..model.num_points as usize {
        let (Some(&x), Some(&y), Some(&z)) = (point_x.get(i), point_y.get(i), point_z.get(i))
        else {
            continue;
        };
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    if min_x == i32::MAX {
        return;
    }
    let base = [
        (min_x, min_y, min_z),
        (max_x, min_y, min_z),
        (max_x, min_y, max_z),
        (min_x, min_y, max_z),
        (min_x, max_y, min_z),
        (max_x, max_y, min_z),
        (max_x, max_y, max_z),
        (min_x, max_y, max_z),
    ];
    let (sin_yaw, cos_yaw) = if yaw != 0 {
        (
            Pix3D::sin_table()[(yaw & 0x7ff) as usize],
            Pix3D::cos_table()[(yaw & 0x7ff) as usize],
        )
    } else {
        (0, 0)
    };
    let mut screen = [(-1, -1); 8];
    for (i, &(mx, my, mz)) in base.iter().enumerate() {
        // The engine's model-space yaw rotate, then the scene translation.
        let (mut x, y, mut z) = (mx, my, mz);
        if yaw != 0 {
            let temp = (z
                .wrapping_mul(sin_yaw)
                .wrapping_add(x.wrapping_mul(cos_yaw)))
                >> 16;
            z = (z
                .wrapping_mul(cos_yaw)
                .wrapping_sub(x.wrapping_mul(sin_yaw)))
                >> 16;
            x = temp;
        }
        let sx = x.wrapping_add(pos_x);
        let sy = y.wrapping_add(pos_y);
        let sz = z.wrapping_add(pos_z);
        let avh = get_av_h(&client.groundh, &client.mapl, sx, sz, client.minusedlevel);
        screen[i] = r.project_overlay(client, sx, sz, avh - sy);
    }
    if screen.iter().any(|&(px, _)| px == -1) {
        return;
    }
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    for (a, b) in EDGES {
        let (ax, ay) = screen[a];
        let (bx, by) = screen[b];
        stroke_line(surface, ax, ay, bx, by, colour, HOP_STROKE_ALPHA);
    }
}
