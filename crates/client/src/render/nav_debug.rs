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
use crate::render::budget::{
    reserve_vec, BudgetCharge, ByteBudget, RenderBudget, RenderLimit, RenderResource,
};
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
impl NavDebugPaint {
    /// Return the actual retained allocation capacity for this paint.
    ///
    /// This census reads vector and string capacities without allocating.
    pub fn retained_bytes(&self) -> usize {
        self.retained_vector_bytes().saturating_add(
            self.labels.iter().fold(0usize, |bytes, (_, _, text)| {
                bytes.saturating_add(text.capacity())
            }),
        )
    }
}

/// Fallible owner for one host-built nav paint candidate.
///
/// The public paint schema remains directly constructible and cloneable.
/// This builder keeps its non-Clone budget charge beside the candidate until
/// the client atomically replaces the retained paint.
pub struct NavDebugPaintBuilder {
    paint: NavDebugPaint,
    budget: Option<ByteBudget>,
    charge: Option<BudgetCharge>,
    label_bytes_remaining: usize,
}

impl NavDebugPaintBuilder {
    /// Reserve exact input capacities before the host copies any facts.
    ///
    /// `label_bytes` is the sum of the UTF-8 byte lengths of all labels that
    /// the caller can append. The active fixture's `NavInput` budget admits
    /// both the vector capacities and those nested strings.
    pub fn with_capacity(
        collision: usize,
        path: usize,
        trail: usize,
        hulls: usize,
        labels: usize,
        label_bytes: usize,
    ) -> Result<Self, RenderLimit> {
        let budget = RenderBudget::current_resource(RenderResource::NavInput);
        let requested = [
            capacity_bytes::<NavDebugCell>(collision, &budget)?,
            capacity_bytes::<(i32, i32, bool)>(path, &budget)?,
            capacity_bytes::<(i32, i32, bool)>(trail, &budget)?,
            capacity_bytes::<NavDebugHull>(hulls, &budget)?,
            capacity_bytes::<(i32, i32, String)>(labels, &budget)?,
        ]
        .into_iter()
        .try_fold(label_bytes, usize::checked_add)
        .ok_or_else(|| nav_input_limit(usize::MAX, budget.as_ref()))?;
        let mut charge = budget
            .as_ref()
            .map(|budget| budget.reserve(requested))
            .transpose()?;

        let paint = NavDebugPaint {
            collision: reserve_input_vec(collision, &budget)?,
            path: reserve_input_vec(path, &budget)?,
            trail: reserve_input_vec(trail, &budget)?,
            hulls: reserve_input_vec(hulls, &budget)?,
            labels: reserve_input_vec(labels, &budget)?,
            ..NavDebugPaint::default()
        };
        let vectors = paint.retained_vector_bytes();
        let reserved = vectors
            .checked_add(label_bytes)
            .ok_or_else(|| nav_input_limit(usize::MAX, budget.as_ref()))?;
        if let Some(charge) = charge.as_mut() {
            charge.resize(reserved)?;
        }

        Ok(Self {
            paint,
            budget,
            charge,
            label_bytes_remaining: label_bytes,
        })
    }

    /// Set the layer colors without allocating.
    pub fn set_colors(&mut self, colors: NavDebugColors) {
        self.paint.colors = colors;
    }

    /// Set the visible layers without allocating.
    pub fn set_visibility(
        &mut self,
        collision: bool,
        nsew: bool,
        path: bool,
        trail: bool,
        hulls: bool,
    ) {
        self.paint.show_collision = collision;
        self.paint.show_nsew = nsew;
        self.paint.show_path = path;
        self.paint.show_trail = trail;
        self.paint.show_hulls = hulls;
    }

    /// Set the current walk aim without allocating.
    pub fn set_click(&mut self, click: Option<(i32, i32)>) {
        self.paint.click = click;
    }
    /// Return whether this candidate already contains the same loc hull.
    pub fn contains_hull(&self, hull: NavDebugHull) -> bool {
        self.paint.hulls.iter().any(|current| {
            current.loc_id == hull.loc_id
                && current.scene_x == hull.scene_x
                && current.scene_z == hull.scene_z
        })
    }

    /// Append one collision fact into its admitted capacity.
    pub fn push_collision(&mut self, cell: NavDebugCell) -> Result<(), RenderLimit> {
        ensure_input_room(&self.paint.collision, 1, self.budget.as_ref())?;
        self.paint.collision.push(cell);
        Ok(())
    }

    /// Append one path fact into its admitted capacity.
    pub fn push_path(&mut self, tile: (i32, i32, bool)) -> Result<(), RenderLimit> {
        ensure_input_room(&self.paint.path, 1, self.budget.as_ref())?;
        self.paint.path.push(tile);
        Ok(())
    }

