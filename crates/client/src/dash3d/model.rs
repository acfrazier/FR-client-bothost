// Port of `~/experiments/Server/webclient/src/dash3d/Model.ts` — the model
// decode, transforms, lighting and animation machinery needed to build scene
// models (`REBUILD_NORMAL` loc placement, entities), plus the render half
// (`objRender`, `worldRender`, `render2`/`render3`, mouse picking). The
// render statics that TS keeps process-wide (`vertexScreenX/Y/Z`, the depth
// and priority buckets, `mouseCheck`/`pickedCount`) live on the per-client
// `Pix3DDraw.model_scratch` and pick fields; the TS `Pix2D` statics become
// the bound `Pix2D` surface.
//
// TS statics (`meta`, `provider`, `tempModel`, the scratch buffers, the
// oX/oY/oZ anim origin) live on a process-wide store. The TS `tempModel`
// singleton and shared-array `copyForAnim`/`set` aliasing are replaced with
// owned copies: single-threaded behaviour is identical and the port stays
// `Send`.
use std::sync::{Arc, Mutex, OnceLock};

use crate::render::budget::{BudgetCharge, ByteBudget, RenderBudget, RenderLimit, RenderResource};

use crate::dash3d::store::ModelStore as GeometryStore;
use crate::dash3d::{AnimFrame, PointNormal};
use crate::datastruct::linkable::{LinkableTrait, Links};
use crate::graphics::{Pix2D, Pix3D, Pix3DDraw};
use crate::io::Packet;

/// OnDemand hook the loader calls when a model id has no unpacked metadata.
pub trait ModelProvider: Send {
    fn request_model(&mut self, id: i32);
}

pub struct ModelMeta {
    src: Option<Vec<u8>>,
    num_points: i32,
    num_faces: i32,
    num_t: i32,
    vertex_order_offset: i32,
    vertex_x_offset: i32,
    vertex_y_offset: i32,
    vertex_z_offset: i32,
    vertex_label_offset: i32,
    face_index_offset: i32,
    face_index_order_offset: i32,
    face_colour_offset: i32,
    face_render_type_offset: i32,
    face_priority_offset: i32,
    face_alpha_offset: i32,
    face_label_offset: i32,
    face_texture_axis_offset: i32,
}
struct ModelCursor<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> ModelCursor<'a> {
    fn at(data: &'a [u8], position: usize) -> Self {
        Self { data, position }
    }

    fn g1(&mut self) -> i32 {
        assert!(self.position < self.data.len());
        let value = self.data[self.position] as i32;
        self.position += 1;
        value
    }

    fn g2(&mut self) -> i32 {
        assert!(self
            .position
            .checked_add(2)
            .is_some_and(|end| end <= self.data.len()));
        let value = u16::from_be_bytes([self.data[self.position], self.data[self.position + 1]]);
        self.position += 2;
        i32::from(value)
    }

    fn gsmarts(&mut self) -> i32 {
        if self.data[self.position] < 0x80 {
            self.g1() - 0x40
        } else {
            self.g2() - 0xc000
        }
    }
}

pub struct ModelStore {
    pub meta: Vec<Option<ModelMeta>>,
    pub provider: Option<Box<dyn ModelProvider + Send>>,
    pub loaded: i32,
}

// Process-wide by design: the unpacked model metadata + provider are the TS
// `Model.meta`/`provider` statics, which all clients share. The metadata is
// immutable once `unpack`ed (the design rule "mutable statics live on
// Client" is about per-client draw/type state), so a `Mutex` around the
// store only serialises concurrent decode requests. The `provider` is the
// OnDemand hook (a shared client resource), set once at startup.
static STORE: OnceLock<Mutex<ModelStore>> = OnceLock::new();

fn store() -> &'static Mutex<ModelStore> {
    STORE.get_or_init(|| {
        Mutex::new(ModelStore {
            meta: Vec::new(),
            provider: None,
            loaded: 0,
        })
    })
}

pub struct Model {
    pub links: Links,

    pub num_points: i32,
    pub point_x: Option<Vec<i32>>,
    pub point_y: Option<Vec<i32>>,
    pub point_z: Option<Vec<i32>>,

    pub num_faces: i32,
    pub face_vertex_a: Option<Vec<i32>>,
    pub face_vertex_b: Option<Vec<i32>>,
    pub face_vertex_c: Option<Vec<i32>>,
    pub face_render_type: Option<Vec<i32>>,
    pub face_priority: Option<Vec<i32>>,
    pub face_alpha: Option<Vec<i32>>,
    pub face_colour: Option<Vec<i32>>,
    pub priority: i32,

    pub num_t: i32,
    pub face_texture_p: Option<Vec<i32>>,
    pub face_texture_m: Option<Vec<i32>>,
    pub face_texture_n: Option<Vec<i32>>,

    pub vertex_label: Option<Vec<i32>>,
    pub face_label: Option<Vec<i32>>,
    pub label_vertices: Option<Vec<Option<Vec<i32>>>>,
    pub label_faces: Option<Vec<Option<Vec<i32>>>>,

    pub point_normal: Option<Vec<Option<PointNormal>>>,
    pub shared_point_normal: Option<Vec<Option<PointNormal>>>,

    pub max_y: i32,
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
    pub min_y: i32,

    pub obj_raise: i32,

    pub face_colour_a: Option<Vec<i32>>,
    pub face_colour_b: Option<Vec<i32>>,
    pub face_colour_c: Option<Vec<i32>>,

    pub use_aabb_mouse_check: bool,
    pub radius: i32,
    pub max_depth: i32,
    pub min_depth: i32,
    render_charge: OnceLock<Mutex<BudgetCharge>>,
}

impl Default for Model {
    fn default() -> Self {
        Model {
            links: Links::new(0),
            num_points: 0,
            point_x: None,
            point_y: None,
            point_z: None,
            num_faces: 0,
            face_vertex_a: None,
            face_vertex_b: None,
            face_vertex_c: None,
            face_render_type: None,
            face_priority: None,
            face_alpha: None,
            face_colour: None,
            priority: 0,
            num_t: 0,
            face_texture_p: None,
            face_texture_m: None,
            face_texture_n: None,
            vertex_label: None,
            face_label: None,
            label_vertices: None,
            label_faces: None,
            point_normal: None,
            shared_point_normal: None,
            max_y: 0,
            min_x: 0,
            max_x: 0,
            min_z: 0,
            max_z: 0,
            min_y: 0,
            obj_raise: 0,
            face_colour_a: None,
            face_colour_b: None,
            face_colour_c: None,
            use_aabb_mouse_check: false,
            radius: 0,
            max_depth: 0,
            min_depth: 0,
            render_charge: OnceLock::new(),
        }
    }
}

impl Clone for Model {
    fn clone(&self) -> Self {
        Self {
            links: self.links.clone(),
            num_points: self.num_points,
            point_x: self.point_x.clone(),
            point_y: self.point_y.clone(),
            point_z: self.point_z.clone(),
            num_faces: self.num_faces,
            face_vertex_a: self.face_vertex_a.clone(),
            face_vertex_b: self.face_vertex_b.clone(),
            face_vertex_c: self.face_vertex_c.clone(),
            face_render_type: self.face_render_type.clone(),
            face_priority: self.face_priority.clone(),
            face_alpha: self.face_alpha.clone(),
            face_colour: self.face_colour.clone(),
            priority: self.priority,
            num_t: self.num_t,
            face_texture_p: self.face_texture_p.clone(),
            face_texture_m: self.face_texture_m.clone(),
            face_texture_n: self.face_texture_n.clone(),
            vertex_label: self.vertex_label.clone(),
            face_label: self.face_label.clone(),
            label_vertices: self.label_vertices.clone(),
            label_faces: self.label_faces.clone(),
            point_normal: self.point_normal.clone(),
            shared_point_normal: self.shared_point_normal.clone(),
            max_y: self.max_y,
            min_x: self.min_x,
            max_x: self.max_x,
            min_z: self.min_z,
            max_z: self.max_z,
            min_y: self.min_y,
            obj_raise: self.obj_raise,
            face_colour_a: self.face_colour_a.clone(),
            face_colour_b: self.face_colour_b.clone(),
            face_colour_c: self.face_colour_c.clone(),
            use_aabb_mouse_check: self.use_aabb_mouse_check,
            radius: self.radius,
            max_depth: self.max_depth,
            min_depth: self.min_depth,
            render_charge: OnceLock::new(),
        }
    }
}

impl LinkableTrait for Model {
    fn links(&self) -> &Links {
        &self.links
    }
    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }
    fn sentinel() -> Self {
        Model::default()
    }
}

impl Model {
    fn byte_total_add(total: &mut usize, count: usize, element_size: usize) -> Option<()> {
        *total = total.checked_add(count.checked_mul(element_size)?)?;
        Some(())
    }

    fn add_optional_vec_bytes<T>(
        total: &mut usize,
        values: Option<&Vec<T>>,
        use_capacity: bool,
    ) -> Option<()> {
        if let Some(values) = values {
            let count = if use_capacity {
                values.capacity()
            } else {
                values.len()
            };
            Self::byte_total_add(total, count, std::mem::size_of::<T>())?;
        }
        Some(())
    }

    fn add_nested_vec_bytes(
        total: &mut usize,
        values: Option<&Vec<Option<Vec<i32>>>>,
        use_capacity: bool,
    ) -> Option<()> {
        let Some(values) = values else {
            return Some(());
        };
        let outer_count = if use_capacity {
            values.capacity()
        } else {
            values.len()
        };
        Self::byte_total_add(total, outer_count, std::mem::size_of::<Option<Vec<i32>>>())?;
        for inner in values.iter().flatten() {
            let inner_count = if use_capacity {
                inner.capacity()
            } else {
                inner.len()
            };
            Self::byte_total_add(total, inner_count, std::mem::size_of::<i32>())?;
        }
        Some(())
    }

    fn checked_retained_bytes(&self, use_capacity: bool) -> Option<usize> {
        let mut total = std::mem::size_of::<Self>();
        Self::add_optional_vec_bytes(&mut total, self.point_x.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.point_y.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.point_z.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_vertex_a.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_vertex_b.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_vertex_c.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_render_type.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_priority.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_alpha.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_colour.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_texture_p.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_texture_m.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_texture_n.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.vertex_label.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_label.as_ref(), use_capacity)?;
        Self::add_nested_vec_bytes(&mut total, self.label_vertices.as_ref(), use_capacity)?;
        Self::add_nested_vec_bytes(&mut total, self.label_faces.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.point_normal.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.shared_point_normal.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_colour_a.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_colour_b.as_ref(), use_capacity)?;
        Self::add_optional_vec_bytes(&mut total, self.face_colour_c.as_ref(), use_capacity)?;
        Some(total)
    }

    /// Logical retained storage, including the Model value and every nested
    /// vector capacity. Overflow saturates so a subsequent budget request
    /// fails closed.
    pub fn retained_bytes(&self) -> usize {
        self.checked_retained_bytes(true).unwrap_or(usize::MAX)
    }

    fn shared_allocation_bytes(model_bytes: usize) -> Option<usize> {
        model_bytes.checked_add(2usize.checked_mul(std::mem::size_of::<usize>())?)
    }

    pub(crate) fn shared_bytes_for_model(model: &Self) -> Option<usize> {
        Self::shared_allocation_bytes(model.checked_retained_bytes(true)?)
    }

    fn checked_decoded_bytes(meta: &ModelMeta) -> Option<usize> {
        let points = usize::try_from(meta.num_points).ok()?;
        let faces = usize::try_from(meta.num_faces).ok()?;
        let textures = usize::try_from(meta.num_t).ok()?;
        let mut total = std::mem::size_of::<Self>();
        Self::byte_total_add(&mut total, points, 3 * std::mem::size_of::<i32>())?;
        Self::byte_total_add(&mut total, faces, 4 * std::mem::size_of::<i32>())?;
        Self::byte_total_add(&mut total, textures, 3 * std::mem::size_of::<i32>())?;
        if meta.vertex_label_offset >= 0 {
            Self::byte_total_add(&mut total, points, std::mem::size_of::<i32>())?;
        }
        if meta.face_render_type_offset >= 0 {
            Self::byte_total_add(&mut total, faces, std::mem::size_of::<i32>())?;
        }
        if meta.face_priority_offset >= 0 {
            Self::byte_total_add(&mut total, faces, std::mem::size_of::<i32>())?;
        }
        if meta.face_alpha_offset >= 0 {
            Self::byte_total_add(&mut total, faces, std::mem::size_of::<i32>())?;
        }
        if meta.face_label_offset >= 0 {
            Self::byte_total_add(&mut total, faces, std::mem::size_of::<i32>())?;
        }
        Self::shared_allocation_bytes(total)
    }

    fn render_error_for_overflow(budget: &ByteBudget) -> RenderLimit {
        RenderLimit::new(budget.resource(), usize::MAX, budget.maximum())
    }

    fn set_render_charge(&self, charge: BudgetCharge) -> Result<(), BudgetCharge> {
        self.render_charge
            .set(Mutex::new(charge))
            .map_err(|charge| {
                charge
                    .into_inner()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
            })
    }

    pub(crate) fn has_render_charge_for(&self, budget: &ByteBudget) -> bool {
        self.render_charge.get().is_some_and(|charge| {
            charge
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .belongs_to(budget)
        })
    }

    fn has_render_charge(&self) -> bool {
        self.render_charge.get().is_some()
    }

    fn resize_render_charge(&self, budget: &ByteBudget, bytes: usize) -> Result<(), RenderLimit> {
        if let Some(charge) = self.render_charge.get() {
            let mut charge = charge
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !charge.belongs_to(budget) {
                return Err(Self::render_error_for_overflow(budget));
            }
            charge.resize(bytes)
        } else {
            self.set_render_charge(budget.reserve(bytes)?)
                .unwrap_or_else(|_| panic!("fresh model already had a render charge"));
            Ok(())
        }
    }

    /// Clone this complete model only after its nested payload has been
    /// admitted. The clone owns an independent charge.
    pub fn try_clone_for_render(&self, budget: &ByteBudget) -> Result<Model, RenderLimit> {
        let bytes = self
            .checked_retained_bytes(false)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let charge = budget.reserve(bytes)?;
        let cloned = self.clone();
        cloned
            .set_render_charge(charge)
            .unwrap_or_else(|_| panic!("fresh render clone already had a charge"));
        Ok(cloned)
    }
    fn add_default_vec_bytes<T>(
        bytes: &mut usize,
        values: Option<&Vec<T>>,
        default_len: usize,
    ) -> Option<()> {
        let len = values.map_or(default_len, Vec::len);
        Self::byte_total_add(bytes, len, std::mem::size_of::<T>())
    }

    fn checked_copy_for_anim_bytes(src: &Self) -> Option<usize> {
        let mut bytes = std::mem::size_of::<Self>();
        for values in [&src.point_x, &src.point_y, &src.point_z, &src.face_colour] {
            Self::add_default_vec_bytes(&mut bytes, values.as_ref(), 0)?;
        }
        Self::add_default_vec_bytes(
            &mut bytes,
            src.face_alpha.as_ref(),
            usize::try_from(src.num_faces).ok()?,
        )?;
        for values in [
            &src.vertex_label,
            &src.face_label,
            &src.face_render_type,
            &src.face_vertex_a,
            &src.face_vertex_b,
            &src.face_vertex_c,
            &src.face_priority,
            &src.face_texture_p,
            &src.face_texture_m,
            &src.face_texture_n,
        ] {
            Self::add_optional_vec_bytes(&mut bytes, values.as_ref(), false)?;
        }
        Some(bytes)
    }

    pub fn try_copy_for_anim(
        src: &Model,
        share_colours: bool,
        share_alpha: bool,
        share_vertices: bool,
        budget: &ByteBudget,
    ) -> Result<Model, RenderLimit> {
        let bytes = Self::checked_copy_for_anim_bytes(src)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let charge = budget.reserve(bytes)?;
        let copied = Self::copy_for_anim(src, share_colours, share_alpha, share_vertices);
        copied
            .set_render_charge(charge)
            .unwrap_or_else(|_| panic!("fresh animation copy already had a charge"));
        Ok(copied)
    }

    fn checked_hill_skew_copy_bytes(
        src: &Self,
        _copy_vertex_y: bool,
        copy_faces: bool,
    ) -> Option<usize> {
        let mut bytes = std::mem::size_of::<Self>();
        Self::add_optional_vec_bytes(&mut bytes, src.point_y.as_ref(), false)?;
        for values in [&src.point_x, &src.point_z] {
            Self::add_optional_vec_bytes(&mut bytes, values.as_ref(), false)?;
        }
        if copy_faces {
            for values in [&src.face_colour_a, &src.face_colour_b, &src.face_colour_c] {
                Self::add_default_vec_bytes(&mut bytes, values.as_ref(), 0)?;
            }
            Self::add_default_vec_bytes(
                &mut bytes,
                src.face_render_type.as_ref(),
                usize::try_from(src.num_faces).ok()?,
            )?;
            Self::add_default_vec_bytes(
                &mut bytes,
                src.point_normal.as_ref(),
                usize::try_from(src.num_points).ok()?,
            )?;
            Self::add_optional_vec_bytes(&mut bytes, src.shared_point_normal.as_ref(), false)?;
        } else {
            for values in [
                &src.face_colour_a,
                &src.face_colour_b,
                &src.face_colour_c,
                &src.face_render_type,
            ] {
                Self::add_optional_vec_bytes(&mut bytes, values.as_ref(), false)?;
            }
        }
        for values in [
            &src.face_colour,
            &src.face_alpha,
            &src.face_priority,
            &src.face_vertex_a,
            &src.face_vertex_b,
            &src.face_vertex_c,
            &src.face_texture_p,
            &src.face_texture_m,
            &src.face_texture_n,
        ] {
            Self::add_optional_vec_bytes(&mut bytes, values.as_ref(), false)?;
        }
        Some(bytes)
    }

