//! Nav debug tile/hull paint. The host retains scene-relative nav facts;
//! the wgpu scene stage projects them for the current camera and emits
//! alpha-blended GPU triangles directly over the 3D target. No Pix2D
//! viewport raster or chrome-atlas upload participates.
//!
//! CpuPix3D and skip-paint slots do not request a retained view. Tiles are
//! projected at terrain height; hulls stroke the live loc model's
//! eight-corner AABB. Loc picking is untouched: hulls read the model but
//! never set `use_aabb_mouse_check`.

use crate::client::client::Client;
use crate::graphics::Pix3D;
use crate::render::draw::get_av_h;
use crate::render::Renderer;

/// Packed face-block bits of a `NavDebugCell` (the NSEW letters).
pub const FACE_N: u8 = 0x1;
pub const FACE_S: u8 = 0x2;
pub const FACE_E: u8 = 0x4;
pub const FACE_W: u8 = 0x8;
pub const CORNER_NE: u8 = 0x10;
pub const CORNER_SE: u8 = 0x20;
pub const CORNER_NW: u8 = 0x40;
pub const CORNER_SW: u8 = 0x80;

/// One painted collision tile: scene coords + packed face-block bits.
#[derive(Clone, Copy, Debug, Default)]
pub struct NavDebugCell {
    pub lx: i32,
    pub lz: i32,
    /// Packed N/S/E/W face and diagonal corner-block bits.
    pub bits: u8,
    /// Blanket blocked ground. Only blocked tiles paint the collision
    /// fill; a face-only cell (bare `W_*` flag) keeps its NSEW letters
    /// but draws no fill quad.
    pub blocked: bool,
    /// Whether the transport network reaches the tile (the host's paint-
    /// only reach bitset; `true` when the host has no bitset). A standable
    /// cell the network never reaches (`!reach`) draws the optional reach
    /// fill under `show_collision` — the walled-courtyard puddle.
    pub reach: bool,
}

/// A live loc hull target: the model at scene tile (`scene_x`, `scene_z`)
/// is stroked with its eight-corner AABB. Paint only — loc picking never
/// changes (`use_aabb_mouse_check` stays false).
#[derive(Clone, Copy, Debug)]
pub struct NavDebugHull {
    pub loc_id: i32,
    pub scene_x: i32,
    pub scene_z: i32,
}

/// RGB bytes for the nav debug layers. Defaults are the design spec's
/// reserved colours; the host/panel overrides them.
#[derive(Clone, Copy, Debug)]
pub struct NavDebugColors {
    /// Collision blocked-tile fill (rs2b0t reserved `#0080FF`).
    pub collision: [u8; 3],
    /// NSEW face-block letters (hop-label white).
    pub nsew: [u8; 3],
    /// Baked remaining path tiles.
    pub path: [u8; 3],
    /// Transport hops (and the loc hulls).
    pub path_hop: [u8; 3],
    /// Client `tryMove` trail (run off).
    pub trail: [u8; 3],
    /// Client trail run-alt tone (run on).
    pub trail_run: [u8; 3],
    /// Live loc hull stroke (transport colour).
    pub hull: [u8; 3],
    /// Click-target outline on the current walk tile.
    pub click: [u8; 3],
    /// Reach overlay fill for standable tiles the transport network never
    /// reaches (the flood-unreachable `#C828F0`; the host leaves this at
    /// the default — no nav setting drives it yet).
    pub reach: [u8; 3],
}

impl Default for NavDebugColors {
    fn default() -> Self {
        NavDebugColors {
            collision: [0x00, 0x80, 0xff],
            nsew: [0xff, 0xff, 0xff],
            path: [0xff, 0x00, 0x00],
            path_hop: [0x00, 0xff, 0x00],
            trail: [0x00, 0xd4, 0xff],
            trail_run: [0xff, 0xff, 0x00],
            hull: [0x00, 0xff, 0x00],
            click: [0xff, 0xff, 0xff],
            reach: [0xc8, 0x28, 0xf0],
        }
    }
}