    /// Append one trail fact into its admitted capacity.
    pub fn push_trail(&mut self, tile: (i32, i32, bool)) -> Result<(), RenderLimit> {
        ensure_input_room(&self.paint.trail, 1, self.budget.as_ref())?;
        self.paint.trail.push(tile);
        Ok(())
    }

    /// Append one live loc hull into its admitted capacity.
    pub fn push_hull(&mut self, hull: NavDebugHull) -> Result<(), RenderLimit> {
        ensure_input_room(&self.paint.hulls, 1, self.budget.as_ref())?;
        self.paint.hulls.push(hull);
        Ok(())
    }

    /// Copy one label only after its bytes and vector slot were admitted.
    pub fn push_label(&mut self, lx: i32, lz: i32, text: &str) -> Result<(), RenderLimit> {
        ensure_input_room(&self.paint.labels, 1, self.budget.as_ref())?;
        if text.len() > self.label_bytes_remaining {
            return Err(nav_input_limit(
                self.paint.retained_bytes().saturating_add(text.len()),
                self.budget.as_ref(),
            ));
        }
        let mut owned = String::new();
        owned.try_reserve_exact(text.len()).map_err(|_| {
            nav_input_limit(
                self.paint.retained_bytes().saturating_add(text.len()),
                self.budget.as_ref(),
            )
        })?;
        owned.push_str(text);
        if owned.capacity() > text.len() {
            let bytes = self
                .charge
                .as_ref()
                .map_or(0, BudgetCharge::bytes)
                .saturating_add(owned.capacity() - text.len());
            if let Some(charge) = self.charge.as_mut() {
                charge.resize(bytes)?;
            }
        }
        self.label_bytes_remaining -= text.len();
        self.paint.labels.push((lx, lz, owned));
        Ok(())
    }

    /// Return the candidate and its non-Clone owner charge.
    pub fn finish(mut self) -> Result<(NavDebugPaint, Option<BudgetCharge>), RenderLimit> {
        if let Some(charge) = self.charge.as_mut() {
            charge.resize(self.paint.retained_bytes())?;
        }
        Ok((self.paint, self.charge.take()))
    }
}

impl NavDebugPaint {
    fn retained_vector_bytes(&self) -> usize {
        self.collision
            .capacity()
            .saturating_mul(std::mem::size_of::<NavDebugCell>())
            .saturating_add(self.path.capacity().saturating_mul(std::mem::size_of::<(
                i32,
                i32,
                bool,
            )>()))
            .saturating_add(self.trail.capacity().saturating_mul(std::mem::size_of::<(
                i32,
                i32,
                bool,
            )>()))
            .saturating_add(
                self.hulls
                    .capacity()
                    .saturating_mul(std::mem::size_of::<NavDebugHull>()),
            )
            .saturating_add(self.labels.capacity().saturating_mul(std::mem::size_of::<(
                i32,
                i32,
                String,
            )>()))
    }
}

fn capacity_bytes<T>(count: usize, budget: &Option<ByteBudget>) -> Result<usize, RenderLimit> {
    count
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(|| nav_input_limit(usize::MAX, budget.as_ref()))
}

fn reserve_input_vec<T>(
    capacity: usize,
    budget: &Option<ByteBudget>,
) -> Result<Vec<T>, RenderLimit> {
    let mut values = Vec::new();
    if capacity != 0 {
        values.try_reserve_exact(capacity).map_err(|_| {
            let requested = capacity_bytes::<T>(capacity, budget).unwrap_or(usize::MAX);
            nav_input_limit(requested, budget.as_ref())
        })?;
    }
    Ok(values)
}

fn ensure_input_room<T>(
    values: &Vec<T>,
    additional: usize,
    budget: Option<&ByteBudget>,
) -> Result<(), RenderLimit> {
    let required_len = values.len().checked_add(additional);
    let required_bytes = required_len
        .and_then(|len| len.checked_mul(std::mem::size_of::<T>()))
        .unwrap_or(usize::MAX);
    if required_len.is_some_and(|len| len <= values.capacity()) {
        Ok(())
    } else {
        Err(nav_input_limit(required_bytes, budget))
    }
}

fn nav_input_limit(requested: usize, budget: Option<&ByteBudget>) -> RenderLimit {
    RenderLimit::new(
        RenderResource::NavInput,
        requested,
        budget.map_or(0, ByteBudget::maximum),
    )
}

#[cfg(test)]
mod nav_input_builder_tests {
    use super::*;

