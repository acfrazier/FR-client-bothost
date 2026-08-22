// Port of `~/experiments/Server/webclient/src/config/IfType.ts` (decode only).
// The interface `data` member lives in the `interface` jag, not `config`, so
// `unpack` of a config jag returns empty. Sprite loads (`graphic`/`graphic2`
// and the `invBackground` names are kept and depacked from the `media` jag by
// `draw_interface`); `font` keeps the font-array index for the same task.
use std::sync::{Mutex, OnceLock};

use crate::config::Cache;
use crate::dash3d::{AnimFrame, ClientPlayer, Model};
use crate::datastruct::LruCache;
use crate::io::{JagFile, Packet};

// Process-wide by design: an LRU of decoded, immutable models shared by
// every client (the TS `IfType.modelCache` static). Cache bookkeeping, not
// per-client draw state; eviction is LRU so clients only contend on the lock.
fn model_cache() -> &'static Mutex<LruCache<Model>> {
    static CACHE: OnceLock<Mutex<LruCache<Model>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(30)))
}

/// `ComponentType` const enum from client-ts.
pub struct ComponentType;

impl ComponentType {
    pub const TYPE_LAYER: i32 = 0;
    pub const TYPE_UNUSED: i32 = 1;
    pub const TYPE_INV: i32 = 2;
    pub const TYPE_RECT: i32 = 3;
    pub const TYPE_TEXT: i32 = 4;
    pub const TYPE_GRAPHIC: i32 = 5;
    pub const TYPE_MODEL: i32 = 6;
    pub const TYPE_INV_TEXT: i32 = 7;
}

/// `ButtonType` const enum from client-ts.
pub struct ButtonType;

impl ButtonType {
    pub const BUTTON_OK: i32 = 1;
    pub const BUTTON_TARGET: i32 = 2;
    pub const BUTTON_CLOSE: i32 = 3;
    pub const BUTTON_TOGGLE: i32 = 4;
    pub const BUTTON_SELECT: i32 = 5;
    pub const BUTTON_CONTINUE: i32 = 6;
}

#[derive(Clone)]
pub struct IfType {
    pub anim_frame: i32,
    pub anim_cycle: i32,
    pub id: i32,
    pub layer_id: i32,
    pub r#type: i32,
    pub button_type: i32,
    pub client_code: i32,
    pub width: i32,
    pub height: i32,
    pub trans: i32,
    pub over_layer_id: i32,
    pub x: i32,
    pub y: i32,
    pub scripts: Option<Vec<Vec<i32>>>,
    pub script_comparator: Option<Vec<i32>>,
    pub script_operand: Option<Vec<i32>>,
    pub scroll_height: i32,
    pub scroll_pos: i32,
    pub hide: bool,
    pub children: Option<Vec<i32>>,
    pub child_x: Option<Vec<i32>>,
    pub child_y: Option<Vec<i32>>,
    pub link_obj_type: Option<Vec<i32>>,
    pub link_obj_number: Option<Vec<i32>>,
    pub obj_swap: bool,
    pub obj_ops: bool,
    pub obj_use: bool,
    pub obj_replace: bool,
    pub margin_x: i32,
    pub margin_y: i32,
    pub inv_background_x: Option<Vec<i32>>,
    pub inv_background_y: Option<Vec<i32>>,
    /// The `"name,index"` gjstr of each slot-frame sprite (Java
    /// `IfType.invBackground`, unpacked from the `media` jag at decode
    /// time). The Rust unpack holds no media jag, so the names are stored
    /// and depacked on demand by `draw_interface`.
    pub inv_background_name: Option<Vec<Option<String>>>,
    pub iop: [Option<String>; 5],
    pub fill: bool,
    pub centre: bool,
    pub font: i32,
    pub shadow: bool,
    pub text: String,
    pub text2: String,
    pub colour: i32,
    pub colour2: i32,
    pub colour_over: i32,
    pub colour2_over: i32,
    pub model1_type: i32,
    pub model1_id: i32,
    pub model2_id: i32,
    pub model2_type: i32,
    pub model_anim: i32,
    pub model_anim2: i32,
    pub model_zoom: i32,
    pub model_xan: i32,
    pub model_yan: i32,
    pub graphic_name: String,
    pub graphic2_name: String,
    pub target_verb: String,
    pub target_base: String,
    pub target_mask: i32,
    pub button_text: String,
}