/// The retained scene paint the GPU backend projects when nav facts change.
#[derive(Clone, Debug, Default)]
pub struct NavDebugPaint {
    /// Blocked collision tiles, scene lx/lz + packed face bits.
    pub collision: Vec<NavDebugCell>,
    /// Remaining baked path tiles (lx, lz, transport hop).
    pub path: Vec<(i32, i32, bool)>,
    /// Client `tryMove` trail tiles (lx, lz, run-alt tone).
    pub trail: Vec<(i32, i32, bool)>,
    /// Live loc hulls (loc id, world tile of the loc).
    pub hulls: Vec<NavDebugHull>,
    /// Hop captions (scene lx, lz, kind word) at `at` only.
    pub labels: Vec<(i32, i32, String)>,
    /// Current walk(aim) scene tile.
    pub click: Option<(i32, i32)>,
    /// RGB bytes from the panel.
    pub colors: NavDebugColors,
    pub show_collision: bool,
    pub show_nsew: bool,
    pub show_path: bool,
    pub show_trail: bool,
    pub show_hulls: bool,
}

/// rs2b0t `resolveNavPathPaintTheme` alphas (0..256; `Pix3D.trans` is the
/// background weight, so the paint's src weight is `256 - trans`).
const WALK_FILL_ALPHA: i32 = 82; // 0.32
const HOP_FILL_ALPHA: i32 = 128; // 0.5
const STROKE_ALPHA: i32 = 230; // 0.9
const HOP_STROKE_ALPHA: i32 = 243; // 0.95

/// Scene units per tile (128ths of a tile, the engine's scene grid).
const TILE: i32 = 128;

/// A projected tile quad, drawn far-to-near so nearer tiles overdraw.
pub(crate) struct Quad {
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

/// One screen-space GPU overlay vertex. The backend uploads these directly
/// into its existing scene vertex buffer after the world submission; no
/// viewport pixels or coverage map are materialized on the CPU.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct NavVertex {
    pub x: f32,
    pub y: f32,
    pub rgba: [u8; 4],
}

/// Reusable projected nav triangles and ordering/dedup scratch.
#[derive(Default)]
pub(crate) struct NavMesh {
    pub vertices: Vec<NavVertex>,
    quads: Vec<Quad>,
    edges: std::collections::HashSet<(i32, i32, i32, i32)>,
}

impl NavMesh {
    /// Drop high-water allocations as soon as this backend stops painting.
    pub(crate) fn release(&mut self) {
        *self = Self::default();
    }

    #[cfg(test)]
    pub(crate) fn retained_capacity(&self) -> usize {
        self.vertices.capacity() + self.quads.capacity() + self.edges.capacity()
    }
}