    fn fixture(nav_input: usize) -> RenderBudget {
        RenderBudget::fixture(crate::render::budget::RenderBudgetLimits::new(
            0, 0, 0, 0, nav_input, 0, 0, 0, 0, 0, 0, 0, 0,
        ))
    }

    #[test]
    fn nav_input_builder_refuses_each_entry_capacity_before_allocation() {
        let requests = [
            std::mem::size_of::<NavDebugCell>(),
            std::mem::size_of::<(i32, i32, bool)>(),
            std::mem::size_of::<(i32, i32, bool)>(),
            std::mem::size_of::<NavDebugHull>(),
            std::mem::size_of::<(i32, i32, String)>(),
        ];
        for (index, bytes) in requests.into_iter().enumerate() {
            let budget = fixture(bytes - 1);
            let _scope = budget.scope();
            let capacities = match index {
                0 => (1, 0, 0, 0, 0, 0),
                1 => (0, 1, 0, 0, 0, 0),
                2 => (0, 0, 1, 0, 0, 0),
                3 => (0, 0, 0, 1, 0, 0),
                _ => (0, 0, 0, 0, 1, 0),
            };
            let error = match NavDebugPaintBuilder::with_capacity(
                capacities.0,
                capacities.1,
                capacities.2,
                capacities.3,
                capacities.4,
                capacities.5,
            ) {
                Err(error) => error,
                Ok(_) => panic!("fixture budget must refuse this input capacity"),
            };
            assert_eq!(error.resource(), RenderResource::NavInput);
            assert_eq!(budget.for_resource(RenderResource::NavInput).live(), 0);
        }
    }

    #[test]
    fn nav_input_builder_refuses_label_bytes_before_label_copy() {
        let descriptor = std::mem::size_of::<(i32, i32, String)>();
        let budget = fixture(descriptor + 3);
        let _scope = budget.scope();
        let error = match NavDebugPaintBuilder::with_capacity(0, 0, 0, 0, 1, 4) {
            Err(error) => error,
            Ok(_) => panic!("fixture budget must refuse the label bytes"),
        };
        assert_eq!(error.resource(), RenderResource::NavInput);

        let budget = fixture(descriptor + 4);
        let _scope = budget.scope();
        let mut builder = NavDebugPaintBuilder::with_capacity(0, 0, 0, 0, 1, 4).unwrap();
        builder.push_label(1, 2, "word").unwrap();
        let (paint, charge) = builder.finish().unwrap();
        assert_eq!(paint.labels[0].2, "word");
        assert_eq!(
            charge.as_ref().map(BudgetCharge::bytes),
            Some(paint.retained_bytes())
        );
    }
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
    order: usize,
}

/// 5×5 dot glyphs for the NSEW face letters (bit 4 = leftmost column).
const GLYPH_N: [u8; 5] = [0b10001, 0b11001, 0b10101, 0b10011, 0b10001];
const GLYPH_S: [u8; 5] = [0b01110, 0b10000, 0b01110, 0b00001, 0b01110];
const GLYPH_E: [u8; 5] = [0b11111, 0b10000, 0b11110, 0b10000, 0b11111];
const GLYPH_W: [u8; 5] = [0b10001, 0b10001, 0b10101, 0b10101, 0b01010];

/// Exact nav-input/mesh element sizes and fixed emitter multiplicities for
/// the ignored full-content census. This query does not allocate.
#[cfg(feature = "render-test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTestNavLayout {
    pub collision_cell_bytes: usize,
    pub path_entry_bytes: usize,
    pub trail_entry_bytes: usize,
    pub hull_bytes: usize,
    pub label_entry_bytes: usize,
    pub vertex_bytes: usize,
    pub quad_bytes: usize,
    pub edge_bytes: usize,
    pub max_edges_per_collision_cell: usize,
    pub max_corner_lines_per_collision_cell: usize,
    pub nsew_glyph_pixels_per_collision_cell: usize,
    pub max_vertices_per_line: usize,
    pub max_vertices_per_quad: usize,
    pub max_caption_fill_vertices_per_quad: usize,
    pub max_hull_lines: usize,
    pub click_lines: usize,
}