impl Default for IfType {
    fn default() -> Self {
        IfType {
            anim_frame: 0,
            anim_cycle: 0,
            id: -1,
            layer_id: -1,
            r#type: -1,
            button_type: -1,
            client_code: 0,
            width: 0,
            height: 0,
            trans: 0,
            over_layer_id: -1,
            x: 0,
            y: 0,
            scripts: None,
            script_comparator: None,
            script_operand: None,
            scroll_height: 0,
            scroll_pos: 0,
            hide: false,
            children: None,
            child_x: None,
            child_y: None,
            link_obj_type: None,
            link_obj_number: None,
            obj_swap: false,
            obj_ops: false,
            obj_use: false,
            obj_replace: false,
            margin_x: 0,
            margin_y: 0,
            inv_background_x: None,
            inv_background_y: None,
            inv_background_name: None,
            iop: Default::default(),
            fill: false,
            centre: false,
            font: 0,
            shadow: false,
            text: String::new(),
            text2: String::new(),
            colour: 0,
            colour2: 0,
            colour_over: 0,
            colour2_over: 0,
            model1_type: 0,
            model1_id: 0,
            model2_id: 0,
            model2_type: 0,
            model_anim: -1,
            model_anim2: -1,
            model_zoom: 0,
            model_xan: 0,
            model_yan: 0,
            graphic_name: String::new(),
            graphic2_name: String::new(),
            target_verb: String::new(),
            target_base: String::new(),
            target_mask: -1,
            button_text: String::new(),
        }
    }
}

impl IfType {
    /// `swapSlots` from client-ts IfType.ts 350-361: exchange the object
    /// id and count of two inventory slots.
    pub fn swap_slots(&mut self, src: usize, dst: usize) {
        if self.link_obj_type.is_none() || self.link_obj_number.is_none() {
            return;
        }
        let (t, n) = (
            self.link_obj_type.as_mut().unwrap(),
            self.link_obj_number.as_mut().unwrap(),
        );
        t.swap(src, dst);
        n.swap(src, dst);
    }

    /// Component ids are sparse, so this returns `Vec<Option<IfType>>`
    /// indexed by component id (the TS `list` array grows the same way).
    pub fn unpack(jag: &JagFile) -> Vec<Option<IfType>> {
        let Some(data) = jag.read("data") else {
            return Vec::new();
        };
        let mut dat = Packet::new(data);
        let mut layer = -1;
        let _count = dat.g2();
        let mut list: Vec<Option<IfType>> = Vec::new();
        while dat.pos < dat.length() {
            let mut id = dat.g2();
            if id == 65535 {
                layer = dat.g2();
                id = dat.g2();
            }
            let mut com = IfType { id, layer_id: layer, ..IfType::default() };
            com.r#type = dat.g1();
            com.button_type = dat.g1();
            com.client_code = dat.g2();
            com.width = dat.g2();
            com.height = dat.g2();
            com.trans = dat.g1();

            com.over_layer_id = dat.g1();
            if com.over_layer_id == 0 {
                com.over_layer_id = -1;
            } else {
                com.over_layer_id = ((com.over_layer_id - 1) << 8) + dat.g1();
            }

            let script_stack_count = dat.g1();
            if script_stack_count > 0 {
                let mut comparator = Vec::with_capacity(script_stack_count as usize);
                let mut operand = Vec::with_capacity(script_stack_count as usize);
                for _ in 0..script_stack_count {
                    comparator.push(dat.g1());
                    operand.push(dat.g2());
                }
                com.script_comparator = Some(comparator);
                com.script_operand = Some(operand);
            }

            let script_count = dat.g1();
            if script_count > 0 {
                let mut scripts = Vec::with_capacity(script_count as usize);
                for _ in 0..script_count {
                    let opcode_count = dat.g2();
                    let mut script = Vec::with_capacity(opcode_count as usize);
                    for _ in 0..opcode_count {
                        script.push(dat.g2());
                    }
                    scripts.push(script);
                }
                com.scripts = Some(scripts);
            }

            if com.r#type == ComponentType::TYPE_LAYER {
                com.scroll_height = dat.g2();
                com.hide = dat.g1() == 1;
                let child_count = dat.g2();
                let mut children = Vec::with_capacity(child_count as usize);
                let mut child_x = Vec::with_capacity(child_count as usize);
                let mut child_y = Vec::with_capacity(child_count as usize);
                for _ in 0..child_count {
                    children.push(dat.g2());
                    child_x.push(dat.g2b());
                    child_y.push(dat.g2b());
                }
                com.children = Some(children);
                com.child_x = Some(child_x);
                com.child_y = Some(child_y);
            }

            if com.r#type == ComponentType::TYPE_UNUSED {
                dat.pos += 3;
            }