    pub fn try_hill_skew_copy(
        src: &Model,
        copy_vertex_y: bool,
        copy_faces: bool,
        budget: &ByteBudget,
    ) -> Result<Model, RenderLimit> {
        let bytes = Self::checked_hill_skew_copy_bytes(src, copy_vertex_y, copy_faces)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let charge = budget.reserve(bytes)?;
        let copied = Self::hill_skew_copy(src, copy_vertex_y, copy_faces);
        copied
            .set_render_charge(charge)
            .unwrap_or_else(|_| panic!("fresh hill-skew copy already had a charge"));
        Ok(copied)
    }

    fn checked_set_bytes(src: &Self) -> Option<usize> {
        let mut bytes = std::mem::size_of::<Self>();
        for values in [
            &src.point_x,
            &src.point_y,
            &src.point_z,
            &src.face_render_type,
            &src.face_colour,
            &src.face_priority,
            &src.face_vertex_a,
            &src.face_vertex_b,
            &src.face_vertex_c,
            &src.face_colour_a,
            &src.face_colour_b,
            &src.face_colour_c,
            &src.face_texture_p,
            &src.face_texture_m,
            &src.face_texture_n,
        ] {
            Self::add_optional_vec_bytes(&mut bytes, values.as_ref(), false)?;
        }
        Self::add_default_vec_bytes(
            &mut bytes,
            src.face_alpha.as_ref(),
            usize::try_from(src.num_faces).ok()?,
        )?;
        Self::add_nested_vec_bytes(&mut bytes, src.label_faces.as_ref(), false)?;
        Self::add_nested_vec_bytes(&mut bytes, src.label_vertices.as_ref(), false)?;
        Some(bytes)
    }

    /// Set a fresh temporary model after charging its copied geometry and
    /// nested animation labels before allocation.
    pub fn try_set_for_render(
        &mut self,
        src: &Model,
        share_alpha: bool,
        budget: &ByteBudget,
    ) -> Result<(), RenderLimit> {
        let new_bytes =
            Self::checked_set_bytes(src).ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let model_size = std::mem::size_of::<Self>();
        let old_bytes = self
            .checked_retained_bytes(true)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let old_payload = old_bytes
            .checked_sub(model_size)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let new_payload = new_bytes
            .checked_sub(model_size)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let peak = model_size
            .checked_add(old_payload)
            .and_then(|bytes| bytes.checked_add(new_payload))
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        self.resize_render_charge(budget, peak)?;
        self.set(src, share_alpha);
        self.resize_render_charge(budget, self.retained_bytes())
    }

    /// Load a shared decoded model under a byte ceiling. Decoding is charged
    /// from metadata before any geometry allocation; only store-only entries
    /// are eligible for LRU eviction.
    pub fn try_load_shared_for_render(
        id: i32,
        budget: &ByteBudget,
    ) -> Result<Option<Arc<Model>>, RenderLimit> {
        let mut shared_store = GeometryStore::instance()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(model) = shared_store.find_model(id) {
            crate::render::store::prepare_render_model_slot(
                &mut shared_store,
                budget,
                id,
                Some(&model),
            )?;
            if model.has_render_charge_for(budget) {
                crate::render::store::track_render_model(id, &model);
                return Ok(Some(model));
            }
            if model.has_render_charge() {
                return Err(Self::render_error_for_overflow(budget));
            }
            let bytes = Self::shared_bytes_for_model(&model)
                .ok_or_else(|| Self::render_error_for_overflow(budget))?;
            let charge =
                crate::render::store::reserve_shared_bytes(&mut shared_store, budget, bytes)?;
            model
                .set_render_charge(charge)
                .unwrap_or_else(|_| panic!("cached model charge raced while store is locked"));
            crate::render::store::track_render_model(id, &model);
            return Ok(Some(model));
        }

        let estimated = {
            let metadata = store().lock().unwrap_or_else(|error| error.into_inner());
            let Some(meta) = metadata.meta.get(id as usize).and_then(Option::as_ref) else {
                return Ok(None);
            };
            if meta.src.is_none() {
                return Ok(None);
            }
            Self::checked_decoded_bytes(meta)
                .ok_or_else(|| Self::render_error_for_overflow(budget))?
        };
        crate::render::store::prepare_render_model_slot(&mut shared_store, budget, id, None)?;
        let charge =
            crate::render::store::reserve_shared_bytes(&mut shared_store, budget, estimated)?;
        let Some(model) = Self::load_raw_with_charge(id, charge) else {
            return Ok(None);
        };
        let shared = shared_store.publish_model(id, model);
        crate::render::store::track_render_model(id, &shared);
        Ok(Some(shared))
    }

    /// Load an independently owned render model, charging both the process
    /// shared decode and this caller's clone. A scoped budget separates the
    /// two owners; an unscoped explicit call uses the supplied budget for both.
    pub fn try_load_for_render(id: i32, budget: &ByteBudget) -> Result<Option<Model>, RenderLimit> {
        let shared_budget = RenderBudget::current_resource(RenderResource::SharedModels)
            .unwrap_or_else(|| budget.clone());
        let Some(shared) = Self::try_load_shared_for_render(id, &shared_budget)? else {
            return Ok(None);
        };
        Self::try_clone_for_render(&shared, budget).map(Some)
    }

    /// Move a charged model into shared Arc ownership, including the Arc
    /// allocation header in the same resource charge.
    pub fn try_into_arc_for_render(self, budget: &ByteBudget) -> Result<Arc<Model>, RenderLimit> {
        let arc_header = 2usize
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let bytes = self
            .checked_retained_bytes(true)
            .and_then(|bytes| bytes.checked_add(arc_header))
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        self.resize_render_charge(budget, bytes)?;
        Ok(Arc::new(self))
    }

    fn load_raw_with_charge(id: i32, charge: BudgetCharge) -> Option<Model> {
        let mut state = store().lock().unwrap_or_else(|error| error.into_inner());
        let has_source = state
            .meta
            .get(id as usize)
            .and_then(Option::as_ref)
            .and_then(|meta| meta.src.as_ref())
            .is_some();
        if !has_source {
            return None;
        }
        state.loaded += 1;
        let meta = state.meta.get(id as usize)?.as_ref()?;
        let model = Self::decode(meta);
        model
            .set_render_charge(charge)
            .unwrap_or_else(|_| panic!("fresh decoded model already had a charge"));
        Some(model)
    }

    fn checked_combine_for_anim_bytes(
        models: &[Option<Model>],
        count: usize,
    ) -> Option<(usize, i32, i32, i32)> {
        let mut points = 0usize;
        let mut faces = 0usize;
        let mut textures = 0usize;
        let mut copy_render_type = false;
        let mut copy_priority = false;
        let mut copy_alpha = false;
        let mut copy_labels = false;
        let mut priority = -1;
        for model in models.iter().take(count).flatten() {
            points = points.checked_add(usize::try_from(model.num_points).ok()?)?;
            faces = faces.checked_add(usize::try_from(model.num_faces).ok()?)?;
            textures = textures.checked_add(usize::try_from(model.num_t).ok()?)?;
            copy_render_type |= model.face_render_type.is_some();
            if model.face_priority.is_none() {
                if priority == -1 {
                    priority = model.priority;
                }
                copy_priority |= priority != model.priority;
            } else {
                copy_priority = true;
            }
            copy_alpha |= model.face_alpha.is_some();
            copy_labels |= model.face_label.is_some();
        }
        if points > i32::MAX as usize || faces > i32::MAX as usize || textures > i32::MAX as usize {
            return None;
        }
        let mut bytes = std::mem::size_of::<Self>();
        Self::byte_total_add(&mut bytes, points, 4 * std::mem::size_of::<i32>())?;
        Self::byte_total_add(&mut bytes, faces, 4 * std::mem::size_of::<i32>())?;
        Self::byte_total_add(&mut bytes, textures, 3 * std::mem::size_of::<i32>())?;
        if copy_render_type {
            Self::byte_total_add(&mut bytes, faces, std::mem::size_of::<i32>())?;
        }
        if copy_priority {
            Self::byte_total_add(&mut bytes, faces, std::mem::size_of::<i32>())?;
        }
        if copy_alpha {
            Self::byte_total_add(&mut bytes, faces, std::mem::size_of::<i32>())?;
        }
        if copy_labels {
            Self::byte_total_add(&mut bytes, faces, std::mem::size_of::<i32>())?;
        }
        Some((bytes, points as i32, faces as i32, textures as i32))
    }

    /// Merge animation parts only after checking the full output geometry.
    pub fn try_combine_for_anim(
        models: &[Option<Model>],
        count: usize,
        budget: &ByteBudget,
    ) -> Result<Model, RenderLimit> {
        let (bytes, _, _, _) = Self::checked_combine_for_anim_bytes(models, count)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let charge = budget.reserve(bytes)?;
        let combined = Self::combine_for_anim(models, count);
        combined
            .set_render_charge(charge)
            .unwrap_or_else(|_| panic!("fresh combined model already had a charge"));
        Ok(combined)
    }
    fn checked_combine_bytes(models: &[Model], count: usize) -> Option<usize> {
        let mut points = 0usize;
        let mut faces = 0usize;
        let mut textures = 0usize;
        let mut copy_render_type = false;
        let mut copy_priority = false;
        let mut copy_alpha = false;
        let mut copy_colour = false;
        let mut priority = -1;
        for model in models.iter().take(count) {
            points = points.checked_add(usize::try_from(model.num_points).ok()?)?;
            faces = faces.checked_add(usize::try_from(model.num_faces).ok()?)?;
            textures = textures.checked_add(usize::try_from(model.num_t).ok()?)?;
            copy_render_type |= model.face_render_type.is_some();
            if model.face_priority.is_none() {
                if priority == -1 {
                    priority = model.priority;
                }
                copy_priority |= priority != model.priority;
            } else {
                copy_priority = true;
            }
            copy_alpha |= model.face_alpha.is_some();
            copy_colour |= model.face_colour.is_some();
        }
        if points > i32::MAX as usize || faces > i32::MAX as usize || textures > i32::MAX as usize {
            return None;
        }
        let mut bytes = std::mem::size_of::<Self>();
        Self::byte_total_add(&mut bytes, points, 3 * std::mem::size_of::<i32>())?;
        Self::byte_total_add(&mut bytes, faces, 6 * std::mem::size_of::<i32>())?;
        Self::byte_total_add(&mut bytes, textures, 3 * std::mem::size_of::<i32>())?;
        for include in [copy_render_type, copy_priority, copy_alpha, copy_colour] {
            if include {
                Self::byte_total_add(&mut bytes, faces, std::mem::size_of::<i32>())?;
            }
        }
        Some(bytes)
    }

    /// Merge lit geometry only after admitting the full output allocation.
    pub fn try_combine(
        models: &[Model],
        count: usize,
        budget: &ByteBudget,
    ) -> Result<Model, RenderLimit> {
        let bytes = Self::checked_combine_bytes(models, count)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let charge = budget.reserve(bytes)?;
        let combined = Self::combine(models, count);
        combined
            .set_render_charge(charge)
            .unwrap_or_else(|_| panic!("fresh combined model already had a charge"));
        Ok(combined)
    }

    /// `Model.init(total, provider)` from client-ts.
    ///
    /// Grow-only: a second client must not throw away unpacked `meta` (the
    /// snapshot inject is process-wide). The provider is replaced so a
    /// later OnDemand can still fetch misses.
    pub fn init(total: i32, provider: Box<dyn ModelProvider + Send>) {
        let mut s = store().lock().unwrap();
        if s.meta.len() < total as usize {
            s.meta.resize_with(total as usize, || None);
        }
        s.provider = Some(provider);
    }

    /// Drop packed metadata, the OnDemand provider, and decoded geometry.
    /// Integration tests that assert "missing id → None" must call this
    /// after a sibling in the same binary has snapshot-injected
    /// `models.bin` into the process-wide stores. Not `cfg(test)`: those
    /// tests are a separate crate and see the lib's public API only.
    pub fn reset_for_tests() {
        {
            let mut s = store().lock().unwrap();
            s.meta.clear();
            s.provider = None;
            s.loaded = 0;
        }
        GeometryStore::instance().lock().unwrap().clear();
        crate::render::store::reset_render_model_index();
    }

    /// `Model.unpack(id, src)` from client-ts; parses the 18-byte trailer and
    /// stores the section offsets so `load` can decode lazily.
    pub fn unpack(id: i32, src: Option<&[u8]>) {
        let mut s = store().lock().unwrap();
        if id as usize >= s.meta.len() {
            s.meta.resize_with(id as usize + 1, || None);
        }
        if s.meta[id as usize]
            .as_ref()
            .is_some_and(|m| m.src.is_some())
        {
            return;
        }
        let Some(src) = src else {
            let meta = ModelMeta {
                src: None,
                num_points: 0,
                num_faces: 0,
                num_t: 0,
                vertex_order_offset: 0,
                vertex_x_offset: 0,
                vertex_y_offset: 0,
                vertex_z_offset: 0,
                vertex_label_offset: -1,
                face_index_offset: 0,
                face_index_order_offset: 0,
                face_colour_offset: 0,
                face_render_type_offset: -1,
                face_priority_offset: 0,
                face_alpha_offset: -1,
                face_label_offset: -1,
                face_texture_axis_offset: 0,
            };
            s.meta[id as usize] = Some(meta);
            return;
        };

        let mut trailer = Packet::new(src.to_vec());
        trailer.pos = src.len() - 18;

        let num_points = trailer.g2();
        let num_faces = trailer.g2();
        let num_t = trailer.g1();

        let has_render_type = trailer.g1();
        let priority = trailer.g1();
        let has_alpha = trailer.g1();
        let has_face_labels = trailer.g1();
        let has_vertex_labels = trailer.g1();

        let data_length_x = trailer.g2();
        let data_length_y = trailer.g2();
        let _data_length_z = trailer.g2();
        let data_length_face_index = trailer.g2();

        let mut pos = 0;
        let vertex_order_offset = pos;
        pos += num_points;

        let face_index_order_offset = pos;
        pos += num_faces;

        let mut face_priority_offset = pos;
        if priority == 255 {
            pos += num_faces;
        } else {
            face_priority_offset = -priority - 1;
        }

        let mut face_label_offset = pos;
        if has_face_labels == 1 {
            pos += num_faces;
        } else {
            face_label_offset = -1;
        }

        let mut face_render_type_offset = pos;
        if has_render_type == 1 {
            pos += num_faces;
        } else {
            face_render_type_offset = -1;
        }

        let mut vertex_label_offset = pos;
        if has_vertex_labels == 1 {
            pos += num_points;
        } else {
            vertex_label_offset = -1;
        }

        let mut face_alpha_offset = pos;
        if has_alpha == 1 {
            pos += num_faces;
        } else {
            face_alpha_offset = -1;
        }

        let face_index_offset = pos;
        pos += data_length_face_index;

        let face_colour_offset = pos;
        pos += num_faces * 2;

        let face_texture_axis_offset = pos;
        pos += num_t * 6;

        let vertex_x_offset = pos;
        pos += data_length_x;

        let vertex_y_offset = pos;
        pos += data_length_y;

        let vertex_z_offset = pos;

        s.meta[id as usize] = Some(ModelMeta {
            src: Some(src.to_vec()),
            num_points,
            num_faces,
            num_t,
            vertex_order_offset,
            vertex_x_offset,
            vertex_y_offset,
            vertex_z_offset,
            vertex_label_offset,
            face_index_offset,
            face_index_order_offset,
            face_colour_offset,
            face_render_type_offset,
            face_priority_offset,
            face_alpha_offset,
            face_label_offset,
            face_texture_axis_offset,
        });
    }

    /// `Model.unload(id)` from client-ts. The decoded geometry is dropped
    /// from the shared store too, so the next `load` re-requests from the
    /// provider instead of serving a stale decode (Task 5).
    pub fn unload(id: i32) {
        {
            let mut s = store().lock().unwrap();
            if (id as usize) < s.meta.len() {
                s.meta[id as usize] = None;
            }
        }
        let mut geometry = GeometryStore::instance().lock().unwrap();
        geometry.remove_model(id);
    }

    /// `Model.load(id)` from client-ts; decodes the model once its metadata
    /// is available, otherwise asks the provider for it and returns null.
    /// The decoded geometry is shared through the canonical `ModelStore`
    /// (Task 5): the first call decodes and publishes the `Arc`; later
    /// calls clone the immutable geometry out of the store, so the unpack
    /// runs once per model id while every caller's per-request transforms
    /// still apply to its own copy.
    pub fn load(id: i32) -> Option<Model> {
        Self::load_shared(id).map(|shared| (*shared).clone())
    }

    /// The shared form of `load`: the store's `Arc<Model>` for `id`,
    /// decoding from the packed source on the first request only. Two
    /// renderers that resolve the same model id get the same `Arc` (the
    /// store's load-count stays 1).
    pub fn load_shared(id: i32) -> Option<Arc<Model>> {
        let mut store = GeometryStore::instance().lock().unwrap();
        if let Some(shared) = store.find_model(id) {
            return Some(shared);
        }
        let model = Self::load_raw(id)?;
        Some(store.publish_model(id, model))
    }

    /// The raw decode from the unpacked metadata (the pre-task-5 `load`
    /// body): the metadata slot is taken and restored so a provider request
    /// is asked exactly once per unavailable id. `pub(super)` so the shared
    /// store can decode on a miss without routing back through `load`.
    pub(super) fn load_raw(id: i32) -> Option<Model> {
        let mut s = store().lock().unwrap();
        let slot = s.meta.get_mut(id as usize)?;
        let meta = slot.take()?;
        if meta.src.is_none() {
            *slot = Some(meta);
            return None;
        }
        s.loaded += 1;
        drop(s);
        let model = Self::decode(&meta);
        store().lock().unwrap().meta[id as usize] = Some(meta);
        Some(model)
    }