/// Return exact layouts and source-defined fixed emitter multiplicities.
#[cfg(feature = "render-test-hooks")]
pub fn render_test_nav_layout() -> RenderTestNavLayout {
    let nsew_glyph_pixels_per_collision_cell = [GLYPH_N, GLYPH_S, GLYPH_E, GLYPH_W]
        .into_iter()
        .flat_map(|glyph| glyph.into_iter())
        .map(|row| row.count_ones() as usize)
        .sum();
    let vertices_per_line = 2 * 3;
    RenderTestNavLayout {
        collision_cell_bytes: std::mem::size_of::<NavDebugCell>(),
        path_entry_bytes: std::mem::size_of::<(i32, i32, bool)>(),
        trail_entry_bytes: std::mem::size_of::<(i32, i32, bool)>(),
        hull_bytes: std::mem::size_of::<NavDebugHull>(),
        label_entry_bytes: std::mem::size_of::<(i32, i32, String)>(),
        vertex_bytes: std::mem::size_of::<NavVertex>(),
        quad_bytes: std::mem::size_of::<Quad>(),
        edge_bytes: std::mem::size_of::<(i32, i32, i32, i32)>(),
        max_edges_per_collision_cell: 4,
        max_corner_lines_per_collision_cell: 4,
        nsew_glyph_pixels_per_collision_cell,
        max_vertices_per_line: vertices_per_line,
        max_vertices_per_quad: 2 * 3 + 4 * vertices_per_line,
        max_caption_fill_vertices_per_quad: 6 * 3,
        max_hull_lines: 12,
        click_lines: 4,
    }
}

/// Count nonzero glyph-mask pixels emitted by one caption. The full-content
/// census uses this read-only helper with the pinned media font.
#[cfg(feature = "render-test-hooks")]
pub fn render_test_caption_ink_pixels(font: &crate::graphics::PixFont, text: &str) -> usize {
    text.chars()
        .filter(|character| *character != ' ')
        .map(|character| {
            let code = character as usize;
            let width = font.char_mask_width.get(code).copied().unwrap_or(0);
            let height = font.char_mask_height.get(code).copied().unwrap_or(0);
            if width <= 0 || height <= 0 {
                return 0;
            }
            let width = width as usize;
            let height = height as usize;
            let Some(pixels) = width.checked_mul(height) else {
                return 0;
            };
            font.char_mask.get(code).map_or(0, |mask| {
                mask.iter()
                    .take(pixels)
                    .filter(|&&pixel| pixel != 0)
                    .count()
            })
        })
        .sum()
}

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
    edges: Vec<(i32, i32, i32, i32)>,
    vertices_budget: Option<ByteBudget>,
    vertices_charge: Option<BudgetCharge>,
    quads_budget: Option<ByteBudget>,
    quads_charge: Option<BudgetCharge>,
    edges_budget: Option<ByteBudget>,
    edges_charge: Option<BudgetCharge>,
}

impl NavMesh {
    fn bind_budgets(&mut self) {
        bind_budget(
            &mut self.vertices,
            &mut self.vertices_charge,
            &mut self.vertices_budget,
            RenderResource::NavVertices,
        );
        bind_budget(
            &mut self.quads,
            &mut self.quads_charge,
            &mut self.quads_budget,
            RenderResource::NavQuads,
        );
        bind_budget(
            &mut self.edges,
            &mut self.edges_charge,
            &mut self.edges_budget,
            RenderResource::NavEdges,
        );
    }

    pub(crate) fn vertices_capacity_bytes(&self) -> usize {
        self.vertices.capacity() * std::mem::size_of::<NavVertex>()
    }

    pub(crate) fn quads_capacity_bytes(&self) -> usize {
        self.quads.capacity() * std::mem::size_of::<Quad>()
    }

    pub(crate) fn edges_capacity_bytes(&self) -> usize {
        self.edges.capacity() * std::mem::size_of::<(i32, i32, i32, i32)>()
    }

    /// Drop high-water allocations as soon as this backend stops painting.
    pub(crate) fn release(&mut self) {
        *self = Self::default();
    }

    #[cfg(test)]
    pub(crate) fn retained_capacity(&self) -> usize {
        self.vertices.capacity() + self.quads.capacity() + self.edges.capacity()
    }
}

fn bind_budget<T>(
    values: &mut Vec<T>,
    charge: &mut Option<BudgetCharge>,
    previous: &mut Option<ByteBudget>,
    resource: RenderResource,
) {
    let current = RenderBudget::current_resource(resource);
    let same = match (previous.as_ref(), current.as_ref()) {
        (Some(old), Some(new)) => old.same_budget(new),
        (None, None) => true,
        _ => false,
    };
    if !same {
        *values = Vec::new();
        *charge = None;
    }
    *previous = current;
}

struct NavBuffer<'a, T> {
    values: &'a mut Vec<T>,
    charge: &'a mut Option<BudgetCharge>,
    budget: Option<&'a ByteBudget>,
}

impl<T> NavBuffer<'_, T> {
    fn push(&mut self, value: T) -> Result<(), RenderLimit> {
        reserve_vec(self.values, 1, self.charge, self.budget)?;
        self.values.push(value);
        Ok(())
    }

    fn extend<const N: usize>(&mut self, values: [T; N]) -> Result<(), RenderLimit> {
        reserve_vec(self.values, N, self.charge, self.budget)?;
        self.values.extend(values);
        Ok(())
    }

    fn insert(&mut self, index: usize, value: T) -> Result<(), RenderLimit> {
        reserve_vec(self.values, 1, self.charge, self.budget)?;
        self.values.insert(index, value);
        Ok(())
    }
}