            if com.r#type == ComponentType::TYPE_INV {
                com.link_obj_type = Some(vec![0; (com.width * com.height) as usize]);
                com.link_obj_number = Some(vec![0; (com.width * com.height) as usize]);

                com.obj_swap = dat.g1() == 1;
                com.obj_ops = dat.g1() == 1;
                com.obj_use = dat.g1() == 1;
                com.obj_replace = dat.g1() == 1;

                com.margin_x = dat.g1();
                com.margin_y = dat.g1();

                // Java allocates invBackgroundX/Y as `new int[20]`, so the
                // x/y offsets are indexed by slot (missing slots stay 0).
                let mut inv_background_x = vec![0; 20];
                let mut inv_background_y = vec![0; 20];
                let mut inv_background_name: Vec<Option<String>> = (0..20).map(|_| None).collect();
                for slot in 0..20 {
                    if dat.g1() == 1 {
                        inv_background_x[slot] = dat.g2b();
                        inv_background_y[slot] = dat.g2b();
                        // Java depacks the sprite from the `media` jag here
                        // (IfType.java 297-300); the Rust unpack holds only
                        // the interface jag, so the "name,index" gjstr is
                        // stored and depacked on demand by draw_interface.
                        let s = dat.gjstr();
                        if !s.is_empty() {
                            inv_background_name[slot] = Some(s);
                        }
                    }
                }
                com.inv_background_x = Some(inv_background_x);
                com.inv_background_y = Some(inv_background_y);
                com.inv_background_name = Some(inv_background_name);