    /// `Model.requestDownload(id)` from client-ts.
    pub fn request_download(id: i32) -> bool {
        let mut s = store().lock().unwrap();
        if s.meta.get(id as usize).and_then(|m| m.as_ref()).is_none() {
            if let Some(provider) = s.provider.as_mut() {
                provider.request_model(id);
            }
            return false;
        }
        true
    }

    fn decode(meta: &ModelMeta) -> Model {
        let src = meta.src.as_deref().expect("model meta has data");
        let mut model = Model::default();
        model.num_points = meta.num_points;
        model.num_faces = meta.num_faces;
        model.num_t = meta.num_t;

        model.point_x = Some(vec![0; meta.num_points as usize]);
        model.point_y = Some(vec![0; meta.num_points as usize]);
        model.point_z = Some(vec![0; meta.num_points as usize]);

        model.face_vertex_a = Some(vec![0; meta.num_faces as usize]);
        model.face_vertex_b = Some(vec![0; meta.num_faces as usize]);
        model.face_vertex_c = Some(vec![0; meta.num_faces as usize]);

        model.face_texture_p = Some(vec![0; meta.num_t as usize]);
        model.face_texture_m = Some(vec![0; meta.num_t as usize]);
        model.face_texture_n = Some(vec![0; meta.num_t as usize]);

        if meta.vertex_label_offset >= 0 {
            model.vertex_label = Some(vec![0; meta.num_points as usize]);
        }
        if meta.face_render_type_offset >= 0 {
            model.face_render_type = Some(vec![0; meta.num_faces as usize]);
        }
        if meta.face_priority_offset >= 0 {
            model.face_priority = Some(vec![0; meta.num_faces as usize]);
        } else {
            model.priority = -meta.face_priority_offset - 1;
        }
        if meta.face_alpha_offset >= 0 {
            model.face_alpha = Some(vec![0; meta.num_faces as usize]);
        }
        if meta.face_label_offset >= 0 {
            model.face_label = Some(vec![0; meta.num_faces as usize]);
        }
        model.face_colour = Some(vec![0; meta.num_faces as usize]);

        let mut point1 = ModelCursor::at(src, meta.vertex_order_offset as usize);
        let mut point2 = ModelCursor::at(src, meta.vertex_x_offset as usize);
        let mut point3 = ModelCursor::at(src, meta.vertex_y_offset as usize);
        let mut point4 = ModelCursor::at(src, meta.vertex_z_offset as usize);
        let mut point5 = ModelCursor::at(src, meta.vertex_label_offset.max(0) as usize);

        let mut dx = 0;
        let mut dy = 0;
        let mut dz = 0;
        for v in 0..meta.num_points as usize {
            let order = point1.g1();
            let mut x = 0;
            if order & 0x1 != 0 {
                x = point2.gsmarts();
            }
            let mut y = 0;
            if order & 0x2 != 0 {
                y = point3.gsmarts();
            }
            let mut z = 0;
            if order & 0x4 != 0 {
                z = point4.gsmarts();
            }

            if let Some(px) = model.point_x.as_mut() {
                px[v] = dx + x;
            }
            if let Some(py) = model.point_y.as_mut() {
                py[v] = dy + y;
            }
            if let Some(pz) = model.point_z.as_mut() {
                pz[v] = dz + z;
            }

            dx = model.point_x.as_ref().unwrap()[v];
            dy = model.point_y.as_ref().unwrap()[v];
            dz = model.point_z.as_ref().unwrap()[v];

            if let Some(vl) = model.vertex_label.as_mut() {
                vl[v] = point5.g1();
            }
        }

        let mut face1 = ModelCursor::at(src, meta.face_colour_offset as usize);
        let mut face2 = ModelCursor::at(src, meta.face_render_type_offset.max(0) as usize);
        let mut face3 = ModelCursor::at(src, meta.face_priority_offset.max(0) as usize);
        let mut face4 = ModelCursor::at(src, meta.face_alpha_offset.max(0) as usize);
        let mut face5 = ModelCursor::at(src, meta.face_label_offset.max(0) as usize);

        for f in 0..meta.num_faces as usize {
            model.face_colour.as_mut().unwrap()[f] = face1.g2();
            if let Some(rt) = model.face_render_type.as_mut() {
                rt[f] = face2.g1();
            }
            if let Some(fp) = model.face_priority.as_mut() {
                fp[f] = face3.g1();
            }
            if let Some(fa) = model.face_alpha.as_mut() {
                fa[f] = face4.g1();
            }
            if let Some(fl) = model.face_label.as_mut() {
                fl[f] = face5.g1();
            }
        }

        let mut vertex1 = ModelCursor::at(src, meta.face_index_offset as usize);
        let mut vertex2 = ModelCursor::at(src, meta.face_index_order_offset as usize);

        let mut a = 0;
        let mut b = 0;
        let mut c = 0;
        let mut last = 0;
        for f in 0..meta.num_faces as usize {
            let order = vertex2.g1();
            if order == 1 {
                a = vertex1.gsmarts() + last;
                b = vertex1.gsmarts() + a;
                c = vertex1.gsmarts() + b;
                last = c;
            } else if order == 2 {
                b = c;
                c = vertex1.gsmarts() + last;
                last = c;
            } else if order == 3 {
                a = c;
                c = vertex1.gsmarts() + last;
                last = c;
            } else if order == 4 {
                let tmp = a;
                a = b;
                b = tmp;
                c = vertex1.gsmarts() + last;
                last = c;
            }

            model.face_vertex_a.as_mut().unwrap()[f] = a;
            model.face_vertex_b.as_mut().unwrap()[f] = b;
            model.face_vertex_c.as_mut().unwrap()[f] = c;
        }

        if meta.num_t > 0 {
            let mut axis = ModelCursor::at(src, meta.face_texture_axis_offset as usize);
            for f in 0..meta.num_t as usize {
                model.face_texture_p.as_mut().unwrap()[f] = axis.g2();
                model.face_texture_m.as_mut().unwrap()[f] = axis.g2();
                model.face_texture_n.as_mut().unwrap()[f] = axis.g2();
            }
        }

        model
    }

    /// `Model.combineForAnim(models, count)` from client-ts; dedupes vertices
    /// through `addPoint` and copies render metadata.
    pub fn combine_for_anim(models: &[Option<Model>], count: usize) -> Model {
        let mut combined = Model::default();
        store().lock().unwrap().loaded += 1;

        let mut copy_render_type = false;
        let mut copy_priority = false;
        let mut copy_alpha = false;
        let mut copy_labels = false;

        combined.priority = -1;

        for model in models.iter().take(count).flatten() {
            combined.num_points += model.num_points;
            combined.num_faces += model.num_faces;
            combined.num_t += model.num_t;

            if model.face_render_type.is_some() {
                copy_render_type = true;
            }
            if model.face_priority.is_none() {
                if combined.priority == -1 {
                    combined.priority = model.priority;
                }
                if combined.priority != model.priority {
                    copy_priority = true;
                }
            } else {
                copy_priority = true;
            }
            if model.face_alpha.is_some() {
                copy_alpha = true;
            }
            if model.face_label.is_some() {
                copy_labels = true;
            }
        }

        combined.point_x = Some(vec![0; combined.num_points as usize]);
        combined.point_y = Some(vec![0; combined.num_points as usize]);
        combined.point_z = Some(vec![0; combined.num_points as usize]);
        combined.vertex_label = Some(vec![0; combined.num_points as usize]);
        combined.face_vertex_a = Some(vec![0; combined.num_faces as usize]);
        combined.face_vertex_b = Some(vec![0; combined.num_faces as usize]);
        combined.face_vertex_c = Some(vec![0; combined.num_faces as usize]);
        combined.face_texture_p = Some(vec![0; combined.num_t as usize]);
        combined.face_texture_m = Some(vec![0; combined.num_t as usize]);
        combined.face_texture_n = Some(vec![0; combined.num_t as usize]);
        if copy_render_type {
            combined.face_render_type = Some(vec![0; combined.num_faces as usize]);
        }
        if copy_priority {
            combined.face_priority = Some(vec![0; combined.num_faces as usize]);
        }
        if copy_alpha {
            combined.face_alpha = Some(vec![0; combined.num_faces as usize]);
        }
        if copy_labels {
            combined.face_label = Some(vec![0; combined.num_faces as usize]);
        }
        combined.face_colour = Some(vec![0; combined.num_faces as usize]);

        combined.num_points = 0;
        combined.num_faces = 0;
        combined.num_t = 0;

        for model in models.iter().take(count).flatten() {
            for f in 0..model.num_faces as usize {
                if copy_render_type {
                    if let Some(src) = &model.face_render_type {
                        if let Some(dst) = combined.face_render_type.as_mut() {
                            dst[combined.num_faces as usize] = src[f];
                        }
                    } else if let Some(dst) = combined.face_render_type.as_mut() {
                        dst[combined.num_faces as usize] = 0;
                    }
                }
                if copy_priority {
                    if let Some(src) = &model.face_priority {
                        if let Some(dst) = combined.face_priority.as_mut() {
                            dst[combined.num_faces as usize] = src[f];
                        }
                    } else if let Some(dst) = combined.face_priority.as_mut() {
                        dst[combined.num_faces as usize] = model.priority;
                    }
                }
                if copy_alpha {
                    if let Some(src) = &model.face_alpha {
                        if let Some(dst) = combined.face_alpha.as_mut() {
                            dst[combined.num_faces as usize] = src[f];
                        }
                    } else if let Some(dst) = combined.face_alpha.as_mut() {
                        dst[combined.num_faces as usize] = 0;
                    }
                }
                if copy_labels {
                    if let Some(src) = &model.face_label {
                        combined.face_label.as_mut().unwrap()[combined.num_faces as usize] = src[f];
                    }
                }

                combined.face_colour.as_mut().unwrap()[combined.num_faces as usize] =
                    model.face_colour.as_ref().unwrap()[f];
                combined.face_vertex_a.as_mut().unwrap()[combined.num_faces as usize] =
                    combined.add_point(model, model.face_vertex_a.as_ref().unwrap()[f]);
                combined.face_vertex_b.as_mut().unwrap()[combined.num_faces as usize] =
                    combined.add_point(model, model.face_vertex_b.as_ref().unwrap()[f]);
                combined.face_vertex_c.as_mut().unwrap()[combined.num_faces as usize] =
                    combined.add_point(model, model.face_vertex_c.as_ref().unwrap()[f]);
                combined.num_faces += 1;
            }

            for f in 0..model.num_t as usize {
                combined.face_texture_p.as_mut().unwrap()[combined.num_t as usize] =
                    combined.add_point(model, model.face_texture_p.as_ref().unwrap()[f]);
                combined.face_texture_m.as_mut().unwrap()[combined.num_t as usize] =
                    combined.add_point(model, model.face_texture_m.as_ref().unwrap()[f]);
                combined.face_texture_n.as_mut().unwrap()[combined.num_t as usize] =
                    combined.add_point(model, model.face_texture_n.as_ref().unwrap()[f]);
                combined.num_t += 1;
            }
        }

        combined
    }

    /// `Model.combine(models, count)` from client-ts; lit models are merged
    /// with their precomputed vertex colours.
    pub fn combine(models: &[Model], count: usize) -> Model {
        let mut combined = Model::default();
        store().lock().unwrap().loaded += 1;

        let mut copy_render_type = false;
        let mut copy_priority = false;
        let mut copy_alpha = false;
        let mut copy_colour = false;

        combined.priority = -1;

        for model in models.iter().take(count) {
            combined.num_points += model.num_points;
            combined.num_faces += model.num_faces;
            combined.num_t += model.num_t;

            if model.face_render_type.is_some() {
                copy_render_type = true;
            }
            if model.face_priority.is_none() {
                if combined.priority == -1 {
                    combined.priority = model.priority;
                }
                if combined.priority != model.priority {
                    copy_priority = true;
                }
            } else {
                copy_priority = true;
            }
            if model.face_alpha.is_some() {
                copy_alpha = true;
            }
            if model.face_colour.is_some() {
                copy_colour = true;
            }
        }

        combined.point_x = Some(vec![0; combined.num_points as usize]);
        combined.point_y = Some(vec![0; combined.num_points as usize]);
        combined.point_z = Some(vec![0; combined.num_points as usize]);
        combined.face_vertex_a = Some(vec![0; combined.num_faces as usize]);
        combined.face_vertex_b = Some(vec![0; combined.num_faces as usize]);
        combined.face_vertex_c = Some(vec![0; combined.num_faces as usize]);
        combined.face_colour_a = Some(vec![0; combined.num_faces as usize]);
        combined.face_colour_b = Some(vec![0; combined.num_faces as usize]);
        combined.face_colour_c = Some(vec![0; combined.num_faces as usize]);
        combined.face_texture_p = Some(vec![0; combined.num_t as usize]);
        combined.face_texture_m = Some(vec![0; combined.num_t as usize]);
        combined.face_texture_n = Some(vec![0; combined.num_t as usize]);
        if copy_render_type {
            combined.face_render_type = Some(vec![0; combined.num_faces as usize]);
        }
        if copy_priority {
            combined.face_priority = Some(vec![0; combined.num_faces as usize]);
        }
        if copy_alpha {
            combined.face_alpha = Some(vec![0; combined.num_faces as usize]);
        }
        if copy_colour {
            combined.face_colour = Some(vec![0; combined.num_faces as usize]);
        }

        combined.num_points = 0;
        combined.num_faces = 0;
        combined.num_t = 0;

        for model in models.iter().take(count) {
            let vertex_count = combined.num_points;
            for v in 0..model.num_points as usize {
                combined.point_x.as_mut().unwrap()[combined.num_points as usize] =
                    model.point_x.as_ref().unwrap()[v];
                combined.point_y.as_mut().unwrap()[combined.num_points as usize] =
                    model.point_y.as_ref().unwrap()[v];
                combined.point_z.as_mut().unwrap()[combined.num_points as usize] =
                    model.point_z.as_ref().unwrap()[v];
                combined.num_points += 1;
            }

            for f in 0..model.num_faces as usize {
                combined.face_vertex_a.as_mut().unwrap()[combined.num_faces as usize] =
                    model.face_vertex_a.as_ref().unwrap()[f] + vertex_count;
                combined.face_vertex_b.as_mut().unwrap()[combined.num_faces as usize] =
                    model.face_vertex_b.as_ref().unwrap()[f] + vertex_count;
                combined.face_vertex_c.as_mut().unwrap()[combined.num_faces as usize] =
                    model.face_vertex_c.as_ref().unwrap()[f] + vertex_count;

                combined.face_colour_a.as_mut().unwrap()[combined.num_faces as usize] =
                    model.face_colour_a.as_ref().unwrap()[f];
                combined.face_colour_b.as_mut().unwrap()[combined.num_faces as usize] =
                    model.face_colour_b.as_ref().unwrap()[f];
                combined.face_colour_c.as_mut().unwrap()[combined.num_faces as usize] =
                    model.face_colour_c.as_ref().unwrap()[f];

                if copy_render_type {
                    if let Some(src) = &model.face_render_type {
                        if let Some(dst) = combined.face_render_type.as_mut() {
                            dst[combined.num_faces as usize] = src[f];
                        }
                    } else if let Some(dst) = combined.face_render_type.as_mut() {
                        dst[combined.num_faces as usize] = 0;
                    }
                }
                if copy_priority {
                    if let Some(src) = &model.face_priority {
                        if let Some(dst) = combined.face_priority.as_mut() {
                            dst[combined.num_faces as usize] = src[f];
                        }
                    } else if let Some(dst) = combined.face_priority.as_mut() {
                        dst[combined.num_faces as usize] = model.priority;
                    }
                }
                if copy_alpha {
                    if let Some(src) = &model.face_alpha {
                        if let Some(dst) = combined.face_alpha.as_mut() {
                            dst[combined.num_faces as usize] = src[f];
                        }
                    } else if let Some(dst) = combined.face_alpha.as_mut() {
                        dst[combined.num_faces as usize] = 0;
                    }
                }
                if copy_colour {
                    if let Some(src) = &model.face_colour {
                        combined.face_colour.as_mut().unwrap()[combined.num_faces as usize] =
                            src[f];
                    }
                }
                combined.num_faces += 1;
            }

            for f in 0..model.num_t as usize {
                combined.face_texture_p.as_mut().unwrap()[combined.num_t as usize] =
                    model.face_texture_p.as_ref().unwrap()[f] + vertex_count;
                combined.face_texture_m.as_mut().unwrap()[combined.num_t as usize] =
                    model.face_texture_m.as_ref().unwrap()[f] + vertex_count;
                combined.face_texture_n.as_mut().unwrap()[combined.num_t as usize] =
                    model.face_texture_n.as_ref().unwrap()[f] + vertex_count;
                combined.num_t += 1;
            }
        }

        combined.calc_bounding_cylinder();
        combined
    }

    /// `Model.copyForAnim(src, shareColours, shareAlpha, shareVertices)`
    /// from client-ts. The TS shares arrays in the cache-hot case; this port
    /// always copies (the shared path never mutates the shared arrays).
    pub fn copy_for_anim(
        src: &Model,
        _share_colours: bool,
        _share_alpha: bool,
        _share_vertices: bool,
    ) -> Model {
        let mut model = Model::default();
        store().lock().unwrap().loaded += 1;

        model.num_points = src.num_points;
        model.num_faces = src.num_faces;
        model.num_t = src.num_t;

        model.point_x = Some(src.point_x.as_ref().map_or_else(Vec::new, |p| p.clone()));
        model.point_y = Some(src.point_y.as_ref().map_or_else(Vec::new, |p| p.clone()));
        model.point_z = Some(src.point_z.as_ref().map_or_else(Vec::new, |p| p.clone()));

        model.face_colour = Some(
            src.face_colour
                .as_ref()
                .map_or_else(Vec::new, |f| f.clone()),
        );

        model.face_alpha = Some(match &src.face_alpha {
            Some(fa) => fa.clone(),
            None => vec![0; src.num_faces as usize],
        });

        model.vertex_label = src.vertex_label.clone();
        model.face_label = src.face_label.clone();
        model.face_render_type = src.face_render_type.clone();
        model.face_vertex_a = src.face_vertex_a.clone();
        model.face_vertex_b = src.face_vertex_b.clone();
        model.face_vertex_c = src.face_vertex_c.clone();
        model.face_priority = src.face_priority.clone();
        model.priority = src.priority;
        model.face_texture_p = src.face_texture_p.clone();
        model.face_texture_m = src.face_texture_m.clone();
        model.face_texture_n = src.face_texture_n.clone();

        model
    }