fn push_quad(quads: &mut NavBuffer<'_, Quad>, mut quad: Quad) -> Result<(), RenderLimit> {
    quad.order = quads.values.len();
    quads.push(quad)
}

struct NavPaintScope<'a> {
    client: &'a mut Client,
    paint: Option<NavDebugPaint>,
}

impl<'a> NavPaintScope<'a> {
    fn take(client: &'a mut Client) -> Self {
        let paint = client.nav_debug_paint.take();
        Self { client, paint }
    }

    fn parts(&mut self) -> (&mut Client, Option<&NavDebugPaint>) {
        (self.client, self.paint.as_ref())
    }
}

impl Drop for NavPaintScope<'_> {
    fn drop(&mut self) {
        self.client.nav_debug_paint = self.paint.take();
    }
}

/// Project the retained host paint into GPU triangles for this camera.
/// Active paint reuses allocations; the backend calls [`NavMesh::release`]
/// when the retained paint disappears.
pub(crate) fn build_gpu_mesh(
    client: &mut Client,
    r: &mut Renderer,
    mesh: &mut NavMesh,
) -> Result<(), RenderLimit> {
    mesh.bind_budgets();
    mesh.vertices.clear();
    mesh.quads.clear();
    mesh.edges.clear();
    let mut output = NavBuffer {
        values: &mut mesh.vertices,
        charge: &mut mesh.vertices_charge,
        budget: mesh.vertices_budget.as_ref(),
    };
    let out = &mut output;
    let mut quads = NavBuffer {
        values: &mut mesh.quads,
        charge: &mut mesh.quads_charge,
        budget: mesh.quads_budget.as_ref(),
    };
    let mut edges = NavBuffer {
        values: &mut mesh.edges,
        charge: &mut mesh.edges_charge,
        budget: mesh.edges_budget.as_ref(),
    };
    let mut paint_scope = NavPaintScope::take(client);
    let (client, paint) = paint_scope.parts();
    let Some(paint) = paint else {
        return Ok(());
    };
    let show_any = paint.show_collision
        || paint.show_nsew
        || paint.show_path
        || paint.show_trail
        || paint.show_hulls
        || !paint.labels.is_empty()
        || paint.click.is_some();
    if !show_any {
        return Ok(());
    }

    let result = (|| {
        // The ordinal preserves stable far-to-near order without sort scratch.
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
                    push_quad(&mut quads, quad)?;
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
                    push_quad(&mut quads, quad)?;
                }
            }
        }
        if paint.show_trail {
            for &(lx, lz, run_alt) in &paint.trail {
                let colour = rgb(if run_alt {
                    paint.colors.trail_run
                } else {
                    paint.colors.trail
                });
                if let Some(quad) =
                    tile_quad(client, r, lx, lz, colour, HOP_FILL_ALPHA, HOP_STROKE_ALPHA)
                {
                    push_quad(&mut quads, quad)?;
                }
            }
        }
        quads
            .values
            .sort_unstable_by_key(|quad| (std::cmp::Reverse(quad.depth), quad.order));
        for quad in quads.values.iter() {
            append_filled_quad(out, quad)?;
            append_quad_stroke(out, quad)?;
        }

        if paint.show_collision {
            for cell in &paint.collision {
                for (bit, x, z, ex, ez) in cardinal_edges(cell) {
                    if cell.bits & bit == 0 {
                        continue;
                    }
                    let edge = (x, z, ex, ez);
                    let Err(index) = edges.values.binary_search(&edge) else {
                        continue;
                    };
                    edges.insert(index, edge)?;
                    append_projected_line(
                        client,
                        r,
                        out,
                        (x, z),
                        (ex, ez),
                        rgb(paint.colors.collision),
                        STROKE_ALPHA,
                    )?;
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
                        )?;
                    }
                }
            }
        }
        if paint.show_nsew {
            for cell in &paint.collision {
                append_nsew(client, r, out, cell, rgb(paint.colors.nsew))?;
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
            )?;
        }
        if paint.show_hulls {
            for hull in &paint.hulls {
                append_hull(client, r, out, hull, rgb(paint.colors.hull))?;
            }
        }
        let colour = rgb(paint.colors.nsew);
        for &(lx, lz, ref text) in &paint.labels {
            append_hop_label(client, r, out, quads.values, (lx, lz), text, colour)?;
        }
        Ok(())
    })();
    if result.is_err() {
        out.values.clear();
    }
    result
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
        order: 0,
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
    out: &mut NavBuffer<'_, NavVertex>,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    colour: i32,
    alpha: i32,
) -> Result<(), RenderLimit> {
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
    ])?;
    Ok(())
}

