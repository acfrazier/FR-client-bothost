// Port of `~/experiments/Server/webclient/src/dash3d/Model.ts` — the model
// decode, transforms, lighting and animation machinery needed to build scene
// models (`REBUILD_NORMAL` loc placement, entities). The rasterisation half
// (`objRender`, `worldRender`, `render2`/`render3`, picking) is part of the
// deferred render pass and is not ported here.
//
// TS statics (`meta`, `provider`, `tempModel`, the scratch buffers, the
// oX/oY/oZ anim origin) live on a process-wide store. The TS `tempModel`
// singleton and shared-array `copyForAnim`/`set` aliasing are replaced with
// owned copies: single-threaded behaviour is identical and the port stays
// `Send`.
use std::sync::{Mutex, OnceLock};

use crate::dash3d::{AnimFrame, PointNormal};
use crate::datastruct::linkable::{LinkableTrait, Links};
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
        Mutex::new(ModelStore { meta: Vec::new(), provider: None, loaded: 0 })
    })
}

#[derive(Clone)]
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
    /// `Model.init(total, provider)` from client-ts.
    pub fn init(total: i32, provider: Box<dyn ModelProvider + Send>) {
        let mut s = store().lock().unwrap();
        s.meta = Vec::with_capacity(total as usize);
        s.meta.resize_with(total as usize, || None);
        s.provider = Some(provider);
    }

    /// `Model.unpack(id, src)` from client-ts; parses the 18-byte trailer and
    /// stores the section offsets so `load` can decode lazily.
    pub fn unpack(id: i32, src: Option<&[u8]>) {
        let mut s = store().lock().unwrap();
        if id as usize >= s.meta.len() {
            s.meta.resize_with(id as usize + 1, || None);
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

    /// `Model.unload(id)` from client-ts.
    pub fn unload(id: i32) {
        let mut s = store().lock().unwrap();
        if (id as usize) < s.meta.len() {
            s.meta[id as usize] = None;
        }
    }

    /// `Model.load(id)` from client-ts; decodes the model once its metadata
    /// is available, otherwise asks the provider for it and returns null.
    pub fn load(id: i32) -> Option<Model> {
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

        let mut point1 = Packet::new(src.to_vec());
        point1.pos = meta.vertex_order_offset as usize;
        let mut point2 = Packet::new(src.to_vec());
        point2.pos = meta.vertex_x_offset as usize;
        let mut point3 = Packet::new(src.to_vec());
        point3.pos = meta.vertex_y_offset as usize;
        let mut point4 = Packet::new(src.to_vec());
        point4.pos = meta.vertex_z_offset as usize;
        let mut point5 = Packet::new(src.to_vec());
        point5.pos = meta.vertex_label_offset.max(0) as usize;

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

        let mut face1 = Packet::new(src.to_vec());
        face1.pos = meta.face_colour_offset as usize;
        let mut face2 = Packet::new(src.to_vec());
        face2.pos = meta.face_render_type_offset.max(0) as usize;
        let mut face3 = Packet::new(src.to_vec());
        face3.pos = meta.face_priority_offset.max(0) as usize;
        let mut face4 = Packet::new(src.to_vec());
        face4.pos = meta.face_alpha_offset.max(0) as usize;
        let mut face5 = Packet::new(src.to_vec());
        face5.pos = meta.face_label_offset.max(0) as usize;

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

        let mut vertex1 = Packet::new(src.to_vec());
        vertex1.pos = meta.face_index_offset as usize;
        let mut vertex2 = Packet::new(src.to_vec());
        vertex2.pos = meta.face_index_order_offset as usize;

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
            let mut axis = Packet::new(src.to_vec());
            axis.pos = meta.face_texture_axis_offset as usize;
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

        model.face_colour = Some(src.face_colour.as_ref().map_or_else(Vec::new, |f| f.clone()));

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
            model.face_colour_a = Some(src.face_colour_a.as_ref().map_or_else(Vec::new, |f| f.clone()));
            model.face_colour_b = Some(src.face_colour_b.as_ref().map_or_else(Vec::new, |f| f.clone()));
            model.face_colour_c = Some(src.face_colour_c.as_ref().map_or_else(Vec::new, |f| f.clone()));

            model.face_render_type = Some(match &src.face_render_type {
                Some(rt) => rt.clone(),
                None => vec![0; src.num_faces as usize],
            });

            model.point_normal = Some(match &src.point_normal {
                Some(pn) => pn.clone(),
                None => (0..src.num_points).map(|_| Some(PointNormal::default())).collect(),
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
        self.min_depth = (((self.radius * self.radius + self.min_y * self.min_y) as f64).sqrt()
            + 0.99) as i32;
        self.max_depth = self.min_depth
            + (((self.radius * self.radius + self.max_y * self.max_y) as f64).sqrt() + 0.99)
                as i32;
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

        self.min_depth = (((self.radius * self.radius + self.min_y * self.min_y) as f64).sqrt()
            + 0.99) as i32;
        self.max_depth = self.min_depth
            + (((self.radius * self.radius + self.max_y * self.max_y) as f64).sqrt() + 0.99)
                as i32;
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
        self.min_depth = ((self.radius * self.radius + self.min_y * self.min_y) as f64).sqrt() as i32;
        self.max_depth = self.min_depth
            + ((self.radius * self.radius + self.max_y * self.max_y) as f64).sqrt() as i32;
    }

    /// `prepareAnim()` from client-ts; builds the label tables used by
    /// `animate`/`maskAnimate`.
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
        let Some(transform) = AnimFrame::get(id) else { return };
        let Some(base) = &transform.base else { return };

        let mut origin = (0i32, 0i32, 0i32);

        for i in 0..transform.size as usize {
            let Some(ti) = &transform.ti else { continue };
            let Some(tx) = &transform.tx else { continue };
            let Some(ty) = &transform.ty else { continue };
            let Some(tz) = &transform.tz else { continue };
            let Some(base_labels) = &base.labels else { continue };
            let Some(base_type) = &base.r#type else { continue };

            let ti = ti[i] as usize;
            let (Some(labels), Some(&r#type)) =
                (base_labels.get(ti).and_then(|l| l.as_deref()), base_type.get(ti))
            else {
                continue;
            };
            origin =
                self.animate2(tx[i], ty[i], tz[i], labels, r#type as i32, origin);
        }
    }

    /// `maskAnimate(primaryId, secondaryId, mask)` from client-ts.
    pub fn mask_animate(
        &mut self,
        primary_id: i32,
        secondary_id: i32,
        mask: Option<&[i32]>,
    ) {
        if primary_id == -1 {
            return;
        }

        let Some(mask) = mask else {
            self.animate(primary_id);
            return;
        };

        let Some(primary) = AnimFrame::get(primary_id) else { return };
        let Some(secondary) = AnimFrame::get(secondary_id) else {
            self.animate(primary_id);
            return;
        };

        let Some(skeleton) = &primary.base else { return };
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
            if base == mask_base || skeleton_type.get(base as usize).copied().unwrap_or(0) as i32 == 0 {
                if let Some(labels) = skeleton_labels.get(base as usize).and_then(|l| l.as_deref()) {
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
            if base == mask_base || skeleton_type.get(base as usize).copied().unwrap_or(0) as i32 == 0 {
                if let Some(labels) = skeleton_labels.get(base as usize).and_then(|l| l.as_deref()) {
                    self.animate2(
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
                let Some(lv) = self.label_vertices.as_deref() else { continue };
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
                let Some(lv) = self.label_vertices.as_deref() else { continue };
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
                        self.point_y.as_mut().unwrap()[v] =
                            (self.point_y.as_ref().unwrap()[v] * cos
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
                        self.point_z.as_mut().unwrap()[v] =
                            (self.point_y.as_ref().unwrap()[v] * sin
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
                        self.point_z.as_mut().unwrap()[v] =
                            (self.point_z.as_ref().unwrap()[v] * cos
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
                let Some(lv) = self.label_vertices.as_deref() else { continue };
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
            let Some(lf) = self.label_faces.as_deref() else { return (ox, oy, oz) };
            let Some(fa) = self.face_alpha.as_mut() else { return (ox, oy, oz) };
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
        let Some(fc) = self.face_colour.as_mut() else { return };
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

        if self.face_colour_a.is_none() || self.face_colour_b.is_none() || self.face_colour_c.is_none()
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
                    self.face_colour_a.as_mut().unwrap()[f] =
                        Model::get_colour(fc[f], lightness, self.face_render_type.as_ref().unwrap()[f]);
                }
            }
        }

        if do_not_share_light {
            self.light(ambient, scale, x, y, z);
        } else {
            self.shared_point_normal = Some(
                self.point_normal
                    .clone()
                    .unwrap_or_else(|| {
                        (0..self.num_points).map(|_| Some(PointNormal::default())).collect()
                    }),
            );
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
}