    /// `Model.hillSkewCopy(src, copyVertexY, copyFaces)` from client-ts.
    pub fn hill_skew_copy(src: &Model, copy_vertex_y: bool, copy_faces: bool) -> Model {
        let mut model = Model::default();
        store().lock().unwrap().loaded += 1;

        model.num_points = src.num_points;
        model.num_faces = src.num_faces;
        model.num_t = src.num_t;

        if copy_vertex_y {
            model.point_y = Some(src.point_y.as_ref().map_or_else(Vec::new, |p| p.clone()));
        } else {
            model.point_y = src.point_y.clone();
        }

        if copy_faces {
            model.face_colour_a = Some(
                src.face_colour_a
                    .as_ref()
                    .map_or_else(Vec::new, |f| f.clone()),
            );
            model.face_colour_b = Some(
                src.face_colour_b
                    .as_ref()
                    .map_or_else(Vec::new, |f| f.clone()),
            );
            model.face_colour_c = Some(
                src.face_colour_c
                    .as_ref()
                    .map_or_else(Vec::new, |f| f.clone()),
            );

            model.face_render_type = Some(match &src.face_render_type {
                Some(rt) => rt.clone(),
                None => vec![0; src.num_faces as usize],
            });

            model.point_normal = Some(match &src.point_normal {
                Some(pn) => pn.clone(),
                None => (0..src.num_points)
                    .map(|_| Some(PointNormal::default()))
                    .collect(),
            });

            model.shared_point_normal = src.shared_point_normal.clone();
        } else {
            model.face_colour_a = src.face_colour_a.clone();
            model.face_colour_b = src.face_colour_b.clone();
            model.face_colour_c = src.face_colour_c.clone();
            model.face_render_type = src.face_render_type.clone();
        }

        model.point_x = src.point_x.clone();
        model.point_z = src.point_z.clone();
        model.face_colour = src.face_colour.clone();
        model.face_alpha = src.face_alpha.clone();
        model.face_priority = src.face_priority.clone();
        model.priority = src.priority;
        model.face_vertex_a = src.face_vertex_a.clone();
        model.face_vertex_b = src.face_vertex_b.clone();
        model.face_vertex_c = src.face_vertex_c.clone();
        model.face_texture_p = src.face_texture_p.clone();
        model.face_texture_m = src.face_texture_m.clone();
        model.face_texture_n = src.face_texture_n.clone();

        model.min_y = src.min_y;
        model.max_y = src.max_y;
        model.radius = src.radius;
        model.min_depth = src.min_depth;
        model.max_depth = src.max_depth;
        model.min_x = src.min_x;
        model.max_x = src.max_x;
        model.min_z = src.min_z;
        model.max_z = src.max_z;

        model
    }

    /// `Model.tempModel` from client-ts; this port returns a fresh scratch
    /// model instead of the shared singleton.
    pub fn temp_model() -> Model {
        Model::default()
    }

    /// `set(src, shareAlpha)` from client-ts.
    pub fn set(&mut self, src: &Model, _share_alpha: bool) {
        self.num_points = src.num_points;
        self.num_faces = src.num_faces;
        self.num_t = src.num_t;

        self.point_x = Some(src.point_x.as_ref().map_or_else(Vec::new, |p| p.clone()));
        self.point_y = Some(src.point_y.as_ref().map_or_else(Vec::new, |p| p.clone()));
        self.point_z = Some(src.point_z.as_ref().map_or_else(Vec::new, |p| p.clone()));

        self.face_alpha = Some(match &src.face_alpha {
            Some(fa) => fa.clone(),
            None => vec![0; src.num_faces as usize],
        });

        self.face_render_type = src.face_render_type.clone();
        self.face_colour = src.face_colour.clone();
        self.face_priority = src.face_priority.clone();
        self.priority = src.priority;
        self.label_faces = src.label_faces.clone();
        self.label_vertices = src.label_vertices.clone();
        self.face_vertex_a = src.face_vertex_a.clone();
        self.face_vertex_b = src.face_vertex_b.clone();
        self.face_vertex_c = src.face_vertex_c.clone();
        self.face_colour_a = src.face_colour_a.clone();
        self.face_colour_b = src.face_colour_b.clone();
        self.face_colour_c = src.face_colour_c.clone();
        self.face_texture_p = src.face_texture_p.clone();
        self.face_texture_m = src.face_texture_m.clone();
        self.face_texture_n = src.face_texture_n.clone();
    }

    /// `addPoint(src, vertex)` from client-ts; returns a deduplicated index.
    pub fn add_point(&mut self, src: &Model, vertex: i32) -> i32 {
        let x = src.point_x.as_ref().unwrap()[vertex as usize];
        let y = src.point_y.as_ref().unwrap()[vertex as usize];
        let z = src.point_z.as_ref().unwrap()[vertex as usize];

        for v in 0..self.num_points as usize {
            if self.point_x.as_ref().unwrap()[v] == x
                && self.point_y.as_ref().unwrap()[v] == y
                && self.point_z.as_ref().unwrap()[v] == z
            {
                return v as i32;
            }
        }

        self.point_x.as_mut().unwrap()[self.num_points as usize] = x;
        self.point_y.as_mut().unwrap()[self.num_points as usize] = y;
        self.point_z.as_mut().unwrap()[self.num_points as usize] = z;

        if let Some(vl) = &src.vertex_label {
            self.vertex_label.as_mut().unwrap()[self.num_points as usize] = vl[vertex as usize];
        }

        let index = self.num_points;
        self.num_points += 1;
        index
    }

    /// `calcBoundingCylinder()` from client-ts.
    pub fn calc_bounding_cylinder(&mut self) {
        self.min_y = 0;
        self.radius = 0;
        self.max_y = 0;

        for i in 0..self.num_points as usize {
            let x = self.point_x.as_ref().unwrap()[i];
            let y = self.point_y.as_ref().unwrap()[i];
            let z = self.point_z.as_ref().unwrap()[i];

            if -y > self.min_y {
                self.min_y = -y;
            }
            if y > self.max_y {
                self.max_y = y;
            }

            let radius_sqr = x * x + z * z;
            if radius_sqr > self.radius {
                self.radius = radius_sqr;
            }
        }

        self.radius = ((self.radius as f64).sqrt() + 0.99) as i32;
        self.min_depth =
            (((self.radius * self.radius + self.min_y * self.min_y) as f64).sqrt() + 0.99) as i32;
        self.max_depth = self.min_depth
            + (((self.radius * self.radius + self.max_y * self.max_y) as f64).sqrt() + 0.99) as i32;
    }

    /// `recalcBoundingCylinder()` from client-ts.
    pub fn recalc_bounding_cylinder(&mut self) {
        self.min_y = 0;
        self.max_y = 0;

        for i in 0..self.num_points as usize {
            let y = self.point_y.as_ref().unwrap()[i];
            if -y > self.min_y {
                self.min_y = -y;
            }
            if y > self.max_y {
                self.max_y = y;
            }
        }

        self.min_depth =
            (((self.radius * self.radius + self.min_y * self.min_y) as f64).sqrt() + 0.99) as i32;
        self.max_depth = self.min_depth
            + (((self.radius * self.radius + self.max_y * self.max_y) as f64).sqrt() + 0.99) as i32;
    }

    /// `calcBoundingCube()` from client-ts; used for sharelit models.
    fn calc_bounding_cube(&mut self) {
        self.min_y = 0;
        self.radius = 0;
        self.max_y = 0;
        self.min_x = 999999;
        self.max_x = -999999;
        self.max_z = -99999;
        self.min_z = 99999;

        for v in 0..self.num_points as usize {
            let x = self.point_x.as_ref().unwrap()[v];
            let y = self.point_y.as_ref().unwrap()[v];
            let z = self.point_z.as_ref().unwrap()[v];

            if x < self.min_x {
                self.min_x = x;
            }
            if x > self.max_x {
                self.max_x = x;
            }
            if z < self.min_z {
                self.min_z = z;
            }
            if z > self.max_z {
                self.max_z = z;
            }
            if -y > self.min_y {
                self.min_y = -y;
            }
            if y > self.max_y {
                self.max_y = y;
            }

            let radius_sqr = x * x + z * z;
            if radius_sqr > self.radius {
                self.radius = radius_sqr;
            }
        }

        self.radius = (self.radius as f64).sqrt() as i32;
        self.min_depth =
            ((self.radius * self.radius + self.min_y * self.min_y) as f64).sqrt() as i32;
        self.max_depth = self.min_depth
            + ((self.radius * self.radius + self.max_y * self.max_y) as f64).sqrt() as i32;
    }

    /// `prepareAnim()` from client-ts; builds the label tables used by
    /// `animate`/`maskAnimate`.
    fn checked_label_table_bytes(labels: Option<&Vec<i32>>) -> Option<usize> {
        let Some(labels) = labels else {
            return Some(0);
        };
        let max_label = labels.iter().copied().max().unwrap_or(0);
        let outer_count = usize::try_from(max_label).ok()?.checked_add(1)?;
        let mut bytes = 0usize;
        Self::byte_total_add(
            &mut bytes,
            outer_count,
            std::mem::size_of::<Option<Vec<i32>>>(),
        )?;
        Self::byte_total_add(&mut bytes, labels.len(), std::mem::size_of::<i32>())?;
        Some(bytes)
    }

    pub fn try_prepare_anim_for_render(&mut self, budget: &ByteBudget) -> Result<(), RenderLimit> {
        let vertex_bytes = Self::checked_label_table_bytes(self.vertex_label.as_ref())
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let face_bytes = Self::checked_label_table_bytes(self.face_label.as_ref())
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let current_bytes = self
            .checked_retained_bytes(true)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        let peak = current_bytes
            .checked_add(vertex_bytes)
            .and_then(|bytes| bytes.checked_add(face_bytes))
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        self.resize_render_charge(budget, peak)?;
        self.prepare_anim();
        self.resize_render_charge(budget, self.retained_bytes())
    }

    pub fn prepare_anim(&mut self) {
        if let Some(vl) = self.vertex_label.take() {
            let mut label_vertex_count = [0i32; 256];
            let mut count = 0i32;
            for &label in &vl {
                label_vertex_count[label as usize] += 1;
                if label > count {
                    count = label;
                }
            }

            let mut label_vertices = Vec::with_capacity(count as usize + 1);
            for label in 0..=count {
                let n = label_vertex_count[label as usize] as usize;
                label_vertices.push(Some(vec![0i32; n]));
                label_vertex_count[label as usize] = 0;
            }

            let mut v = 0usize;
            while v < vl.len() {
                let label = vl[v] as usize;
                if let Some(verts) = label_vertices.get_mut(label) {
                    if let Some(verts) = verts {
                        verts[label_vertex_count[label] as usize] = v as i32;
                        label_vertex_count[label] += 1;
                        v += 1;
                        continue;
                    }
                }
                v += 1;
            }

            self.label_vertices = Some(label_vertices);
        }

        if let Some(fl) = self.face_label.take() {
            let mut label_face_count = [0i32; 256];
            let mut count = 0i32;
            for &label in &fl {
                label_face_count[label as usize] += 1;
                if label > count {
                    count = label;
                }
            }

            let mut label_faces = Vec::with_capacity(count as usize + 1);
            for label in 0..=count {
                let n = label_face_count[label as usize] as usize;
                label_faces.push(Some(vec![0i32; n]));
                label_face_count[label as usize] = 0;
            }

            let mut face = 0usize;
            while face < fl.len() {
                let label = fl[face] as usize;
                if let Some(faces) = label_faces.get_mut(label) {
                    if let Some(faces) = faces {
                        faces[label_face_count[label] as usize] = face as i32;
                        label_face_count[label] += 1;
                        face += 1;
                        continue;
                    }
                }
                face += 1;
            }

            self.label_faces = Some(label_faces);
        }
    }