fn append_filled_quad(out: &mut NavBuffer<'_, NavVertex>, quad: &Quad) -> Result<(), RenderLimit> {
    let p = [
        (quad.x[0] as f32, quad.y[0] as f32),
        (quad.x[1] as f32, quad.y[1] as f32),
        (quad.x[2] as f32, quad.y[2] as f32),
        (quad.x[3] as f32, quad.y[3] as f32),
    ];
    append_triangle(out, p[0], p[1], p[2], quad.colour, quad.fill_alpha)?;
    append_triangle(out, p[0], p[2], p[3], quad.colour, quad.fill_alpha)?;
    Ok(())
}

/// Re-emit each non-black fill under a caption at the caption opacity. The
/// Pix2D path raises the coverage of existing non-black caption-box pixels;
/// clipping the retained fill polygon preserves its RGB while doing the same.
fn append_caption_fill(
    out: &mut NavBuffer<'_, NavVertex>,
    quad: &Quad,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> Result<(), RenderLimit> {
    if quad.colour & 0x00ff_ffff == 0 {
        return Ok(());
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
        )?;
    }
    Ok(())
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

fn append_quad_stroke(out: &mut NavBuffer<'_, NavVertex>, quad: &Quad) -> Result<(), RenderLimit> {
    for i in 0..4 {
        append_line(
            out,
            (quad.x[i], quad.y[i]),
            (quad.x[(i + 1) % 4], quad.y[(i + 1) % 4]),
            quad.colour,
            quad.stroke_alpha,
        )?;
    }
    Ok(())
}

/// A one-pixel screen-space line expressed as two GPU triangles with square
/// endpoint extension. Integer coordinates name Pix2D pixel centres.
fn append_line(
    out: &mut NavBuffer<'_, NavVertex>,
    a: (i32, i32),
    b: (i32, i32),
    colour: i32,
    alpha: i32,
) -> Result<(), RenderLimit> {
    let dx = (b.0 - a.0) as f32;
    let dy = (b.1 - a.1) as f32;
    let len = dx.hypot(dy);
    if len == 0.0 {
        append_pixel(out, a.0, a.1, colour, alpha)?;
        return Ok(());
    }
    let tx = dx * 0.5 / len;
    let ty = dy * 0.5 / len;
    let nx = -ty;
    let ny = tx;
    let p0 = (a.0 as f32 - tx + nx, a.1 as f32 - ty + ny);
    let p1 = (b.0 as f32 + tx + nx, b.1 as f32 + ty + ny);
    let p2 = (b.0 as f32 + tx - nx, b.1 as f32 + ty - ny);
    let p3 = (a.0 as f32 - tx - nx, a.1 as f32 - ty - ny);
    append_triangle(out, p0, p1, p2, colour, alpha)?;
    append_triangle(out, p0, p2, p3, colour, alpha)?;
    Ok(())
}

fn append_pixel(
    out: &mut NavBuffer<'_, NavVertex>,
    x: i32,
    y: i32,
    colour: i32,
    alpha: i32,
) -> Result<(), RenderLimit> {
    let p0 = (x as f32 - 0.5, y as f32 - 0.5);
    let p1 = (x as f32 + 0.5, y as f32 - 0.5);
    let p2 = (x as f32 + 0.5, y as f32 + 0.5);
    let p3 = (x as f32 - 0.5, y as f32 + 0.5);
    append_triangle(out, p0, p1, p2, colour, alpha)?;
    append_triangle(out, p0, p2, p3, colour, alpha)?;
    Ok(())
}

fn append_projected_line(
    client: &Client,
    r: &Renderer,
    out: &mut NavBuffer<'_, NavVertex>,
    a: (i32, i32),
    b: (i32, i32),
    colour: i32,
    alpha: i32,
) -> Result<(), RenderLimit> {
    let a = r.project_overlay(client, a.0, a.1, 0);
    let b = r.project_overlay(client, b.0, b.1, 0);
    if a.0 != -1 && b.0 != -1 {
        append_line(out, a, b, colour, alpha)?;
    }
    Ok(())
}

fn append_nsew(
    client: &Client,
    r: &Renderer,
    out: &mut NavBuffer<'_, NavVertex>,
    cell: &NavDebugCell,
    colour: i32,
) -> Result<(), RenderLimit> {
    let glyphs = [GLYPH_N, GLYPH_S, GLYPH_E, GLYPH_W];
    for ((bit, fx, fz), glyph) in nsew_centres(cell).into_iter().zip(glyphs) {
        if cell.bits & bit == 0 {
            continue;
        }
        let (px, py) = r.project_overlay(client, fx, fz, 0);
        if px != -1 {
            append_glyph(out, px - 2, py - 2, glyph, colour, STROKE_ALPHA)?;
        }
    }
    Ok(())
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
    out: &mut NavBuffer<'_, NavVertex>,
    fills: &[Quad],
    tile: (i32, i32),
    text: &str,
    colour: i32,
) -> Result<(), RenderLimit> {
    let x = tile.0.wrapping_mul(TILE) + TILE / 2;
    let z = tile.1.wrapping_mul(TILE) + TILE / 2;
    let (px, py) = r.project_overlay(client, x, z, 0);
    if px == -1 {
        return Ok(());
    }
    let Some(font) = r.media.b12.as_ref() else {
        return Ok(());
    };
    let width = font.string_wid(Some(text));
    let height = font.height.max(12);
    let left = (px - width / 2 - 1) as f32 - 0.5;
    let top = (py - height - 1) as f32 - 0.5;
    let right = (px + width / 2 + 1) as f32 + 0.5;
    let bottom = (py + 1) as f32 + 0.5;
    for fill in fills {
        append_caption_fill(out, fill, left, top, right, bottom)?;
    }
    let start_x = px - width / 2;
    append_font_text(out, font, text, start_x + 1, py + 1, 0, 255)?;
    append_font_text(out, font, text, start_x, py, colour, STROKE_ALPHA)?;
    Ok(())
}

fn append_font_text(
    out: &mut NavBuffer<'_, NavVertex>,
    font: &crate::graphics::PixFont,
    text: &str,
    mut x: i32,
    baseline_y: i32,
    colour: i32,
    alpha: i32,
) -> Result<(), RenderLimit> {
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
                        append_pixel(out, x + ox + col, y + oy + row, colour, alpha)?;
                    }
                }
            }
        }
        x += font.char_advance.get(code).copied().unwrap_or(0);
    }
    Ok(())
}