                for i in 0..5 {
                    let s = dat.gjstr();
                    com.iop[i] = if s.is_empty() { None } else { Some(s) };
                }
            }

            if com.r#type == ComponentType::TYPE_RECT {
                com.fill = dat.g1() == 1;
            }

            if com.r#type == ComponentType::TYPE_TEXT || com.r#type == ComponentType::TYPE_UNUSED {
                com.centre = dat.g1() == 1;
                // fonts land with Task 14; keep the font-array index
                com.font = dat.g1();
                com.shadow = dat.g1() == 1;
            }

            if com.r#type == ComponentType::TYPE_TEXT {
                com.text = dat.gjstr();
                com.text2 = dat.gjstr();
            }

            if com.r#type == ComponentType::TYPE_UNUSED
                || com.r#type == ComponentType::TYPE_RECT
                || com.r#type == ComponentType::TYPE_TEXT
            {
                com.colour = dat.g4();
            }

            if com.r#type == ComponentType::TYPE_RECT || com.r#type == ComponentType::TYPE_TEXT {
                com.colour2 = dat.g4();
                com.colour_over = dat.g4();
                com.colour2_over = dat.g4();
            }

            if com.r#type == ComponentType::TYPE_GRAPHIC {
                // "name,index" as IfType.ts 251-262; the sprites themselves
                // depack from the `media` jag on demand (draw_interface).
                com.graphic_name = dat.gjstr();
                com.graphic2_name = dat.gjstr();
            }

            if com.r#type == ComponentType::TYPE_MODEL {
                let model = dat.g1();
                if model != 0 {
                    com.model1_type = 1;
                    com.model1_id = ((model - 1) << 8) + dat.g1();
                }

                let active_model = dat.g1();
                if active_model != 0 {
                    com.model2_type = 1;
                    com.model2_id = ((active_model - 1) << 8) + dat.g1();
                }

                com.model_anim = dat.g1();
                if com.model_anim == 0 {
                    com.model_anim = -1;
                } else {
                    com.model_anim = ((com.model_anim - 1) << 8) + dat.g1();
                }

                com.model_anim2 = dat.g1();
                if com.model_anim2 == 0 {
                    com.model_anim2 = -1;
                } else {
                    com.model_anim2 = ((com.model_anim2 - 1) << 8) + dat.g1();
                }

                com.model_zoom = dat.g2();
                com.model_xan = dat.g2();
                com.model_yan = dat.g2();
            }

            if com.r#type == ComponentType::TYPE_INV_TEXT {
                com.link_obj_type = Some(vec![0; (com.width * com.height) as usize]);
                com.link_obj_number = Some(vec![0; (com.width * com.height) as usize]);

                com.centre = dat.g1() == 1;
                // fonts land with Task 14; keep the font-array index
                com.font = dat.g1();
                com.shadow = dat.g1() == 1;
                com.colour = dat.g4();
                com.margin_x = dat.g2b();
                com.margin_y = dat.g2b();

                com.obj_ops = dat.g1() == 1;

                for i in 0..5 {
                    let s = dat.gjstr();
                    com.iop[i] = if s.is_empty() { None } else { Some(s) };
                }
            }

            if com.button_type == ButtonType::BUTTON_TARGET || com.r#type == ComponentType::TYPE_INV
            {
                com.target_verb = dat.gjstr();
                com.target_base = dat.gjstr();
                com.target_mask = dat.g2();
            }

            if com.button_type == ButtonType::BUTTON_OK
                || com.button_type == ButtonType::BUTTON_TOGGLE
                || com.button_type == ButtonType::BUTTON_SELECT
                || com.button_type == ButtonType::BUTTON_CONTINUE
            {
                com.button_text = dat.gjstr();
                if com.button_text.is_empty() {
                    com.button_text = match com.button_type {
                        ButtonType::BUTTON_OK => "Ok".into(),
                        ButtonType::BUTTON_TOGGLE => "Select".into(),
                        ButtonType::BUTTON_SELECT => "Select".into(),
                        _ => "Continue".into(),
                    };
                }
            }

            if list.len() <= id as usize {
                list.resize(id as usize + 1, None);
            }
            list[id as usize] = Some(com);
        }
        list
    }

    /// `getModel(type, id)` from Java IfType.java 482-506: the cached base
    /// model for a TYPE_MODEL component. LRU key `(type << 16) + id`; type 1
    /// loads a model, 2 an NPC head, 3 the local player's head, 4 an obj's
    /// unlit model (count 50), anything else (5) is null.
    pub fn get_model(
        cache: &Cache,
        local_player: Option<&ClientPlayer>,
        r#type: i32,
        id: i32,
    ) -> Option<Model> {
        let key = ((r#type as i64) << 16) + id as i64;
        {
            let mut model_cache = model_cache().lock().unwrap();
            if let Some(model) = model_cache.find(key) {
                return Some(model.clone());
            }
        }

        let model = match r#type {
            1 => Model::load(id),
            2 => {
                if (id as usize) < cache.npcs.len() {
                    cache.npc(id as usize).get_head()
                } else {
                    None
                }
            }
            3 => local_player.and_then(|p| p.get_head_model(cache)),
            4 => {
                if (id as usize) < cache.objs.len() {
                    cache.obj(id as usize).get_model_unlit(cache, 50)
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(model) = &model {
            model_cache().lock().unwrap().put(model.clone(), key);
        }
        model
    }

    /// `cacheModel(model, type, id)` from IfType.ts 424-430: clear the whole
    /// model cache, then put the temp model under `(type << 16) + id` (type 4
    /// is skipped). The design-preview arm caches under `(5 << 16) + 0`,
    /// which `get_model(5, 0)` finds before its `_ => None` arm (TS
    /// `getModel` type 5 reads the cache the same way).
    pub fn cache_model(model: Model, r#type: i32, id: i32) {
        let mut model_cache = model_cache().lock().unwrap();
        model_cache.clear();
        if r#type != 4 {
            model_cache.put(model, ((r#type as i64) << 16) + id as i64);
        }
    }

    /// `getTempModel(primaryFrame, secondaryFrame, active)` from Java
    /// IfType.java 454-478 (TS IfType.ts 364-394): the animated model for a
    /// TYPE_MODEL component, picked from the active (`model2`) or inactive
    /// (`model1`) slot. With no frame ids and no face colours the base model
    /// returns directly; otherwise a copy animates both frames and re-lights.
    pub fn get_temp_model(
        &self,
        cache: &Cache,
        local_player: Option<&ClientPlayer>,
        primary: i32,
        secondary: i32,
        active: bool,
    ) -> Option<Model> {
        let base = if active {
            Self::get_model(cache, local_player, self.model2_type, self.model2_id)
        } else {
            Self::get_model(cache, local_player, self.model1_type, self.model1_id)
        }?;

        if primary == -1 && secondary == -1 && base.face_colour.is_none() {
            return Some(base);
        }

        let mut tmp = Model::copy_for_anim(
            &base,
            true,
            AnimFrame::animate_transparencies(primary) && AnimFrame::animate_transparencies(secondary),
            false,
        );
        if primary != -1 || secondary != -1 {
            tmp.prepare_anim();
        }
        if primary != -1 {
            tmp.animate(primary);
        }
        if secondary != -1 {
            tmp.animate(secondary);
        }
        tmp.calculate_normals(64, 768, -50, -10, -50, true);
        Some(tmp)
    }
}