/// Project the retained host paint into GPU triangles for this camera.
/// Active paint reuses allocations; the backend calls [`NavMesh::release`]
/// when the retained paint disappears.
pub(crate) fn build_gpu_mesh(client: &mut Client, r: &mut Renderer, mesh: &mut NavMesh) {
    mesh.vertices.clear();
    mesh.quads.clear();
    mesh.edges.clear();
    let out = &mut mesh.vertices;
    let quads = &mut mesh.quads;
    let edges = &mut mesh.edges;
    let Some(paint) = client.nav_debug_paint.take() else {
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
        client.nav_debug_paint = Some(paint);
        return;
    }

    // Fill tiles far-to-near, preserving the established painter order.
    // Filled tiles retain painter order in reusable backend scratch.
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
    for quad in quads.iter() {
        append_filled_quad(out, quad);
        append_quad_stroke(out, quad);
    }

    if paint.show_collision {
        // Shared tile faces are emitted once, as in the Pix2D path.
        for cell in &paint.collision {
            for (bit, x, z, ex, ez) in cardinal_edges(cell) {
                if cell.bits & bit == 0 || !edges.insert((x, z, ex, ez)) {
                    continue;
                }
                append_projected_line(
                    client,
                    r,
                    out,
                    (x, z),
                    (ex, ez),
                    rgb(paint.colors.collision),
                    STROKE_ALPHA,
                );
            }
            for (bit, ax, az, bx, bz) in corner_strokes(cell) {
                if cell.bits & bit != 0 {
                    append_projected_line(
                        client,
                        r,
                        out,
                        (ax, az),
                        (bx, bz),
                        rgb(paint.colors.collision),
                        STROKE_ALPHA,
                    );
                }
            }
        }
    }

    if paint.show_nsew {
        for cell in &paint.collision {
            append_nsew(client, r, out, cell, rgb(paint.colors.nsew));
        }
    }
    if let Some((lx, lz)) = paint.click {
        append_tile_stroke(
            client,
            r,
            out,
            lx,
            lz,
            rgb(paint.colors.click),
            STROKE_ALPHA,
        );
    }
    if paint.show_hulls {
        for hull in &paint.hulls {
            append_hull(client, r, out, hull, rgb(paint.colors.hull));
        }
    }
    if !paint.labels.is_empty() {
        let colour = rgb(paint.colors.nsew);
        for &(lx, lz, ref text) in &paint.labels {
            append_hop_label(client, r, out, quads, (lx, lz), text, colour);
        }
    }
    client.nav_debug_paint = Some(paint);
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

fn rgba(colour: i32, alpha: i32) -> [u8; 4] {
    [
        ((colour >> 16) & 0xff) as u8,
        ((colour >> 8) & 0xff) as u8,
        (colour & 0xff) as u8,
        alpha.clamp(0, 255) as u8,
    ]
}

fn append_triangle(
    out: &mut Vec<NavVertex>,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    colour: i32,
    alpha: i32,
) {
    let rgba = rgba(colour, alpha);
    out.extend([
        NavVertex {
            x: a.0,
            y: a.1,
            rgba,
        },
        NavVertex {
            x: b.0,
            y: b.1,
            rgba,
        },
        NavVertex {
            x: c.0,
            y: c.1,
            rgba,
        },
    ]);
}

fn append_filled_quad(out: &mut Vec<NavVertex>, quad: &Quad) {
    let p = [
        (quad.x[0] as f32, quad.y[0] as f32),
        (quad.x[1] as f32, quad.y[1] as f32),
        (quad.x[2] as f32, quad.y[2] as f32),
        (quad.x[3] as f32, quad.y[3] as f32),
    ];
    append_triangle(out, p[0], p[1], p[2], quad.colour, quad.fill_alpha);
    append_triangle(out, p[0], p[2], p[3], quad.colour, quad.fill_alpha);
}

/// Re-emit each non-black fill under a caption at the caption opacity. The
/// Pix2D path raises the coverage of existing non-black caption-box pixels;
/// clipping the retained fill polygon preserves its RGB while doing the same.
fn append_caption_fill(
    out: &mut Vec<NavVertex>,
    quad: &Quad,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) {
    if quad.colour & 0x00ff_ffff == 0 {
        return;
    }
    let mut polygon = [(0.0, 0.0); 8];
    for (i, point) in polygon.iter_mut().take(4).enumerate() {
        *point = (quad.x[i] as f32, quad.y[i] as f32);
    }
    let mut scratch = [(0.0, 0.0); 8];
    let mut len = 4;
    for (axis, bound, keep_greater) in [
        (0, left, true),
        (0, right, false),
        (1, top, true),
        (1, bottom, false),
    ] {
        len = clip_polygon_edge(&polygon, len, &mut scratch, axis, bound, keep_greater);
        std::mem::swap(&mut polygon, &mut scratch);
    }
    for i in 1..len.saturating_sub(1) {
        append_triangle(
            out,
            polygon[0],
            polygon[i],
            polygon[i + 1],
            quad.colour,
            STROKE_ALPHA,
        );
    }
}

fn clip_polygon_edge(
    input: &[(f32, f32); 8],
    input_len: usize,
    output: &mut [(f32, f32); 8],
    axis: usize,
    bound: f32,
    keep_greater: bool,
) -> usize {
    if input_len == 0 {
        return 0;
    }
    let coordinate = |point: (f32, f32)| if axis == 0 { point.0 } else { point.1 };
    let inside = |point: (f32, f32)| {
        let value = coordinate(point);
        if keep_greater {
            value >= bound
        } else {
            value <= bound
        }
    };
    let mut output_len = 0;
    let mut previous = input[input_len - 1];
    let mut previous_inside = inside(previous);
    for &current in input.iter().take(input_len) {
        let current_inside = inside(current);
        if current_inside != previous_inside {
            let previous_axis = coordinate(previous);
            let scale = (bound - previous_axis) / (coordinate(current) - previous_axis);
            let intersection = (
                previous.0 + (current.0 - previous.0) * scale,
                previous.1 + (current.1 - previous.1) * scale,
            );
            output[output_len] = intersection;
            output_len += 1;
        }
        if current_inside {
            output[output_len] = current;
            output_len += 1;
        }
        previous = current;
        previous_inside = current_inside;
    }
    output_len
}

fn append_quad_stroke(out: &mut Vec<NavVertex>, quad: &Quad) {
    for i in 0..4 {
        append_line(
            out,
            (quad.x[i], quad.y[i]),
            (quad.x[(i + 1) % 4], quad.y[(i + 1) % 4]),
            quad.colour,
            quad.stroke_alpha,
        );
    }
}

/// A one-pixel screen-space line expressed as two GPU triangles with square
/// endpoint extension. Integer coordinates name Pix2D pixel centres.
fn append_line(out: &mut Vec<NavVertex>, a: (i32, i32), b: (i32, i32), colour: i32, alpha: i32) {
    let dx = (b.0 - a.0) as f32;
    let dy = (b.1 - a.1) as f32;
    let len = dx.hypot(dy);
    if len == 0.0 {
        append_pixel(out, a.0, a.1, colour, alpha);
        return;
    }
    let tx = dx * 0.5 / len;
    let ty = dy * 0.5 / len;
    let nx = -ty;
    let ny = tx;
    let p0 = (a.0 as f32 - tx + nx, a.1 as f32 - ty + ny);
    let p1 = (b.0 as f32 + tx + nx, b.1 as f32 + ty + ny);
    let p2 = (b.0 as f32 + tx - nx, b.1 as f32 + ty - ny);
    let p3 = (a.0 as f32 - tx - nx, a.1 as f32 - ty - ny);
    append_triangle(out, p0, p1, p2, colour, alpha);
    append_triangle(out, p0, p2, p3, colour, alpha);
}

fn append_pixel(out: &mut Vec<NavVertex>, x: i32, y: i32, colour: i32, alpha: i32) {
    let p0 = (x as f32 - 0.5, y as f32 - 0.5);
    let p1 = (x as f32 + 0.5, y as f32 - 0.5);
    let p2 = (x as f32 + 0.5, y as f32 + 0.5);
    let p3 = (x as f32 - 0.5, y as f32 + 0.5);
    append_triangle(out, p0, p1, p2, colour, alpha);
    append_triangle(out, p0, p2, p3, colour, alpha);
}

fn append_projected_line(
    client: &Client,
    r: &Renderer,
    out: &mut Vec<NavVertex>,
    a: (i32, i32),
    b: (i32, i32),
    colour: i32,
    alpha: i32,
) {
    let a = r.project_overlay(client, a.0, a.1, 0);
    let b = r.project_overlay(client, b.0, b.1, 0);
    if a.0 != -1 && b.0 != -1 {
        append_line(out, a, b, colour, alpha);
    }
}

fn append_nsew(
    client: &Client,
    r: &Renderer,
    out: &mut Vec<NavVertex>,
    cell: &NavDebugCell,
    colour: i32,
) {
    let glyphs = [GLYPH_N, GLYPH_S, GLYPH_E, GLYPH_W];
    for ((bit, fx, fz), glyph) in nsew_centres(cell).into_iter().zip(glyphs) {
        if cell.bits & bit == 0 {
            continue;
        }
        let (px, py) = r.project_overlay(client, fx, fz, 0);
        if px != -1 {
            append_glyph(out, px - 2, py - 2, glyph, colour, STROKE_ALPHA);
        }
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

/// Caption at the projected tile centre using the same b12 glyph masks,
/// opaque one-pixel shadow and caption-box coverage as PixFont.
fn append_hop_label(
    client: &Client,
    r: &Renderer,
    out: &mut Vec<NavVertex>,
    fills: &[Quad],
    tile: (i32, i32),
    text: &str,
    colour: i32,
) {
    let x = tile.0.wrapping_mul(TILE) + TILE / 2;
    let z = tile.1.wrapping_mul(TILE) + TILE / 2;
    let (px, py) = r.project_overlay(client, x, z, 0);
    if px == -1 {
        return;
    }
    let Some(font) = r.media.b12.as_ref() else {
        return;
    };
    let width = font.string_wid(Some(text));
    let height = font.height.max(12);
    let left = (px - width / 2 - 1) as f32 - 0.5;
    let top = (py - height - 1) as f32 - 0.5;
    let right = (px + width / 2 + 1) as f32 + 0.5;
    let bottom = (py + 1) as f32 + 0.5;
    for fill in fills {
        append_caption_fill(out, fill, left, top, right, bottom);
    }
    let start_x = px - width / 2;
    append_font_text(out, font, text, start_x + 1, py + 1, 0, 255);
    append_font_text(out, font, text, start_x, py, colour, STROKE_ALPHA);
}

fn append_font_text(
    out: &mut Vec<NavVertex>,
    font: &crate::graphics::PixFont,
    text: &str,
    mut x: i32,
    baseline_y: i32,
    colour: i32,
    alpha: i32,
) {
    let y = baseline_y - font.height;
    for c in text.chars() {
        let code = c as usize;
        if code != 32 {
            let mask = font.char_mask.get(code).map(Vec::as_slice).unwrap_or(&[]);
            let ox = font.char_offset_x.get(code).copied().unwrap_or(0);
            let oy = font.char_offset_y.get(code).copied().unwrap_or(0);
            let width = font.char_mask_width.get(code).copied().unwrap_or(0);
            let height = font.char_mask_height.get(code).copied().unwrap_or(0);
            for row in 0..height {
                for col in 0..width {
                    let index = (col + row * width) as usize;
                    if mask.get(index).copied().unwrap_or(0) != 0 {
                        append_pixel(out, x + ox + col, y + oy + row, colour, alpha);
                    }
                }
            }
        }
        x += font.char_advance.get(code).copied().unwrap_or(0);
    }
}

fn append_glyph(out: &mut Vec<NavVertex>, x: i32, y: i32, glyph: [u8; 5], colour: i32, alpha: i32) {
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) != 0 {
                append_pixel(out, x + col as i32, y + row as i32, colour, alpha);
            }
        }
    }
}