fn append_glyph(
    out: &mut NavBuffer<'_, NavVertex>,
    x: i32,
    y: i32,
    glyph: [u8; 5],
    colour: i32,
    alpha: i32,
) -> Result<(), RenderLimit> {
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) != 0 {
                append_pixel(out, x + col as i32, y + row as i32, colour, alpha)?;
            }
        }
    }
    Ok(())
}

fn append_tile_stroke(
    client: &Client,
    r: &Renderer,
    out: &mut NavBuffer<'_, NavVertex>,
    lx: i32,
    lz: i32,
    colour: i32,
    alpha: i32,
) -> Result<(), RenderLimit> {
    let x0 = lx.wrapping_mul(TILE);
    let z0 = lz.wrapping_mul(TILE);
    let p = [
        r.project_overlay(client, x0, z0, 0),
        r.project_overlay(client, x0 + TILE, z0, 0),
        r.project_overlay(client, x0 + TILE, z0 + TILE, 0),
        r.project_overlay(client, x0, z0 + TILE, 0),
    ];
    if p.iter().any(|&(px, _)| px == -1) {
        return Ok(());
    }
    for i in 0..4 {
        append_line(out, p[i], p[(i + 1) % 4], colour, alpha)?;
    }
    Ok(())
}

/// Stroke a loc hull's eight-corner AABB from the live model at the hull's
/// scene tile. Loc picking remains untouched.
fn append_hull(
    client: &mut Client,
    r: &mut Renderer,
    out: &mut NavBuffer<'_, NavVertex>,
    hull: &NavDebugHull,
    colour: i32,
) -> Result<(), RenderLimit> {
    let Some((pos_x, pos_y, pos_z, yaw, model)) = r.world.loc_model_at(
        &client.world,
        &client.cache,
        client.loop_cycle,
        client.minusedlevel,
        hull.scene_x,
        hull.scene_z,
        hull.loc_id,
    )?
    else {
        return Ok(());
    };
    let (Some(point_x), Some(point_y), Some(point_z)) =
        (&model.point_x, &model.point_y, &model.point_z)
    else {
        return Ok(());
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
        return Ok(());
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
        return Ok(());
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
        append_line(out, screen[a], screen[b], colour, HOP_STROKE_ALPHA)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(colour: i32, depth: i32) -> Quad {
        Quad {
            depth,
            x: [0, 1, 1, 0],
            y: [0, 0, 1, 1],
            colour,
            fill_alpha: WALK_FILL_ALPHA,
            stroke_alpha: STROKE_ALPHA,
            order: 0,
        }
    }

    #[test]
    fn nav_budget_bounds_quads_edges_vertices_and_labels() {
        let vertex_bytes = std::mem::size_of::<NavVertex>();
        let vertices_budget = ByteBudget::new(RenderResource::NavVertices, 3 * vertex_bytes);
        let mut vertices = Vec::new();
        let mut vertices_charge = None;
        let mut out = NavBuffer {
            values: &mut vertices,
            charge: &mut vertices_charge,
            budget: Some(&vertices_budget),
        };
        append_triangle(&mut out, (0.0, 0.0), (1.0, 0.0), (0.0, 1.0), 1, 255).unwrap();
        let pointer = out.values.as_ptr();
        let error =
            append_triangle(&mut out, (0.0, 0.0), (1.0, 0.0), (0.0, 1.0), 1, 255).unwrap_err();
        assert_eq!(error.resource(), RenderResource::NavVertices);
        assert_eq!(out.values.len(), 3);
        assert_eq!(out.values.as_ptr(), pointer);
        assert_eq!(vertices_budget.live(), 3 * vertex_bytes);
        assert_eq!(vertices_budget.peak(), 3 * vertex_bytes);

        let quad_bytes = std::mem::size_of::<Quad>();
        let quads_budget = ByteBudget::new(RenderResource::NavQuads, quad_bytes);
        let mut quads = Vec::new();
        let mut quads_charge = None;
        let mut quads = NavBuffer {
            values: &mut quads,
            charge: &mut quads_charge,
            budget: Some(&quads_budget),
        };
        push_quad(&mut quads, quad(1, 1)).unwrap();
        assert_eq!(
            push_quad(&mut quads, quad(2, 2)).unwrap_err().resource(),
            RenderResource::NavQuads
        );
        assert_eq!(quads.values.len(), 1);
        assert_eq!(quads_budget.live(), quad_bytes);
        assert_eq!(quads_budget.peak(), quad_bytes);

        let edge_bytes = std::mem::size_of::<(i32, i32, i32, i32)>();
        let edges_budget = ByteBudget::new(RenderResource::NavEdges, edge_bytes);
        let mut edges = Vec::new();
        let mut edges_charge = None;
        let mut edges = NavBuffer {
            values: &mut edges,
            charge: &mut edges_charge,
            budget: Some(&edges_budget),
        };
        edges.insert(0, (0, 0, 1, 0)).unwrap();
        assert_eq!(
            edges.insert(1, (1, 0, 1, 1)).unwrap_err().resource(),
            RenderResource::NavEdges
        );
        assert_eq!(edges.values.len(), 1);
        assert_eq!(edges_budget.live(), edge_bytes);
        assert_eq!(edges_budget.peak(), edge_bytes);

        let labels_budget = ByteBudget::new(RenderResource::NavVertices, 9 * vertex_bytes);
        let mut font = crate::graphics::PixFont::default();
        font.char_mask[b'A' as usize] = vec![1];
        font.char_mask_width[b'A' as usize] = 1;
        font.char_mask_height[b'A' as usize] = 1;
        font.char_advance[b'A' as usize] = 1;
        let mut labels = Vec::new();
        let mut labels_charge = None;
        let mut labels = NavBuffer {
            values: &mut labels,
            charge: &mut labels_charge,
            budget: Some(&labels_budget),
        };
        append_font_text(&mut labels, &font, "A", 0, 0, 1, 255).unwrap();
        assert_eq!(labels.values.len(), 6);
        assert_eq!(
            append_font_text(&mut labels, &font, "A", 1, 0, 1, 255)
                .unwrap_err()
                .resource(),
            RenderResource::NavVertices
        );
        assert_eq!(labels.values.len(), 6);
        assert_eq!(labels_budget.live(), 6 * vertex_bytes);
        assert_eq!(labels_budget.peak(), 9 * vertex_bytes);
    }

    #[test]
    fn equal_depth_quads_keep_original_painter_order_without_sort_scratch() {
        let mut values = Vec::new();
        let mut charge = None;
        let mut quads = NavBuffer {
            values: &mut values,
            charge: &mut charge,
            budget: None,
        };
        push_quad(&mut quads, quad(1, 10)).unwrap();
        push_quad(&mut quads, quad(2, 20)).unwrap();
        push_quad(&mut quads, quad(3, 10)).unwrap();
        quads
            .values
            .sort_unstable_by_key(|quad| (std::cmp::Reverse(quad.depth), quad.order));
        assert_eq!(
            quads
                .values
                .iter()
                .map(|quad| quad.colour)
                .collect::<Vec<_>>(),
            [2, 1, 3]
        );
    }

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