    /// `animate(id)` from client-ts.
    pub fn animate(&mut self, id: i32) {
        if self.label_vertices.is_none() || id == -1 {
            return;
        }
        let Some(transform) = AnimFrame::get(id) else {
            return;
        };
        let Some(base) = &transform.base else { return };

        let mut origin = (0i32, 0i32, 0i32);

        for i in 0..transform.size as usize {
            let Some(ti) = &transform.ti else { continue };
            let Some(tx) = &transform.tx else { continue };
            let Some(ty) = &transform.ty else { continue };
            let Some(tz) = &transform.tz else { continue };
            let Some(base_labels) = &base.labels else {
                continue;
            };
            let Some(base_type) = &base.r#type else {
                continue;
            };

            let ti = ti[i] as usize;
            let (Some(labels), Some(&r#type)) = (
                base_labels.get(ti).and_then(|l| l.as_deref()),
                base_type.get(ti),
            ) else {
                continue;
            };
            origin = self.animate2(tx[i], ty[i], tz[i], labels, r#type as i32, origin);
        }
    }

    /// `maskAnimate(primaryId, secondaryId, mask)` from client-ts.
    pub fn mask_animate(&mut self, primary_id: i32, secondary_id: i32, mask: Option<&[i32]>) {
        if primary_id == -1 {
            return;
        }

        let Some(mask) = mask else {
            self.animate(primary_id);
            return;
        };

        let Some(primary) = AnimFrame::get(primary_id) else {
            return;
        };
        let Some(secondary) = AnimFrame::get(secondary_id) else {
            self.animate(primary_id);
            return;
        };

        let Some(skeleton) = &primary.base else {
            return;
        };
        let skeleton_type = skeleton.r#type.as_deref().unwrap_or(&[]);
        let skeleton_labels = skeleton.labels.as_deref().unwrap_or(&[]);

        let mut counter = 0usize;
        let mut mask_base = mask[counter];
        counter += 1;

        let mut origin = (0i32, 0i32, 0i32);
        for i in 0..primary.size as usize {
            let Some(ti) = &primary.ti else { continue };
            let Some(tx) = &primary.tx else { continue };
            let Some(ty) = &primary.ty else { continue };
            let Some(tz) = &primary.tz else { continue };
            let base = ti[i];
            while base > mask_base {
                if counter >= mask.len() {
                    break;
                }
                mask_base = mask[counter];
                counter += 1;
            }
            // Java Model.maskAnimate primary: `ti != walkmerge || type == 0`
            // (type 0 ORIGIN always). Secondary below uses `==`.
            if base != mask_base
                || skeleton_type.get(base as usize).copied().unwrap_or(0) as i32 == 0
            {
                if let Some(labels) = skeleton_labels
                    .get(base as usize)
                    .and_then(|l| l.as_deref())
                {
                    origin = self.animate2(
                        tx[i],
                        ty[i],
                        tz[i],
                        labels,
                        skeleton_type.get(base as usize).copied().unwrap_or(0) as i32,
                        origin,
                    );
                }
            }
        }

        counter = 0;
        mask_base = mask[counter];
        counter += 1;

        origin = (0i32, 0i32, 0i32);
        for i in 0..secondary.size as usize {
            let Some(ti) = &secondary.ti else { continue };
            let Some(tx) = &secondary.tx else { continue };
            let Some(ty) = &secondary.ty else { continue };
            let Some(tz) = &secondary.tz else { continue };
            let base = ti[i];
            while base > mask_base {
                if counter >= mask.len() {
                    break;
                }
                mask_base = mask[counter];
                counter += 1;
            }
            if base == mask_base
                || skeleton_type.get(base as usize).copied().unwrap_or(0) as i32 == 0
            {
                if let Some(labels) = skeleton_labels
                    .get(base as usize)
                    .and_then(|l| l.as_deref())
                {
                    // Java animate2 mutates instance oX/oY/oZ; thread the
                    // returned origin so a later ROTATE/SCALE uses it.
                    origin = self.animate2(
                        tx[i],
                        ty[i],
                        tz[i],
                        labels,
                        skeleton_type.get(base as usize).copied().unwrap_or(0) as i32,
                        origin,
                    );
                }
            }
        }
    }

    /// `animate2(x, y, z, labels, type)` from client-ts; the anim origin is
    /// threaded through the call (the TS keeps it on `Model.oX/oY/oZ`).
    #[allow(clippy::too_many_arguments)]
    fn animate2(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        labels: &[u8],
        r#type: i32,
        origin: (i32, i32, i32),
    ) -> (i32, i32, i32) {
        let (ox, oy, oz) = origin;
        if r#type == crate::dash3d::AnimTransform::ORIGIN {
            let mut count = 0i32;
            let mut tx = 0i32;
            let mut ty = 0i32;
            let mut tz = 0i32;

            let label_vertices = self.label_vertices.as_deref();
            for &label in labels {
                let Some(lv) = label_vertices else { continue };
                let Some(vertices) = lv.get(label as usize).and_then(|v| v.as_deref()) else {
                    continue;
                };
                for &v in vertices {
                    tx += self.point_x.as_ref().unwrap()[v as usize];
                    ty += self.point_y.as_ref().unwrap()[v as usize];
                    tz += self.point_z.as_ref().unwrap()[v as usize];
                    count += 1;
                }
            }

            if count > 0 {
                (tx / count + x, ty / count + y, tz / count + z)
            } else {
                (x, y, z)
            }
        } else if r#type == crate::dash3d::AnimTransform::TRANSLATE {
            for &group in labels {
                let Some(lv) = self.label_vertices.as_deref() else {
                    continue;
                };
                let Some(vertices) = lv.get(group as usize).and_then(|v| v.as_deref()) else {
                    continue;
                };
                for &v in vertices {
                    self.point_x.as_mut().unwrap()[v as usize] += x;
                    self.point_y.as_mut().unwrap()[v as usize] += y;
                    self.point_z.as_mut().unwrap()[v as usize] += z;
                }
            }
            (ox, oy, oz)
        } else if r#type == crate::dash3d::AnimTransform::ROTATE {
            for &group in labels {
                let Some(lv) = self.label_vertices.as_deref() else {
                    continue;
                };
                let Some(vertices) = lv.get(group as usize).and_then(|v| v.as_deref()) else {
                    continue;
                };
                for &v in vertices {
                    let v = v as usize;
                    self.point_x.as_mut().unwrap()[v] -= ox;
                    self.point_y.as_mut().unwrap()[v] -= oy;
                    self.point_z.as_mut().unwrap()[v] -= oz;

                    let pitch = ((x & 0xff) * 8) as usize;
                    let yaw = ((y & 0xff) * 8) as usize;
                    let roll = ((z & 0xff) * 8) as usize;
                    let sin_table = crate::graphics::Pix3D::sin_table();
                    let cos_table = crate::graphics::Pix3D::cos_table();

                    if roll != 0 {
                        let sin = sin_table[roll];
                        let cos = cos_table[roll];
                        let x_ = (self.point_y.as_ref().unwrap()[v] * sin
                            + self.point_x.as_ref().unwrap()[v] * cos)
                            >> 16;
                        self.point_y.as_mut().unwrap()[v] = (self.point_y.as_ref().unwrap()[v]
                            * cos
                            - self.point_x.as_ref().unwrap()[v] * sin)
                            >> 16;
                        self.point_x.as_mut().unwrap()[v] = x_;
                    }

                    if pitch != 0 {
                        let sin = sin_table[pitch];
                        let cos = cos_table[pitch];
                        let y_ = (self.point_y.as_ref().unwrap()[v] * cos
                            - self.point_z.as_ref().unwrap()[v] * sin)
                            >> 16;
                        self.point_z.as_mut().unwrap()[v] = (self.point_y.as_ref().unwrap()[v]
                            * sin
                            + self.point_z.as_ref().unwrap()[v] * cos)
                            >> 16;
                        self.point_y.as_mut().unwrap()[v] = y_;
                    }

                    if yaw != 0 {
                        let sin = sin_table[yaw];
                        let cos = cos_table[yaw];
                        let x_ = (self.point_z.as_ref().unwrap()[v] * sin
                            + self.point_x.as_ref().unwrap()[v] * cos)
                            >> 16;
                        self.point_z.as_mut().unwrap()[v] = (self.point_z.as_ref().unwrap()[v]
                            * cos
                            - self.point_x.as_ref().unwrap()[v] * sin)
                            >> 16;
                        self.point_x.as_mut().unwrap()[v] = x_;
                    }

                    self.point_x.as_mut().unwrap()[v] += ox;
                    self.point_y.as_mut().unwrap()[v] += oy;
                    self.point_z.as_mut().unwrap()[v] += oz;
                }
            }
            (ox, oy, oz)
        } else if r#type == crate::dash3d::AnimTransform::SCALE {
            for &group in labels {
                let Some(lv) = self.label_vertices.as_deref() else {
                    continue;
                };
                let Some(vertices) = lv.get(group as usize).and_then(|v| v.as_deref()) else {
                    continue;
                };
                for &v in vertices {
                    let v = v as usize;
                    self.point_x.as_mut().unwrap()[v] -= ox;
                    self.point_y.as_mut().unwrap()[v] -= oy;
                    self.point_z.as_mut().unwrap()[v] -= oz;

                    self.point_x.as_mut().unwrap()[v] =
                        (self.point_x.as_ref().unwrap()[v] * x) / 128;
                    self.point_y.as_mut().unwrap()[v] =
                        (self.point_y.as_ref().unwrap()[v] * y) / 128;
                    self.point_z.as_mut().unwrap()[v] =
                        (self.point_z.as_ref().unwrap()[v] * z) / 128;

                    self.point_x.as_mut().unwrap()[v] += ox;
                    self.point_y.as_mut().unwrap()[v] += oy;
                    self.point_z.as_mut().unwrap()[v] += oz;
                }
            }
            (ox, oy, oz)
        } else if r#type == crate::dash3d::AnimTransform::TRANSPARENCY {
            let Some(lf) = self.label_faces.as_deref() else {
                return (ox, oy, oz);
            };
            let Some(fa) = self.face_alpha.as_mut() else {
                return (ox, oy, oz);
            };
            for &label in labels {
                let Some(faces) = lf.get(label as usize).and_then(|f| f.as_deref()) else {
                    continue;
                };
                for &t in faces {
                    fa[t as usize] += x * 8;
                    if fa[t as usize] < 0 {
                        fa[t as usize] = 0;
                    }
                    if fa[t as usize] > 255 {
                        fa[t as usize] = 255;
                    }
                }
            }
            (ox, oy, oz)
        } else {
            (ox, oy, oz)
        }
    }

    /// `rotate90()` from client-ts.
    pub fn rotate90(&mut self) {
        for v in 0..self.num_points as usize {
            let tmp = self.point_x.as_ref().unwrap()[v];
            self.point_x.as_mut().unwrap()[v] = self.point_z.as_ref().unwrap()[v];
            self.point_z.as_mut().unwrap()[v] = -tmp;
        }
    }

    /// `rotateXAxis(angle)` from client-ts (1406).
    pub fn rotate_x_axis(&mut self, angle: i32) {
        let sin = Pix3D::sin_table()[angle as usize];
        let cos = Pix3D::cos_table()[angle as usize];

        for v in 0..self.num_points as usize {
            let tmp = (self.point_y.as_ref().unwrap()[v] * cos
                - self.point_z.as_ref().unwrap()[v] * sin)
                >> 16;
            self.point_z.as_mut().unwrap()[v] = (self.point_y.as_ref().unwrap()[v] * sin
                + self.point_z.as_ref().unwrap()[v] * cos)
                >> 16;
            self.point_y.as_mut().unwrap()[v] = tmp;
        }
    }

    /// `translate(y, x, z)` from client-ts (y-first argument order).
    pub fn translate(&mut self, y: i32, x: i32, z: i32) {
        for v in 0..self.num_points as usize {
            self.point_x.as_mut().unwrap()[v] += x;
            self.point_y.as_mut().unwrap()[v] += y;
            self.point_z.as_mut().unwrap()[v] += z;
        }
    }

    /// `recolour(src, dst)` from client-ts.
    pub fn recolour(&mut self, src: i32, dst: i32) {
        let Some(fc) = self.face_colour.as_mut() else {
            return;
        };
        for colour in fc.iter_mut() {
            if *colour == src {
                *colour = dst;
            }
        }
    }

    /// `mirror()` from client-ts.
    pub fn mirror(&mut self) {
        for v in 0..self.num_points as usize {
            self.point_z.as_mut().unwrap()[v] = -self.point_z.as_ref().unwrap()[v];
        }
        for f in 0..self.num_faces as usize {
            let tmp = self.face_vertex_a.as_ref().unwrap()[f];
            self.face_vertex_a.as_mut().unwrap()[f] = self.face_vertex_c.as_ref().unwrap()[f];
            self.face_vertex_c.as_mut().unwrap()[f] = tmp;
        }
    }

    /// `resize(x, y, z)` from client-ts.
    pub fn resize(&mut self, x: i32, y: i32, z: i32) {
        for v in 0..self.num_points as usize {
            self.point_x.as_mut().unwrap()[v] = (self.point_x.as_ref().unwrap()[v] * x) / 128;
            self.point_y.as_mut().unwrap()[v] = (self.point_y.as_ref().unwrap()[v] * y) / 128;
            self.point_z.as_mut().unwrap()[v] = (self.point_z.as_ref().unwrap()[v] * z) / 128;
        }
    }

    /// `calculateNormals(ambient, contrast, x, y, z, doNotShareLight)` from
    /// client-ts.
    fn checked_calculate_normals_peak_bytes(&self, do_not_share_light: bool) -> Option<usize> {
        let mut running = self.checked_retained_bytes(true)?;
        let mut peak = running;
        let faces = usize::try_from(self.num_faces).ok()?;
        let points = usize::try_from(self.num_points).ok()?;
        let colour_bytes = faces.checked_mul(std::mem::size_of::<i32>())?;
        if self.face_colour_a.is_none()
            || self.face_colour_b.is_none()
            || self.face_colour_c.is_none()
        {
            for old in [
                self.face_colour_a.as_ref(),
                self.face_colour_b.as_ref(),
                self.face_colour_c.as_ref(),
            ] {
                let old_bytes = if let Some(values) = old {
                    values.capacity().checked_mul(std::mem::size_of::<i32>())?
                } else {
                    0
                };
                let allocated = running.checked_add(colour_bytes)?;
                peak = peak.max(allocated);
                running = allocated.checked_sub(old_bytes)?;
            }
        }
        let new_normal_bytes = points.checked_mul(std::mem::size_of::<Option<PointNormal>>())?;
        if self.point_normal.is_none() {
            running = running.checked_add(new_normal_bytes)?;
            peak = peak.max(running);
        }
        if !do_not_share_light {
            let old_shared_bytes = if let Some(values) = &self.shared_point_normal {
                values
                    .capacity()
                    .checked_mul(std::mem::size_of::<Option<PointNormal>>())?
            } else {
                0
            };
            let cloned_normal_count = self.point_normal.as_ref().map_or(points, Vec::len);
            let cloned_normal_bytes =
                cloned_normal_count.checked_mul(std::mem::size_of::<Option<PointNormal>>())?;
            let allocated = running.checked_add(cloned_normal_bytes)?;
            peak = peak.max(allocated);
            let _final_running = allocated.checked_sub(old_shared_bytes)?;
        }
        Some(peak)
    }

    pub fn try_calculate_normals_for_render(
        &mut self,
        ambient: i32,
        contrast: i32,
        x: i32,
        y: i32,
        z: i32,
        do_not_share_light: bool,
        budget: &ByteBudget,
    ) -> Result<(), RenderLimit> {
        let peak = self
            .checked_calculate_normals_peak_bytes(do_not_share_light)
            .ok_or_else(|| Self::render_error_for_overflow(budget))?;
        self.resize_render_charge(budget, peak)?;
        self.calculate_normals(ambient, contrast, x, y, z, do_not_share_light);
        self.resize_render_charge(budget, self.retained_bytes())
    }

    pub fn calculate_normals(
        &mut self,
        ambient: i32,
        contrast: i32,
        x: i32,
        y: i32,
        z: i32,
        do_not_share_light: bool,
    ) {
        let light_magnitude = ((x * x + y * y + z * z) as f64).sqrt() as i32;
        let scale = (contrast * light_magnitude) >> 8;

        if self.face_colour_a.is_none()
            || self.face_colour_b.is_none()
            || self.face_colour_c.is_none()
        {
            self.face_colour_a = Some(vec![0; self.num_faces as usize]);
            self.face_colour_b = Some(vec![0; self.num_faces as usize]);
            self.face_colour_c = Some(vec![0; self.num_faces as usize]);
        }

        if self.point_normal.is_none() {
            self.point_normal = Some(
                (0..self.num_points)
                    .map(|_| Some(PointNormal::default()))
                    .collect(),
            );
        }

        for f in 0..self.num_faces as usize {
            let a = self.face_vertex_a.as_ref().unwrap()[f] as usize;
            let b = self.face_vertex_b.as_ref().unwrap()[f] as usize;
            let c = self.face_vertex_c.as_ref().unwrap()[f] as usize;

            let dx_ab = self.point_x.as_ref().unwrap()[b] - self.point_x.as_ref().unwrap()[a];
            let dy_ab = self.point_y.as_ref().unwrap()[b] - self.point_y.as_ref().unwrap()[a];
            let dz_ab = self.point_z.as_ref().unwrap()[b] - self.point_z.as_ref().unwrap()[a];

            let dx_ac = self.point_x.as_ref().unwrap()[c] - self.point_x.as_ref().unwrap()[a];
            let dy_ac = self.point_y.as_ref().unwrap()[c] - self.point_y.as_ref().unwrap()[a];
            let dz_ac = self.point_z.as_ref().unwrap()[c] - self.point_z.as_ref().unwrap()[a];

            let mut nx = dy_ab * dz_ac - dy_ac * dz_ab;
            let mut ny = dz_ab * dx_ac - dz_ac * dx_ab;
            let mut nz = dx_ab * dy_ac - dx_ac * dy_ab;

            while nx > 8192 || ny > 8192 || nz > 8192 || nx < -8192 || ny < -8192 || nz < -8192 {
                nx >>= 1;
                ny >>= 1;
                nz >>= 1;
            }

            let mut length = ((nx * nx + ny * ny + nz * nz) as f64).sqrt() as i32;
            if length <= 0 {
                length = 1;
            }

            nx = (nx * 256) / length;
            ny = (ny * 256) / length;
            nz = (nz * 256) / length;

            let face_is_lit = self
                .face_render_type
                .as_ref()
                .map(|rt| (rt[f] & 0x1) == 0)
                .unwrap_or(true);

            if face_is_lit {
                let pn = self.point_normal.as_mut().unwrap();
                if let Some(n) = pn[a].as_mut() {
                    n.x += nx;
                    n.y += ny;
                    n.z += nz;
                    n.w += 1;
                }
                if let Some(n) = pn[b].as_mut() {
                    n.x += nx;
                    n.y += ny;
                    n.z += nz;
                    n.w += 1;
                }
                if let Some(n) = pn[c].as_mut() {
                    n.x += nx;
                    n.y += ny;
                    n.z += nz;
                    n.w += 1;
                }
            } else {
                let lightness = ambient + ((x * nx + y * ny + z * nz) / (scale + (scale / 2)));
                if let Some(fc) = self.face_colour.as_ref() {
                    self.face_colour_a.as_mut().unwrap()[f] = Model::get_colour(
                        fc[f],
                        lightness,
                        self.face_render_type.as_ref().unwrap()[f],
                    );
                }
            }
        }

        if do_not_share_light {
            self.light(ambient, scale, x, y, z);
        } else {
            self.shared_point_normal = Some(self.point_normal.clone().unwrap_or_else(|| {
                (0..self.num_points)
                    .map(|_| Some(PointNormal::default()))
                    .collect()
            }));
        }

        if do_not_share_light {
            self.calc_bounding_cylinder();
        } else {
            self.calc_bounding_cube();
        }
    }

    /// `light(ambient, contrast, x, y, z)` from client-ts.
    pub fn light(&mut self, ambient: i32, contrast: i32, x: i32, y: i32, z: i32) {
        for f in 0..self.num_faces as usize {
            let a = self.face_vertex_a.as_ref().unwrap()[f] as usize;
            let b = self.face_vertex_b.as_ref().unwrap()[f] as usize;
            let c = self.face_vertex_c.as_ref().unwrap()[f] as usize;

            let face_unlit = self
                .face_render_type
                .as_ref()
                .map(|rt| (rt[f] & 0x1) == 0)
                .unwrap_or(true);

            if self.face_render_type.is_none() && face_unlit {
                let colour = self.face_colour.as_ref().unwrap()[f];
                let pn = self.point_normal.as_ref().unwrap();
                let fca = self.face_colour_a.as_mut().unwrap();
                if let Some(va) = pn[a].as_ref() {
                    fca[f] = Model::get_colour(
                        colour,
                        ambient + ((x * va.x + y * va.y + z * va.z) / (contrast * va.w)),
                        0,
                    );
                }
                let fcb = self.face_colour_b.as_mut().unwrap();
                if let Some(vb) = pn[b].as_ref() {
                    fcb[f] = Model::get_colour(
                        colour,
                        ambient + ((x * vb.x + y * vb.y + z * vb.z) / (contrast * vb.w)),
                        0,
                    );
                }
                let fcc = self.face_colour_c.as_mut().unwrap();
                if let Some(vc) = pn[c].as_ref() {
                    fcc[f] = Model::get_colour(
                        colour,
                        ambient + ((x * vc.x + y * vc.y + z * vc.z) / (contrast * vc.w)),
                        0,
                    );
                }
            } else if self.face_render_type.is_some() && face_unlit {
                let colour = self.face_colour.as_ref().unwrap()[f];
                let info = self.face_render_type.as_ref().unwrap()[f];
                let pn = self.point_normal.as_ref().unwrap();
                let fca = self.face_colour_a.as_mut().unwrap();
                if let Some(va) = pn[a].as_ref() {
                    fca[f] = Model::get_colour(
                        colour,
                        ambient + ((x * va.x + y * va.y + z * va.z) / (contrast * va.w)),
                        info,
                    );
                }
                let fcb = self.face_colour_b.as_mut().unwrap();
                if let Some(vb) = pn[b].as_ref() {
                    fcb[f] = Model::get_colour(
                        colour,
                        ambient + ((x * vb.x + y * vb.y + z * vb.z) / (contrast * vb.w)),
                        info,
                    );
                }
                let fcc = self.face_colour_c.as_mut().unwrap();
                if let Some(vc) = pn[c].as_ref() {
                    fcc[f] = Model::get_colour(
                        colour,
                        ambient + ((x * vc.x + y * vc.y + z * vc.z) / (contrast * vc.w)),
                        info,
                    );
                }
            }
        }

        self.point_normal = None;
        self.shared_point_normal = None;
        self.vertex_label = None;
        self.face_label = None;

        if let Some(rt) = self.face_render_type.as_ref() {
            for &render in rt.iter().take(self.num_faces as usize) {
                if render & 0x2 == 0x2 {
                    return;
                }
            }
        }

        self.face_colour = None;
    }

    /// `Model.getColour(hsl, scalar, faceRenderType)` from client-ts.
    pub fn get_colour(hsl: i32, mut scalar: i32, face_render_type: i32) -> i32 {
        if face_render_type & 0x2 == 0x2 {
            if scalar < 0 {
                scalar = 0;
            } else if scalar > 127 {
                scalar = 127;
            }
            127 - scalar
        } else {
            scalar = (scalar * (hsl & 0x7f)) >> 7;
            if scalar < 2 {
                scalar = 2;
            } else if scalar > 126 {
                scalar = 126;
            }
            (hsl & 0xff80) + scalar
        }
    }

    /// TS `Model.pickedEntityTypecode[Model.pickedCount++] = typecode`, the
    /// single pick-append site (the worldRender AABB and the render2
    /// triangle test both funnel here). The 1000-slot array guards the write
    /// like a TS typed array; the counter always advances. `pub(crate)` so
    /// the GPU mesh emitter can share it (locs are per-face, not AABB).
    pub(crate) fn pick(pix: &mut Pix3DDraw, typecode: i32) {
        if let Some(slot) = pix
            .picked_entity_typecode
            .get_mut(pix.picked_count as usize)
        {
            *slot = typecode;
        }
        pix.picked_count += 1;
    }

    /// TS typed-array reads of `Model.vertexScreenX/Y/Z[v]`; an out-of-range
    /// vertex (a model bigger than the 4096-slot arrays) reads 0 like a TS
    /// `Int32Array` miss.
    fn vertex_screen(pix: &Pix3DDraw, v: usize) -> (i32, i32, i32) {
        (
            pix.model_scratch
                .vertex_screen_x
                .get(v)
                .copied()
                .unwrap_or(0),
            pix.model_scratch
                .vertex_screen_y
                .get(v)
                .copied()
                .unwrap_or(0),
            pix.model_scratch
                .vertex_screen_z
                .get(v)
                .copied()
                .unwrap_or(0),
        )
    }

    /// TS typed-array reads of `Model.vertexViewSpaceX/Y/Z[v]` (same OOB
    /// semantics as `vertex_screen`).
    fn vertex_view_space(pix: &Pix3DDraw, v: usize) -> (i32, i32, i32) {
        (
            pix.model_scratch
                .vertex_view_space_x
                .get(v)
                .copied()
                .unwrap_or(0),
            pix.model_scratch
                .vertex_view_space_y
                .get(v)
                .copied()
                .unwrap_or(0),
            pix.model_scratch
                .vertex_view_space_z
                .get(v)
                .copied()
                .unwrap_or(0),
        )
    }

    /// `Model.objRender(pitch, yaw, roll, eyePitch, eyeX, eyeY, eyeZ)` from
    /// client-ts: draws the model from eye-space offsets (mapview sprite
    /// overlays, entity decorations). Projection statics live on `pix`; the
    /// near plane is not guarded — TS relies on `render2`'s winding test
    /// dropping the garbage triangles.
    #[allow(clippy::too_many_arguments)]
    pub fn obj_render(
        &self,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        pitch: i32,
        yaw: i32,
        roll: i32,
        eye_pitch: i32,
        eye_x: i32,
        eye_y: i32,
        eye_z: i32,
    ) {
        let sin_table = Pix3D::sin_table();
        let cos_table = Pix3D::cos_table();
        let sin_pitch = sin_table.get(pitch as usize).copied().unwrap_or(0);
        let cos_pitch = cos_table.get(pitch as usize).copied().unwrap_or(0);
        let sin_yaw = sin_table.get(yaw as usize).copied().unwrap_or(0);
        let cos_yaw = cos_table.get(yaw as usize).copied().unwrap_or(0);
        let sin_roll = sin_table.get(roll as usize).copied().unwrap_or(0);
        let cos_roll = cos_table.get(roll as usize).copied().unwrap_or(0);
        let sin_eye_pitch = sin_table.get(eye_pitch as usize).copied().unwrap_or(0);
        let cos_eye_pitch = cos_table.get(eye_pitch as usize).copied().unwrap_or(0);

        let mid_z = (eye_y
            .wrapping_mul(sin_eye_pitch)
            .wrapping_add(eye_z.wrapping_mul(cos_eye_pitch)))
            >> 16;

        let (Some(point_x), Some(point_y), Some(point_z)) =
            (&self.point_x, &self.point_y, &self.point_z)
        else {
            return;
        };

        for v in 0..self.num_points as usize {
            let (Some(&x0), Some(&y0), Some(&z0)) =
                (point_x.get(v), point_y.get(v), point_z.get(v))
            else {
                continue;
            };
            let (mut x, mut y, mut z) = (x0, y0, z0);

            if roll != 0 {
                let tmp = (y
                    .wrapping_mul(sin_roll)
                    .wrapping_add(x.wrapping_mul(cos_roll)))
                    >> 16;
                y = (y
                    .wrapping_mul(cos_roll)
                    .wrapping_sub(x.wrapping_mul(sin_roll)))
                    >> 16;
                x = tmp;
            }

            if pitch != 0 {
                let tmp = (y
                    .wrapping_mul(cos_pitch)
                    .wrapping_sub(z.wrapping_mul(sin_pitch)))
                    >> 16;
                z = (y
                    .wrapping_mul(sin_pitch)
                    .wrapping_add(z.wrapping_mul(cos_pitch)))
                    >> 16;
                y = tmp;
            }

            if yaw != 0 {
                let tmp = (z
                    .wrapping_mul(sin_yaw)
                    .wrapping_add(x.wrapping_mul(cos_yaw)))
                    >> 16;
                z = (z
                    .wrapping_mul(cos_yaw)
                    .wrapping_sub(x.wrapping_mul(sin_yaw)))
                    >> 16;
                x = tmp;
            }

            x = x.wrapping_add(eye_x);
            y = y.wrapping_add(eye_y);
            z = z.wrapping_add(eye_z);

            let tmp = (y
                .wrapping_mul(cos_eye_pitch)
                .wrapping_sub(z.wrapping_mul(sin_eye_pitch)))
                >> 16;
            z = (y
                .wrapping_mul(sin_eye_pitch)
                .wrapping_add(z.wrapping_mul(cos_eye_pitch)))
                >> 16;
            y = tmp;

            if let Some(slot) = pix.model_scratch.vertex_screen_z.get_mut(v) {
                *slot = z.wrapping_sub(mid_z);
            }
            if let Some(slot) = pix.model_scratch.vertex_screen_x.get_mut(v) {
                // TS `((x << 9) / z) | 0` is 0 for a vertex exactly on the
                // camera plane (z == 0 → Infinity → 0); `checked_div` keeps
                // that from panicking where TS silently writes 0.
                *slot = pix.origin_x + x.wrapping_shl(9).checked_div(z).unwrap_or(0);
            }
            if let Some(slot) = pix.model_scratch.vertex_screen_y.get_mut(v) {
                *slot = pix.origin_y + y.wrapping_shl(9).checked_div(z).unwrap_or(0);
            }

            if self.num_t > 0 {
                if let Some(slot) = pix.model_scratch.vertex_view_space_x.get_mut(v) {
                    *slot = x;
                }
                if let Some(slot) = pix.model_scratch.vertex_view_space_y.get_mut(v) {
                    *slot = y;
                }
                if let Some(slot) = pix.model_scratch.vertex_view_space_z.get_mut(v) {
                    *slot = z;
                }
            }
        }

        self.render2(pix, surface, false, false, 0);
    }

    /// `Model.worldRender(yaw, sinEyePitch, cosEyePitch, sinEyeYaw,
    /// cosEyeYaw, relativeX, relativeY, relativeZ, typecode)` from client-ts,
    /// 1:1 including the `typecode > 0 && mouseCheck` pick append. The TS
    /// `Pix3D`/`Model` statics (origin, `hclip`, `trans`, the projection
    /// scratch, the mouse pick state) live on `pix`; the TS `Pix2D` statics
    /// (`maxX`/`maxY`/`sizeX`) become the bound `surface`. The viewport
    /// frustum tests use TS wrap semantics (relative coordinates and the trig
    /// values overflow i32 in the products).
    #[allow(clippy::too_many_arguments)]
    pub fn world_render(
        &self,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        yaw: i32,
        sin_eye_pitch: i32,
        cos_eye_pitch: i32,
        sin_eye_yaw: i32,
        cos_eye_yaw: i32,
        relative_x: i32,
        relative_y: i32,
        relative_z: i32,
        typecode: i32,
    ) -> Result<(), RenderLimit> {
        let z_prime = (relative_z
            .wrapping_mul(cos_eye_yaw)
            .wrapping_sub(relative_x.wrapping_mul(sin_eye_yaw)))
            >> 16;
        let mid_z = relative_y
            .wrapping_mul(sin_eye_pitch)
            .wrapping_add(z_prime.wrapping_mul(cos_eye_pitch))
            >> 16;
        let radius_cos_eye_pitch = (self.radius.wrapping_mul(cos_eye_pitch)) >> 16;

        let max_z = mid_z + radius_cos_eye_pitch;
        if max_z <= 50 || mid_z >= 3500 {
            #[cfg(feature = "render-diagnostics")]
            crate::render::diagnostics::paint(typecode, "near-plane", mid_z, max_z);
            return Ok(());
        }

        let mid_x = (relative_z
            .wrapping_mul(sin_eye_yaw)
            .wrapping_add(relative_x.wrapping_mul(cos_eye_yaw)))
            >> 16;
        let mut left_x = (mid_x - self.radius) << 9;
        if left_x.wrapping_div(max_z) >= surface.max_x {
            #[cfg(feature = "render-diagnostics")]
            crate::render::diagnostics::paint(typecode, "frustum-left", mid_z, max_z);
            return Ok(());
        }

        let mut right_x = (mid_x + self.radius) << 9;
        if right_x.wrapping_div(max_z) <= -surface.max_x {
            #[cfg(feature = "render-diagnostics")]
            crate::render::diagnostics::paint(typecode, "frustum-right", mid_z, max_z);
            return Ok(());
        }

        let mid_y = relative_y
            .wrapping_mul(cos_eye_pitch)
            .wrapping_sub(z_prime.wrapping_mul(sin_eye_pitch))
            >> 16;
        let radius_sin_eye_pitch = (self.radius.wrapping_mul(sin_eye_pitch)) >> 16;

        let mut bottom_y = (mid_y + radius_sin_eye_pitch) << 9;
        if bottom_y.wrapping_div(max_z) <= -surface.max_y {
            #[cfg(feature = "render-diagnostics")]
            crate::render::diagnostics::paint(typecode, "frustum-bottom", mid_z, max_z);
            return Ok(());
        }

        let y_prime = radius_sin_eye_pitch + ((self.min_y.wrapping_mul(cos_eye_pitch)) >> 16);
        let mut top_y = (mid_y - y_prime) << 9;
        if top_y.wrapping_div(max_z) >= surface.max_y {
            #[cfg(feature = "render-diagnostics")]
            crate::render::diagnostics::paint(typecode, "frustum-top", mid_z, max_z);
            return Ok(());
        }

        let radius_z = radius_cos_eye_pitch + ((self.min_y.wrapping_mul(sin_eye_pitch)) >> 16);

        #[cfg(feature = "render-diagnostics")]
        crate::render::diagnostics::paint(typecode, "accepted", mid_z, max_z);

        let mut clipped = mid_z - radius_z <= 50;
        let mut picking = false;

        if typecode > 0 && pix.mouse_check {
            let mut z = mid_z - radius_cos_eye_pitch;
            if z <= 50 {
                z = 50;
            }

            if mid_x > 0 {
                left_x = left_x.wrapping_div(max_z);
                right_x = right_x.wrapping_div(z);
            } else {
                right_x = right_x.wrapping_div(max_z);
                left_x = left_x.wrapping_div(z);
            }

            if mid_y > 0 {
                top_y = top_y.wrapping_div(max_z);
                bottom_y = bottom_y.wrapping_div(z);
            } else {
                bottom_y = bottom_y.wrapping_div(max_z);
                top_y = top_y.wrapping_div(z);
            }

            let mouse_x = pix.mouse_x - pix.origin_x;
            let mouse_y = pix.mouse_y - pix.origin_y;
            if mouse_x > left_x && mouse_x < right_x && mouse_y > top_y && mouse_y < bottom_y {
                if self.use_aabb_mouse_check {
                    Self::pick(pix, typecode);
                } else {
                    picking = true;
                }
            }
        }

        let center_x = pix.origin_x;
        let center_y = pix.origin_y;

        let mut sin_yaw = 0;
        let mut cos_yaw = 0;
        if yaw != 0 {
            sin_yaw = Pix3D::sin_table().get(yaw as usize).copied().unwrap_or(0);
            cos_yaw = Pix3D::cos_table().get(yaw as usize).copied().unwrap_or(0);
        }

        let (Some(point_x), Some(point_y), Some(point_z)) =
            (&self.point_x, &self.point_y, &self.point_z)
        else {
            return Ok(());
        };

        for v in 0..self.num_points as usize {
            let (Some(&x0), Some(&y0), Some(&z0)) =
                (point_x.get(v), point_y.get(v), point_z.get(v))
            else {
                continue;
            };
            let (mut x, mut y, mut z) = (x0, y0, z0);

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

            x = x.wrapping_add(relative_x);
            y = y.wrapping_add(relative_y);
            z = z.wrapping_add(relative_z);

            let temp = (z
                .wrapping_mul(sin_eye_yaw)
                .wrapping_add(x.wrapping_mul(cos_eye_yaw)))
                >> 16;
            z = (z
                .wrapping_mul(cos_eye_yaw)
                .wrapping_sub(x.wrapping_mul(sin_eye_yaw)))
                >> 16;
            x = temp;

            let temp = (y
                .wrapping_mul(cos_eye_pitch)
                .wrapping_sub(z.wrapping_mul(sin_eye_pitch)))
                >> 16;
            z = (y
                .wrapping_mul(sin_eye_pitch)
                .wrapping_add(z.wrapping_mul(cos_eye_pitch)))
                >> 16;
            y = temp;

            if let Some(slot) = pix.model_scratch.vertex_screen_z.get_mut(v) {
                *slot = z.wrapping_sub(mid_z);
            }

            if z >= 50 {
                if let Some(slot) = pix.model_scratch.vertex_screen_x.get_mut(v) {
                    *slot = center_x + x.wrapping_shl(9).wrapping_div(z);
                }
                if let Some(slot) = pix.model_scratch.vertex_screen_y.get_mut(v) {
                    *slot = center_y + y.wrapping_shl(9).wrapping_div(z);
                }
            } else {
                if let Some(slot) = pix.model_scratch.vertex_screen_x.get_mut(v) {
                    *slot = -5000;
                }
                clipped = true;
            }

            if clipped || self.num_t > 0 || pix.capture.is_some() {
                if let Some(slot) = pix.model_scratch.vertex_view_space_x.get_mut(v) {
                    *slot = x;
                }
                if let Some(slot) = pix.model_scratch.vertex_view_space_y.get_mut(v) {
                    *slot = y;
                }
                if let Some(slot) = pix.model_scratch.vertex_view_space_z.get_mut(v) {
                    *slot = z;
                }
            }
        }

        self.render2(pix, surface, clipped, picking, typecode)?;
        Ok(())
    }

    /// `Model.render2(clipped, picking, typecode)` from client-ts: bucket the
    /// faces by depth and draw them back-to-front (priority-ordered when the
    /// model has per-face priorities). The depth/priority scratch lives on
    /// `pix`; out-of-range buckets are guarded no-ops like TS typed arrays.
    fn render2(
        &self,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        clipped: bool,
        mut picking: bool,
        typecode: i32,
    ) -> Result<(), RenderLimit> {
        for depth in 0..self.max_depth as usize {
            if let Some(count) = pix.model_scratch.tmp_depth_face_count.get_mut(depth) {
                *count = 0;
            }
        }

        let (Some(face_vertex_a), Some(face_vertex_b), Some(face_vertex_c)) = (
            &self.face_vertex_a,
            &self.face_vertex_b,
            &self.face_vertex_c,
        ) else {
            return Ok(());
        };

        for f in 0..self.num_faces as usize {
            if let Some(render_type) = &self.face_render_type {
                if render_type.get(f).copied().unwrap_or(0) == -1 {
                    continue;
                }
            }

            let (Some(&a), Some(&b), Some(&c)) = (
                face_vertex_a.get(f),
                face_vertex_b.get(f),
                face_vertex_c.get(f),
            ) else {
                continue;
            };
            let (a, b, c) = (a as usize, b as usize, c as usize);

            let (x_a, y_a, z_a) = Self::vertex_screen(pix, a);
            let (x_b, y_b, z_b) = Self::vertex_screen(pix, b);
            let (x_c, y_c, z_c) = Self::vertex_screen(pix, c);

            if clipped && (x_a == -5000 || x_b == -5000 || x_c == -5000) {
                if let Some(slot) = pix.model_scratch.face_near_clipped.get_mut(f) {
                    *slot = true;
                }

                let depth_average =
                    ((z_a as i64 + z_b as i64 + z_c as i64) / 3) as i32 + self.min_depth;
                if let Some(count) = pix
                    .model_scratch
                    .tmp_depth_face_count
                    .get_mut(depth_average as usize)
                {
                    let index = *count as usize;
                    *count += 1;
                    // The count advances past the 512-slot row (TS
                    // `tmpDepthFaceCount[depthAverage]++` always runs) but
                    // the face write is dropped out of range like a TS
                    // typed-array write, so it cannot spill into the next
                    // depth bucket's row.
                    if index < 512 {
                        if let Some(slot) = pix
                            .model_scratch
                            .tmp_depth_faces
                            .get_mut(depth_average as usize * 512 + index)
                        {
                            *slot = f as i32;
                        }
                    }
                }
            } else {
                if picking
                    && self.is_mouse_roughly_inside_triangle(
                        pix.mouse_x,
                        pix.mouse_y,
                        y_a,
                        y_b,
                        y_c,
                        x_a,
                        x_b,
                        x_c,
                    )
                {
                    Self::pick(pix, typecode);
                    picking = false;
                }

                let dx_ab = x_a - x_b;
                let dy_ab = y_a - y_b;
                let dx_cb = x_c - x_b;
                let dy_cb = y_c - y_b;

                // Java `int` wrap, not i64/TS doubles: near faces overflow
                // i32 and the wrap is what draws the fence over the hill.
                if crate::dash3d::wrapping_cross(dx_ab, dy_cb, dy_ab, dx_cb) <= 0 {
                    continue;
                }

                if let Some(slot) = pix.model_scratch.face_near_clipped.get_mut(f) {
                    *slot = false;
                }
                let face_clipped_x = x_a < 0
                    || x_b < 0
                    || x_c < 0
                    || x_a > surface.size_x
                    || x_b > surface.size_x
                    || x_c > surface.size_x;
                if let Some(slot) = pix.model_scratch.face_clipped_x.get_mut(f) {
                    *slot = face_clipped_x;
                }

                let depth_average =
                    ((z_a as i64 + z_b as i64 + z_c as i64) / 3) as i32 + self.min_depth;
                if let Some(count) = pix
                    .model_scratch
                    .tmp_depth_face_count
                    .get_mut(depth_average as usize)
                {
                    let index = *count as usize;
                    *count += 1;
                    if index < 512 {
                        if let Some(slot) = pix
                            .model_scratch
                            .tmp_depth_faces
                            .get_mut(depth_average as usize * 512 + index)
                        {
                            *slot = f as i32;
                        }
                    }
                }
            }
        }

        if self.face_priority.is_none() {
            for depth in (0..self.max_depth as usize).rev() {
                let count = pix
                    .model_scratch
                    .tmp_depth_face_count
                    .get(depth)
                    .copied()
                    .unwrap_or(0);
                if count <= 0 {
                    continue;
                }

                // A bucket count past 512 has no stored faces (the writes are
                // dropped out of range); TS reads `undefined` and skips them.
                let base = depth * 512;
                for i in 0..count.min(512) as usize {
                    let face = pix
                        .model_scratch
                        .tmp_depth_faces
                        .get(base + i)
                        .copied()
                        .unwrap_or(0) as usize;
                    self.render3(pix, surface, face)?;
                }
            }

            return Ok(());
        }

        for priority in 0..12 {
            if let Some(slot) = pix.model_scratch.tmp_priority_face_count.get_mut(priority) {
                *slot = 0;
            }
            if let Some(slot) = pix.model_scratch.tmp_priority_depth_sum.get_mut(priority) {
                *slot = 0;
            }
        }

        let face_priority = self.face_priority.as_deref().unwrap_or(&[]);

        for depth in (0..self.max_depth as usize).rev() {
            let face_count = pix
                .model_scratch
                .tmp_depth_face_count
                .get(depth)
                .copied()
                .unwrap_or(0);
            if face_count > 0 {
                let base = depth * 512;
                for i in 0..face_count.min(512) as usize {
                    let priority_depth = pix
                        .model_scratch
                        .tmp_depth_faces
                        .get(base + i)
                        .copied()
                        .unwrap_or(0) as usize;
                    let priority_face = face_priority.get(priority_depth).copied().unwrap_or(0);

                    if let Some(count) = pix
                        .model_scratch
                        .tmp_priority_face_count
                        .get_mut(priority_face as usize)
                    {
                        let index = *count as usize;
                        *count += 1;
                        // The count advances past the 2000-slot row (TS
                        // `tmpPriorityFaceCount[priorityFace]++` always runs)
                        // but the face write is dropped out of range like a
                        // TS typed-array write, so it cannot spill into the
                        // next priority bucket's row.
                        if index < 2000 {
                            if let Some(slot) = pix
                                .model_scratch
                                .tmp_priority_faces
                                .get_mut(priority_face as usize * 2000 + index)
                            {
                                *slot = priority_depth as i32;
                            }
                        }

                        if priority_face < 10 {
                            if let Some(sum) = pix
                                .model_scratch
                                .tmp_priority_depth_sum
                                .get_mut(priority_face as usize)
                            {
                                *sum += depth as i32;
                            }
                        } else if priority_face == 10 {
                            if let Some(slot) =
                                pix.model_scratch.tmp_priority10_face_depth.get_mut(index)
                            {
                                *slot = depth as i32;
                            }
                        } else if let Some(slot) =
                            pix.model_scratch.tmp_priority11_face_depth.get_mut(index)
                        {
                            *slot = depth as i32;
                        }
                    }
                }
            }
        }

        let mut average_priority_depth_sum1_2 = 0;
        let count1 = pix
            .model_scratch
            .tmp_priority_face_count
            .get(1)
            .copied()
            .unwrap_or(0);
        let count2 = pix
            .model_scratch
            .tmp_priority_face_count
            .get(2)
            .copied()
            .unwrap_or(0);
        if count1 > 0 || count2 > 0 {
            let sum1 = pix
                .model_scratch
                .tmp_priority_depth_sum
                .get(1)
                .copied()
                .unwrap_or(0);
            let sum2 = pix
                .model_scratch
                .tmp_priority_depth_sum
                .get(2)
                .copied()
                .unwrap_or(0);
            average_priority_depth_sum1_2 =
                ((sum1 + sum2) as i64 / (count1 + count2) as i64) as i32;
        }

        let mut average_priority_depth_sum3_4 = 0;
        let count3 = pix
            .model_scratch
            .tmp_priority_face_count
            .get(3)
            .copied()
            .unwrap_or(0);
        let count4 = pix
            .model_scratch
            .tmp_priority_face_count
            .get(4)
            .copied()
            .unwrap_or(0);
        if count3 > 0 || count4 > 0 {
            let sum3 = pix
                .model_scratch
                .tmp_priority_depth_sum
                .get(3)
                .copied()
                .unwrap_or(0);
            let sum4 = pix
                .model_scratch
                .tmp_priority_depth_sum
                .get(4)
                .copied()
                .unwrap_or(0);
            average_priority_depth_sum3_4 =
                ((sum3 + sum4) as i64 / (count3 + count4) as i64) as i32;
        }

        let mut average_priority_depth_sum6_8 = 0;
        let count6 = pix
            .model_scratch
            .tmp_priority_face_count
            .get(6)
            .copied()
            .unwrap_or(0);
        let count8 = pix
            .model_scratch
            .tmp_priority_face_count
            .get(8)
            .copied()
            .unwrap_or(0);
        if count6 > 0 || count8 > 0 {
            let sum6 = pix
                .model_scratch
                .tmp_priority_depth_sum
                .get(6)
                .copied()
                .unwrap_or(0);
            let sum8 = pix
                .model_scratch
                .tmp_priority_depth_sum
                .get(8)
                .copied()
                .unwrap_or(0);
            average_priority_depth_sum6_8 =
                ((sum6 + sum8) as i64 / (count6 + count8) as i64) as i32;
        }

        let mut priority_face = 0usize;
        let mut priority_face_count = pix
            .model_scratch
            .tmp_priority_face_count
            .get(10)
            .copied()
            .unwrap_or(0);
        let mut on_11 = false;
        let mut priority_depth;
        (priority_face, priority_face_count, on_11, priority_depth) =
            Self::advance_priority(pix, priority_face, priority_face_count, on_11);

        for priority in 0..10 {
            while priority == 0 && priority_depth > average_priority_depth_sum1_2 {
                self.render_priority_face(
                    pix,
                    surface,
                    &mut priority_face,
                    &mut priority_face_count,
                    &mut on_11,
                    &mut priority_depth,
                )?;
            }

            while priority == 3 && priority_depth > average_priority_depth_sum3_4 {
                self.render_priority_face(
                    pix,
                    surface,
                    &mut priority_face,
                    &mut priority_face_count,
                    &mut on_11,
                    &mut priority_depth,
                )?;
            }

            while priority == 5 && priority_depth > average_priority_depth_sum6_8 {
                self.render_priority_face(
                    pix,
                    surface,
                    &mut priority_face,
                    &mut priority_face_count,
                    &mut on_11,
                    &mut priority_depth,
                )?;
            }

            let count = pix
                .model_scratch
                .tmp_priority_face_count
                .get(priority as usize)
                .copied()
                .unwrap_or(0);
            if count > 0 {
                let base = priority as usize * 2000;
                for i in 0..count.min(2000) as usize {
                    let face = pix
                        .model_scratch
                        .tmp_priority_faces
                        .get(base + i)
                        .copied()
                        .unwrap_or(0) as usize;
                    self.render3(pix, surface, face)?;
                }
            }
        }

        while priority_depth != -1000 {
            self.render_priority_face(
                pix,
                surface,
                &mut priority_face,
                &mut priority_face_count,
                &mut on_11,
                &mut priority_depth,
            )?;
        }
        Ok(())
    }

    /// One `render3` step of the render2 priority merge: draw
    /// `tmpPriorityFaces[10|11][priorityFace++]`, roll bucket 10 over to 11
    /// when exhausted, and re-read `priorityDepth` (the TS inline block
    /// repeated in every merge loop). The cursor state is threaded back
    /// through the `&mut` arguments so the caller's merge loop sees the
    /// bucket-11 rollover.
    #[allow(clippy::too_many_arguments)]
    fn render_priority_face(
        &self,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        priority_face: &mut usize,
        priority_face_count: &mut i32,
        on_11: &mut bool,
        priority_depth: &mut i32,
    ) -> Result<(), RenderLimit> {
        let index = *priority_face;
        *priority_face += 1;
        // Past the 2000-slot row the TS read is `undefined` and render3
        // throws (swallowed by the merge loop's try/catch): skip the face
        // instead of reading the next bucket's row.
        if index < 2000 {
            let face = pix
                .model_scratch
                .tmp_priority_faces
                .get((if *on_11 { 11 } else { 10 }) * 2000 + index)
                .copied()
                .unwrap_or(0) as usize;
            self.render3(pix, surface, face)?;
        }

        let (new_face, new_count, new_on_11, new_depth) =
            Self::advance_priority(pix, *priority_face, *priority_face_count, *on_11);
        *priority_face = new_face;
        *priority_face_count = new_count;
        *on_11 = new_on_11;
        *priority_depth = new_depth;
        Ok(())
    }

    /// TS `Model.tmpPriorityFaceCount[10|11]` rollover + the
    /// `priorityFaceDepths[priorityFace]` re-read that follows every merge
    /// step in render2 (returns the new face/depth cursor state).
    #[allow(clippy::type_complexity)]
    fn advance_priority(
        pix: &Pix3DDraw,
        priority_face: usize,
        priority_face_count: i32,
        on_11: bool,
    ) -> (usize, i32, bool, i32) {
        let (priority_face, priority_face_count, on_11) =
            if (priority_face as i32) == priority_face_count && !on_11 {
                (
                    0,
                    pix.model_scratch
                        .tmp_priority_face_count
                        .get(11)
                        .copied()
                        .unwrap_or(0),
                    true,
                )
            } else {
                (priority_face, priority_face_count, on_11)
            };

        let priority_depth = if (priority_face as i32) < priority_face_count {
            if on_11 {
                pix.model_scratch
                    .tmp_priority11_face_depth
                    .get(priority_face)
                    .copied()
                    .unwrap_or(0)
            } else {
                pix.model_scratch
                    .tmp_priority10_face_depth
                    .get(priority_face)
                    .copied()
                    .unwrap_or(0)
            }
        } else {
            -1000
        };

        (priority_face, priority_face_count, on_11, priority_depth)
    }

    /// `Model.render3(face)` from client-ts: draw one bucketed face, setting
    /// `hclip`/`trans` from the per-face flags and alpha, then dispatch on
    /// `faceRenderType & 0x3` to gouraud/flat/texture. Missing colour or
    /// texture arrays skip the face (the TS `!` asserts would throw and be
    /// swallowed by `render2`'s try/catch).
    fn render3(
        &self,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        face: usize,
    ) -> Result<(), RenderLimit> {
        if pix.capture.is_some() {
            self.capture_face(pix, face)?;
            return Ok(());
        }
        if pix
            .model_scratch
            .face_near_clipped
            .get(face)
            .copied()
            .unwrap_or(false)
        {
            self.render3_z_clip(pix, surface, face);
            return Ok(());
        }

        let (Some(face_vertex_a), Some(face_vertex_b), Some(face_vertex_c)) = (
            &self.face_vertex_a,
            &self.face_vertex_b,
            &self.face_vertex_c,
        ) else {
            return Ok(());
        };
        let (Some(&a), Some(&b), Some(&c)) = (
            face_vertex_a.get(face),
            face_vertex_b.get(face),
            face_vertex_c.get(face),
        ) else {
            return Ok(());
        };
        let (a, b, c) = (a as usize, b as usize, c as usize);

        pix.hclip = pix
            .model_scratch
            .face_clipped_x
            .get(face)
            .copied()
            .unwrap_or(false);

        pix.trans = match &self.face_alpha {
            Some(alpha) => alpha.get(face).copied().unwrap_or(0),
            None => 0,
        };

        let render_type = self
            .face_render_type
            .as_ref()
            .and_then(|rt| rt.get(face))
            .copied()
            .unwrap_or(0);
        let r#type = render_type & 0x3;

        let (x_a, y_a, _) = Self::vertex_screen(pix, a);
        let (x_b, y_b, _) = Self::vertex_screen(pix, b);
        let (x_c, y_c, _) = Self::vertex_screen(pix, c);

        if r#type == 0 {
            let (Some(face_colour_a), Some(face_colour_b), Some(face_colour_c)) = (
                &self.face_colour_a,
                &self.face_colour_b,
                &self.face_colour_c,
            ) else {
                return Ok(());
            };
            let (Some(&shade_a), Some(&shade_b), Some(&shade_c)) = (
                face_colour_a.get(face),
                face_colour_b.get(face),
                face_colour_c.get(face),
            ) else {
                return Ok(());
            };
            self.render_triangle(
                pix,
                surface,
                r#type,
                render_type,
                face,
                x_a,
                x_b,
                x_c,
                y_a,
                y_b,
                y_c,
                shade_a,
                shade_b,
                shade_c,
            );
        } else if r#type == 1 {
            self.render_triangle(
                pix,
                surface,
                r#type,
                render_type,
                face,
                x_a,
                x_b,
                x_c,
                y_a,
                y_b,
                y_c,
                0,
                0,
                0,
            );
        } else {
            let (Some(face_colour_a), Some(face_colour_b), Some(face_colour_c)) = (
                &self.face_colour_a,
                &self.face_colour_b,
                &self.face_colour_c,
            ) else {
                return Ok(());
            };
            let (Some(&shade_a), Some(&shade_b), Some(&shade_c)) = (
                face_colour_a.get(face),
                face_colour_b.get(face),
                face_colour_c.get(face),
            ) else {
                return Ok(());
            };
            self.render_triangle(
                pix,
                surface,
                r#type,
                render_type,
                face,
                x_a,
                x_b,
                x_c,
                y_a,
                y_b,
                y_c,
                shade_a,
                shade_b,
                shade_c,
            );
        }
        Ok(())
    }

    /// The wgpu capture of one `render3` face: the face's view-space
    /// vertices (written by `world_render` whenever a capture is bound)
    /// go to the scene mesh at this point of the painter order. The
    /// near-plane clip and its winding test happen in the capture, the
    /// way `render3ZClip` does them for the rasterizer.
    fn capture_face(&self, pix: &mut Pix3DDraw, face: usize) -> Result<(), RenderLimit> {
        let (Some(face_vertex_a), Some(face_vertex_b), Some(face_vertex_c)) = (
            &self.face_vertex_a,
            &self.face_vertex_b,
            &self.face_vertex_c,
        ) else {
            return Ok(());
        };
        let (Some(&a), Some(&b), Some(&c)) = (
            face_vertex_a.get(face),
            face_vertex_b.get(face),
            face_vertex_c.get(face),
        ) else {
            return Ok(());
        };
        let view = [
            Self::vertex_view_space(pix, a as usize),
            Self::vertex_view_space(pix, b as usize),
            Self::vertex_view_space(pix, c as usize),
        ];
        crate::render::world::capture_model_face(self, face, [a, b, c], view, pix)?;
        Ok(())
    }

    /// Shared face raster used by `render3` and `render3ZClip`: dispatch on
    /// `faceRenderType & 0x3` to gouraud/flat/texture. Type 0 and 2 use the
    /// given per-vertex shades; type 1 and 3 re-read `faceColourA[face]`
    /// exactly as TS does in both call sites.
    #[allow(clippy::too_many_arguments)]
    fn render_triangle(
        &self,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        r#type: i32,
        render_type: i32,
        face: usize,
        x_a: i32,
        x_b: i32,
        x_c: i32,
        y_a: i32,
        y_b: i32,
        y_c: i32,
        shade_a: i32,
        shade_b: i32,
        shade_c: i32,
    ) {
        if r#type == 0 {
            pix.gouraud_triangle(
                surface, x_a, x_b, x_c, y_a, y_b, y_c, shade_a, shade_b, shade_c,
            );
        } else if r#type == 1 {
            let Some(face_colour_a) = &self.face_colour_a else {
                return;
            };
            let Some(&shade) = face_colour_a.get(face) else {
                return;
            };
            let colour = Pix3D::colour_table()
                .get(shade as usize)
                .copied()
                .unwrap_or(0);
            pix.flat_triangle(surface, x_a, x_b, x_c, y_a, y_b, y_c, colour);
        } else {
            let textured_face = (render_type >> 2) as usize;
            let (Some(texture_p), Some(texture_m), Some(texture_n)) = (
                &self.face_texture_p,
                &self.face_texture_m,
                &self.face_texture_n,
            ) else {
                return;
            };
            let (Some(&t_a), Some(&t_b), Some(&t_c)) = (
                texture_p.get(textured_face),
                texture_m.get(textured_face),
                texture_n.get(textured_face),
            ) else {
                return;
            };
            let Some(face_colour) = &self.face_colour else {
                return;
            };
            let Some(&texture) = face_colour.get(face) else {
                return;
            };

            let (origin_x, origin_y, origin_z) = Self::vertex_view_space(pix, t_a as usize);
            let (tx_b, ty_b, tz_b) = Self::vertex_view_space(pix, t_b as usize);
            let (tx_c, ty_c, tz_c) = Self::vertex_view_space(pix, t_c as usize);

            let (shade_a, shade_b, shade_c) = if r#type == 3 {
                let Some(face_colour_a) = &self.face_colour_a else {
                    return;
                };
                let Some(&shade) = face_colour_a.get(face) else {
                    return;
                };
                (shade, shade, shade)
            } else {
                (shade_a, shade_b, shade_c)
            };

            pix.texture_triangle(
                surface, x_a, x_b, x_c, y_a, y_b, y_c, shade_a, shade_b, shade_c, origin_x,
                origin_y, origin_z, tx_b, tx_c, ty_b, ty_c, tz_b, tz_c, texture,
            );
        }
    }

    /// `Model.render3ZClip(face)` from client-ts: near-plane clip of a face
    /// with an off-screen (`-5000`) vertex into a 3- or 4-vertex polygon,
    /// then the same type dispatch as `render3`. The view-space scratch is
    /// written by `worldRender` whenever the frame `clipped` (or the model
    /// is textured).
    fn render3_z_clip(&self, pix: &mut Pix3DDraw, surface: &mut Pix2D, face: usize) {
        let mut elements = 0usize;

        let center_x = pix.origin_x;
        let center_y = pix.origin_y;

        let (Some(face_vertex_a), Some(face_vertex_b), Some(face_vertex_c)) = (
            &self.face_vertex_a,
            &self.face_vertex_b,
            &self.face_vertex_c,
        ) else {
            return;
        };
        let (Some(&a), Some(&b), Some(&c)) = (
            face_vertex_a.get(face),
            face_vertex_b.get(face),
            face_vertex_c.get(face),
        ) else {
            return;
        };
        let (a, b, c) = (a as usize, b as usize, c as usize);

        let (_, _, z_a) = Self::vertex_view_space(pix, a);
        let (_, _, z_b) = Self::vertex_view_space(pix, b);
        let (_, _, z_c) = Self::vertex_view_space(pix, c);

        let Some(face_colour_a) = &self.face_colour_a else {
            return;
        };
        let Some(face_colour_b) = &self.face_colour_b else {
            return;
        };
        let Some(face_colour_c) = &self.face_colour_c else {
            return;
        };

        if z_a >= 50 {
            let (x, y, _) = Self::vertex_screen(pix, a);
            Self::clipped_push(
                pix,
                &mut elements,
                x,
                y,
                face_colour_a.get(face).copied().unwrap_or(0),
            );
        } else {
            let (x_a, y_a, _) = Self::vertex_view_space(pix, a);
            let colour_a = face_colour_a.get(face).copied().unwrap_or(0);

            if z_c >= 50 {
                let scalar = (50 - z_a).wrapping_mul(
                    Pix3D::div_table2()
                        .get((z_c - z_a) as usize)
                        .copied()
                        .unwrap_or(0),
                );
                let (x_c, y_c, _) = Self::vertex_view_space(pix, c);
                let colour_c = face_colour_c.get(face).copied().unwrap_or(0);
                Self::clipped_push(
                    pix,
                    &mut elements,
                    center_x
                        + x_a
                            .wrapping_add((x_c - x_a).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    center_y
                        + y_a
                            .wrapping_add((y_c - y_a).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    colour_a.wrapping_add((colour_c - colour_a).wrapping_mul(scalar) >> 16),
                );
            }

            if z_b >= 50 {
                let scalar = (50 - z_a).wrapping_mul(
                    Pix3D::div_table2()
                        .get((z_b - z_a) as usize)
                        .copied()
                        .unwrap_or(0),
                );
                let (x_b, y_b, _) = Self::vertex_view_space(pix, b);
                let colour_b = face_colour_b.get(face).copied().unwrap_or(0);
                Self::clipped_push(
                    pix,
                    &mut elements,
                    center_x
                        + x_a
                            .wrapping_add((x_b - x_a).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    center_y
                        + y_a
                            .wrapping_add((y_b - y_a).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    colour_a.wrapping_add((colour_b - colour_a).wrapping_mul(scalar) >> 16),
                );
            }
        }

        if z_b >= 50 {
            let (x, y, _) = Self::vertex_screen(pix, b);
            Self::clipped_push(
                pix,
                &mut elements,
                x,
                y,
                face_colour_b.get(face).copied().unwrap_or(0),
            );
        } else {
            let (x_b, y_b, _) = Self::vertex_view_space(pix, b);
            let colour_b = face_colour_b.get(face).copied().unwrap_or(0);

            if z_a >= 50 {
                let scalar = (50 - z_b).wrapping_mul(
                    Pix3D::div_table2()
                        .get((z_a - z_b) as usize)
                        .copied()
                        .unwrap_or(0),
                );
                let (x_a, y_a, _) = Self::vertex_view_space(pix, a);
                let colour_a = face_colour_a.get(face).copied().unwrap_or(0);
                Self::clipped_push(
                    pix,
                    &mut elements,
                    center_x
                        + x_b
                            .wrapping_add((x_a - x_b).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    center_y
                        + y_b
                            .wrapping_add((y_a - y_b).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    colour_b.wrapping_add((colour_a - colour_b).wrapping_mul(scalar) >> 16),
                );
            }

            if z_c >= 50 {
                let scalar = (50 - z_b).wrapping_mul(
                    Pix3D::div_table2()
                        .get((z_c - z_b) as usize)
                        .copied()
                        .unwrap_or(0),
                );
                let (x_c, y_c, _) = Self::vertex_view_space(pix, c);
                let colour_c = face_colour_c.get(face).copied().unwrap_or(0);
                Self::clipped_push(
                    pix,
                    &mut elements,
                    center_x
                        + x_b
                            .wrapping_add((x_c - x_b).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    center_y
                        + y_b
                            .wrapping_add((y_c - y_b).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    colour_b.wrapping_add((colour_c - colour_b).wrapping_mul(scalar) >> 16),
                );
            }
        }

        if z_c >= 50 {
            let (x, y, _) = Self::vertex_screen(pix, c);
            Self::clipped_push(
                pix,
                &mut elements,
                x,
                y,
                face_colour_c.get(face).copied().unwrap_or(0),
            );
        } else {
            let (x_c, y_c, _) = Self::vertex_view_space(pix, c);
            let colour_c = face_colour_c.get(face).copied().unwrap_or(0);

            if z_b >= 50 {
                let scalar = (50 - z_c).wrapping_mul(
                    Pix3D::div_table2()
                        .get((z_b - z_c) as usize)
                        .copied()
                        .unwrap_or(0),
                );
                let (x_b, y_b, _) = Self::vertex_view_space(pix, b);
                let colour_b = face_colour_b.get(face).copied().unwrap_or(0);
                Self::clipped_push(
                    pix,
                    &mut elements,
                    center_x
                        + x_c
                            .wrapping_add((x_b - x_c).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    center_y
                        + y_c
                            .wrapping_add((y_b - y_c).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    colour_c.wrapping_add((colour_b - colour_c).wrapping_mul(scalar) >> 16),
                );
            }

            if z_a >= 50 {
                let scalar = (50 - z_c).wrapping_mul(
                    Pix3D::div_table2()
                        .get((z_a - z_c) as usize)
                        .copied()
                        .unwrap_or(0),
                );
                let (x_a, y_a, _) = Self::vertex_view_space(pix, a);
                let colour_a = face_colour_a.get(face).copied().unwrap_or(0);
                Self::clipped_push(
                    pix,
                    &mut elements,
                    center_x
                        + x_c
                            .wrapping_add((x_a - x_c).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    center_y
                        + y_c
                            .wrapping_add((y_a - y_c).wrapping_mul(scalar) >> 16)
                            .wrapping_shl(9)
                            .wrapping_div(50),
                    colour_c.wrapping_add((colour_a - colour_c).wrapping_mul(scalar) >> 16),
                );
            }
        }

        let x0 = pix.model_scratch.clipped_x.first().copied().unwrap_or(0);
        let x1 = pix.model_scratch.clipped_x.get(1).copied().unwrap_or(0);
        let x2 = pix.model_scratch.clipped_x.get(2).copied().unwrap_or(0);
        let y0 = pix.model_scratch.clipped_y.first().copied().unwrap_or(0);
        let y1 = pix.model_scratch.clipped_y.get(1).copied().unwrap_or(0);
        let y2 = pix.model_scratch.clipped_y.get(2).copied().unwrap_or(0);

        if crate::dash3d::wrapping_cross(
            x0.wrapping_sub(x1),
            y2.wrapping_sub(y1),
            y0.wrapping_sub(y1),
            x2.wrapping_sub(x1),
        ) <= 0
        {
            return;
        }

        pix.hclip = false;

        let render_type = self
            .face_render_type
            .as_ref()
            .and_then(|rt| rt.get(face))
            .copied()
            .unwrap_or(0);
        let r#type = render_type & 0x3;

        if elements == 3 {
            if x0 < 0
                || x1 < 0
                || x2 < 0
                || x0 > surface.size_x
                || x1 > surface.size_x
                || x2 > surface.size_x
            {
                pix.hclip = true;
            }

            let c0 = pix
                .model_scratch
                .clipped_colour
                .first()
                .copied()
                .unwrap_or(0);
            let c1 = pix
                .model_scratch
                .clipped_colour
                .get(1)
                .copied()
                .unwrap_or(0);
            let c2 = pix
                .model_scratch
                .clipped_colour
                .get(2)
                .copied()
                .unwrap_or(0);
            self.render_triangle(
                pix,
                surface,
                r#type,
                render_type,
                face,
                x0,
                x1,
                x2,
                y0,
                y1,
                y2,
                c0,
                c1,
                c2,
            );
        } else if elements == 4 {
            if x0 < 0
                || x1 < 0
                || x2 < 0
                || x0 > surface.size_x
                || x1 > surface.size_x
                || x2 > surface.size_x
                || pix.model_scratch.clipped_x.get(3).copied().unwrap_or(0) < 0
                || pix.model_scratch.clipped_x.get(3).copied().unwrap_or(0) > surface.size_x
            {
                pix.hclip = true;
            }

            let x3 = pix.model_scratch.clipped_x.get(3).copied().unwrap_or(0);
            let y3 = pix.model_scratch.clipped_y.get(3).copied().unwrap_or(0);
            let c0 = pix
                .model_scratch
                .clipped_colour
                .first()
                .copied()
                .unwrap_or(0);
            let c1 = pix
                .model_scratch
                .clipped_colour
                .get(1)
                .copied()
                .unwrap_or(0);
            let c2 = pix
                .model_scratch
                .clipped_colour
                .get(2)
                .copied()
                .unwrap_or(0);
            let c3 = pix
                .model_scratch
                .clipped_colour
                .get(3)
                .copied()
                .unwrap_or(0);
            self.render_triangle(
                pix,
                surface,
                r#type,
                render_type,
                face,
                x0,
                x1,
                x2,
                y0,
                y1,
                y2,
                c0,
                c1,
                c2,
            );
            self.render_triangle(
                pix,
                surface,
                r#type,
                render_type,
                face,
                x0,
                x2,
                x3,
                y0,
                y2,
                y3,
                c0,
                c2,
                c3,
            );
        }
    }

    /// TS `Model.clippedX[elements] = x` etc. appends (10-slot typed arrays;
    /// the near clip never produces more than 4 vertices).
    fn clipped_push(pix: &mut Pix3DDraw, elements: &mut usize, x: i32, y: i32, colour: i32) {
        if let Some(slot) = pix.model_scratch.clipped_x.get_mut(*elements) {
            *slot = x;
        }
        if let Some(slot) = pix.model_scratch.clipped_y.get_mut(*elements) {
            *slot = y;
        }
        if let Some(slot) = pix.model_scratch.clipped_colour.get_mut(*elements) {
            *slot = colour;
        }
        *elements += 1;
    }

    /// `Model.isMouseRoughlyInsideTriangle(...)` from client-ts: the mouse in
    /// the projected triangle's axis-aligned bounding box (the cheap
    /// pre-test `render2` runs per picking face). The four-branch boolean
    /// chain is kept 1:1 with the TS.
    #[allow(clippy::needless_bool)]
    pub(crate) fn is_mouse_roughly_inside_triangle(
        &self,
        x: i32,
        y: i32,
        y_a: i32,
        y_b: i32,
        y_c: i32,
        x_a: i32,
        x_b: i32,
        x_c: i32,
    ) -> bool {
        if y < y_a && y < y_b && y < y_c {
            false
        } else if y > y_a && y > y_b && y > y_c {
            false
        } else if x < x_a && x < x_b && x < x_c {
            false
        } else if x > x_a && x > x_b && x > x_c {
            false
        } else {
            true
        }
    }
}
#[cfg(test)]
mod render_budget_tests {
    use super::*;
    use crate::dash3d::store::tests::CACHE_LOCK;
    use crate::render::budget::{ByteBudget, RenderResource};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    const MODEL_BYTES: &[u8] = &[
        7, 7, 7, 1, 0x40, 0x41, 0x41, 0x00, 0xFF, 0x40, 0x68, 0x18, 0x68, 0x40, 0x18, 0x40, 0x40,
        0x40, 0, 3, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 3, 0, 3, 0, 3,
    ];

    fn expect_limit<T>(result: Result<T, RenderLimit>) -> RenderLimit {
        match result {
            Err(error) => error,
            Ok(_) => panic!("expected the render byte budget to reject the operation"),
        }
    }

    #[test]
    fn render_budget_rejects_before_model_decode_clone_or_merge_allocation() {
        let _guard = CACHE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        Model::reset_for_tests();
        Model::unpack(0, Some(MODEL_BYTES));

        let measurement_budget = ByteBudget::new(RenderResource::SharedModels, usize::MAX);
        let measured = Model::try_load_shared_for_render(0, &measurement_budget)
            .unwrap()
            .unwrap();
        let shared_bytes = measured
            .retained_bytes()
            .checked_add(2 * std::mem::size_of::<usize>())
            .unwrap();
        let index_bytes = measurement_budget
            .live()
            .checked_sub(shared_bytes)
            .expect("shared model and LRU index are both charged");
        assert!(index_bytes > 0);
        drop(measured);
        Model::reset_for_tests();
        assert_eq!(measurement_budget.live(), 0);

        Model::unpack(0, Some(MODEL_BYTES));
        let limit = index_bytes.checked_add(shared_bytes).unwrap() - 1;
        let decode_budget = ByteBudget::new(RenderResource::SharedModels, limit);
        let error = expect_limit(Model::try_load_shared_for_render(0, &decode_budget));
        assert_eq!(error.resource(), RenderResource::SharedModels);
        assert_eq!(decode_budget.live(), index_bytes);
        assert_eq!(store().lock().unwrap().loaded, 0);
        assert_eq!(
            GeometryStore::instance()
                .lock()
                .unwrap()
                .model_load_count(0),
            0
        );
        Model::reset_for_tests();
        assert_eq!(decode_budget.live(), 0);

        Model::unpack(1, Some(MODEL_BYTES));
        let source = Model::load(1).unwrap();
        let loaded_before = store().lock().unwrap().loaded;
        let clone_budget = ByteBudget::new(RenderResource::TempModels, 0);
        let error = expect_limit(source.try_clone_for_render(&clone_budget));
        assert_eq!(error.resource(), RenderResource::TempModels);
        assert_eq!(clone_budget.live(), 0);

        let models = [Some(source.clone())];
        let merge_budget = ByteBudget::new(RenderResource::TempModels, 0);
        let error = expect_limit(Model::try_combine_for_anim(&models, 1, &merge_budget));
        assert_eq!(error.resource(), RenderResource::TempModels);
        assert_eq!(merge_budget.live(), 0);
        assert_eq!(store().lock().unwrap().loaded, loaded_before);
        Model::reset_for_tests();
    }

    #[test]
    fn model_render_charge_recovers_on_drop_and_unwind() {
        let _guard = CACHE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        Model::reset_for_tests();
        Model::unpack(0, Some(MODEL_BYTES));
        let source = Model::load(0).unwrap();
        let limit = source.retained_bytes().checked_mul(2).unwrap();
        let budget = ByteBudget::new(RenderResource::TempModels, limit);

        let cloned = source.try_clone_for_render(&budget).unwrap();
        assert!(budget.live() > 0);
        drop(cloned);
        assert_eq!(budget.live(), 0);

        let unwind = catch_unwind(AssertUnwindSafe(|| {
            let _cloned = source.try_clone_for_render(&budget).unwrap();
            panic!("release the model charge during unwind");
        }));
        assert!(unwind.is_err());
        assert_eq!(budget.live(), 0);
        Model::reset_for_tests();
    }

    #[test]
    fn model_retained_bytes_include_nested_label_capacities() {
        let mut vertices = Vec::with_capacity(3);
        vertices.push(Some(Vec::with_capacity(5)));
        vertices.push(None);
        vertices[0].as_mut().unwrap().extend([1, 2, 3]);

        let mut faces = Vec::with_capacity(2);
        faces.push(Some(Vec::with_capacity(4)));
        faces[0].as_mut().unwrap().extend([4, 5]);

        let vertex_bytes = vertices.capacity() * std::mem::size_of::<Option<Vec<i32>>>()
            + vertices
                .iter()
                .flatten()
                .map(|label| label.capacity() * std::mem::size_of::<i32>())
                .sum::<usize>();
        let face_bytes = faces.capacity() * std::mem::size_of::<Option<Vec<i32>>>()
            + faces
                .iter()
                .flatten()
                .map(|label| label.capacity() * std::mem::size_of::<i32>())
                .sum::<usize>();
        let mut model = Model::default();
        model.label_vertices = Some(vertices);
        model.label_faces = Some(faces);

        let expected = std::mem::size_of::<Model>() + vertex_bytes + face_bytes;
        assert_eq!(model.retained_bytes(), expected);
    }
}