fn append_tile_stroke(
    client: &Client,
    r: &Renderer,
    out: &mut Vec<NavVertex>,
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
        append_line(out, p[i], p[(i + 1) % 4], colour, alpha);
    }
}

/// Stroke a loc hull's eight-corner AABB from the live model at the hull's
/// scene tile. Loc picking remains untouched.
fn append_hull(
    client: &mut Client,
    r: &mut Renderer,
    out: &mut Vec<NavVertex>,
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
        append_line(out, screen[a], screen[b], colour, HOP_STROKE_ALPHA);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_only_cell_has_letters_but_no_fill() {
        // A W_S-only tile: standable ground with a south wall face. It
        // stays in the NSEW set yet must never paint the collision fill.
        let colors = NavDebugColors::default();
        let cell = NavDebugCell {
            lx: 0,
            lz: 0,
            bits: FACE_S,
            blocked: false,
            reach: false,
        };
        assert_ne!(cell.bits & FACE_S, 0, "face-only cell keeps its letters");
        assert_ne!(
            cell_fill(&cell, &colors),
            Some((rgb(colors.collision), WALK_FILL_ALPHA)),
            "face-only cells never paint the collision fill"
        );
        let blocked = NavDebugCell {
            blocked: true,
            ..cell
        };
        assert_eq!(
            cell_fill(&blocked, &colors),
            Some((rgb(colors.collision), WALK_FILL_ALPHA)),
            "blocked cells paint the collision fill"
        );
    }

    #[test]
    fn reach_fill_colours_only_unreached_standable_cells() {
        let colors = NavDebugColors::default();
        // A reached standable cell (open ground) draws no fill.
        let open = NavDebugCell {
            reach: true,
            ..Default::default()
        };
        assert_eq!(
            cell_fill(&open, &colors),
            None,
            "reached ground has no fill"
        );
        // The walled-courtyard puddle: standable, unreached → reach fill.
        let yard = NavDebugCell {
            reach: false,
            ..Default::default()
        };
        assert_eq!(
            cell_fill(&yard, &colors),
            Some((rgb(colors.reach), WALK_FILL_ALPHA)),
            "unreached standable cells draw the reach fill"
        );
        // Blocked ground keeps the collision colour even when unreached.
        let wall = NavDebugCell {
            blocked: true,
            reach: false,
            ..Default::default()
        };
        assert_eq!(
            cell_fill(&wall, &colors),
            Some((rgb(colors.collision), WALK_FILL_ALPHA)),
            "walls keep the collision fill"
        );
        let reached_edge = NavDebugCell {
            bits: FACE_N | CORNER_NE,
            reach: true,
            ..Default::default()
        };
        assert_eq!(
            cell_fill(&reached_edge, &colors),
            None,
            "reached standable edge/corner cells have no fill"
        );
    }

    #[test]
    fn nsew_letters_sit_on_the_right_edges() {
        // +z is north: N on the far (z+TILE) edge, S on the near (z) edge.
        let cell = NavDebugCell {
            lx: 10,
            lz: 20,
            ..Default::default()
        };
        let centres = nsew_centres(&cell);
        let (north, south, east, west) = (
            centres
                .iter()
                .find(|(bit, _, _)| *bit == FACE_N)
                .expect("N centre"),
            centres
                .iter()
                .find(|(bit, _, _)| *bit == FACE_S)
                .expect("S centre"),
            centres
                .iter()
                .find(|(bit, _, _)| *bit == FACE_E)
                .expect("E centre"),
            centres
                .iter()
                .find(|(bit, _, _)| *bit == FACE_W)
                .expect("W centre"),
        );
        assert_eq!((north.1, north.2), (10 * TILE + TILE / 2, 21 * TILE));
        assert_eq!((south.1, south.2), (10 * TILE + TILE / 2, 20 * TILE));
        assert_eq!((east.1, east.2), (11 * TILE, 20 * TILE + TILE / 2));
        assert_eq!((west.1, west.2), (10 * TILE, 20 * TILE + TILE / 2));
    }

    #[test]
    fn cardinal_edges_span_each_oriented_tile_boundary() {
        let cell = NavDebugCell {
            lx: 10,
            lz: 20,
            ..Default::default()
        };
        assert_eq!(
            cardinal_edges(&cell),
            [
                (FACE_N, 10 * TILE, 21 * TILE, 11 * TILE, 21 * TILE),
                (FACE_S, 10 * TILE, 20 * TILE, 11 * TILE, 20 * TILE),
                (FACE_E, 11 * TILE, 20 * TILE, 11 * TILE, 21 * TILE),
                (FACE_W, 10 * TILE, 20 * TILE, 10 * TILE, 21 * TILE),
            ]
        );
    }

    #[test]
    fn diagonal_markers_point_inward_from_each_corner() {
        let cell = NavDebugCell {
            lx: 2,
            lz: 3,
            ..Default::default()
        };
        let q = TILE / 4;
        let marks = corner_strokes(&cell);
        assert_eq!(
            marks[0],
            (CORNER_NE, 3 * TILE, 4 * TILE, 3 * TILE - q, 4 * TILE - q)
        );
        assert_eq!(
            marks[1],
            (CORNER_SE, 3 * TILE, 3 * TILE, 3 * TILE - q, 3 * TILE + q)
        );
        assert_eq!(
            marks[2],
            (CORNER_NW, 2 * TILE, 4 * TILE, 2 * TILE + q, 4 * TILE - q)
        );
        assert_eq!(
            marks[3],
            (CORNER_SW, 2 * TILE, 3 * TILE, 2 * TILE + q, 3 * TILE + q)
        );
    }
}
