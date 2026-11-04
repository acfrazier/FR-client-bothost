//! Revision-377 interface component decoder and sparse interface archive.
//!
//! Java source anchors below are relative to `src/main/java/jagex2/` in the
//! read-only Client-Java 377 checkout. Component defaults and fields use
//! `config/Component.java:14-230`; decoding uses `config/Component.java:313-554`.
//! `unpack`, lazy `get` and `unloadCom` use `config/Component.java:557-587`,
//! `config/Component.java:256-266`, and `config/Component.java:589-600`.
//! Type 8 text is 377-only at `config/Component.java:524-526`; the 289
//! component branches at `config/IfType.java:269-400` stop at type 7.
//! Script semantics use revision-377 `client/Client.java:11174-11302`.
//! Client-Java 289's `client/Client.java:10274-10403` also observes IDs 0-20.
//! This module never maps unknown 377 words through a 289 opcode table.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use super::config_377::{DecodeError377, DecodeErrorKind377, Reader377};

/// A sprite reference from Java's `getImage(index, name)` call.
///
/// The source string is split at its final comma, as `Component.decode` does.
/// Media pixels are not loaded here.
#[derive(Debug, PartialEq, Eq)]
pub struct SpriteRef377 {
    pub name: String,
    pub index: i32,
}

/// One revision-377 interface component, before any runtime rendering or
/// interaction state is applied.
///
/// Field names follow the Java source where practical. Java-null references
/// remain `None`; a decoded empty string remains `Some(String::new())` unless
/// Java explicitly converts it to null (for example, empty `iop` entries).
#[derive(Debug, Default, PartialEq)]
pub struct IfType377 {
    pub id: i32,
    pub layer: i32,
    pub r#type: i32,
    pub button_type: i32,
    pub client_code: i32,
    pub width: i32,
    pub height: i32,
    /// Java `byte trans`, sign-extended from the input byte.
    pub trans: i32,
    /// Java `overlayer`, after its zero-to-minus-one sentinel conversion.
    pub overlayer: i32,
    /// Java's six unencoded runtime integers remain zero on a fresh decode.
    pub field_700: i32,
    pub field_709: i32,
    pub field_710: i32,
    pub field_713: i32,
    pub field_717: i32,
    pub field_741: i32,
    pub script_comparator: Option<Vec<i32>>,
    pub script_operand: Option<Vec<i32>>,
    /// Every unsigned-short word is retained in its original order.
    pub scripts: Option<Vec<Vec<i32>>>,
    pub scroll: i32,
    pub hide: bool,
    pub children: Option<Vec<i32>>,
    pub child_x: Option<Vec<i32>>,
    pub child_y: Option<Vec<i32>>,
    /// Java `field707` and `field715`, decoded only for type 1.
    pub field_707: i32,
    pub field_715: bool,
    pub inv_slot_obj_id: Option<Vec<i32>>,
    pub inv_slot_obj_count: Option<Vec<i32>>,
    pub draggable: bool,
    pub interactable: bool,
    pub usable: bool,
    pub swappable: bool,
    pub margin_x: i32,
    pub margin_y: i32,
    pub inv_slot_offset_x: Option<Vec<i32>>,
    pub inv_slot_offset_y: Option<Vec<i32>>,
    /// The 20 Java `invSlotGraphic` entries, represented as media references.
    pub inv_slot_graphic: Option<Vec<Option<SpriteRef377>>>,
    /// Java `iop`, allocated only by component types 2 and 7.
    pub iop: Option<[Option<String>; 5]>,
    pub fill: bool,
    pub center: bool,
    /// Java's font-array index. The renderer resolves it against its font set.
    pub font_index: i32,
    pub shadowed: bool,
    pub text: Option<String>,
    pub active_text: Option<String>,
    pub colour: i32,
    pub active_colour: i32,
    pub over_colour: i32,
    pub active_over_colour: i32,
    pub graphic: Option<SpriteRef377>,
    pub active_graphic: Option<SpriteRef377>,
    pub model_type: i32,
    pub model: i32,
    pub active_model_type: i32,
    pub active_model: i32,
    pub anim: i32,
    pub active_anim: i32,
    pub zoom: i32,
    pub xan: i32,
    pub yan: i32,
    pub target_verb: Option<String>,
    pub target_text: Option<String>,
    pub target_mask: i32,
    /// Java `option`, including its button-specific defaults.
    pub option: Option<String>,
}

impl IfType377 {
    /// Decode one component body. The body starts at Java's `type` byte and
    /// excludes the component ID and the layer prefix.
    pub fn decode(id: i32, layer: i32, buf: &mut Reader377<'_>) -> Result<Self, DecodeError377> {
        let mut component = Self {
            id,
            layer,
            ..Self::default()
        };

        component.r#type = buf.g1()?;
        component.button_type = buf.g1()?;
        component.client_code = buf.g2()?;
        component.width = buf.g2()?;
        component.height = buf.g2()?;
        component.trans = buf.g1b()?;

        let overlayer = buf.g1()?;
        component.overlayer = if overlayer == 0 {
            -1
        } else {
            ((overlayer - 1) << 8) + buf.g1()?
        };

        let comparator_count = buf.g1()? as usize;
        if comparator_count > 0 {
            let mut comparators = Vec::with_capacity(comparator_count);
            let mut operands = Vec::with_capacity(comparator_count);
            for _ in 0..comparator_count {
                comparators.push(buf.g1()?);
                operands.push(buf.g2()?);
            }
            component.script_comparator = Some(comparators);
            component.script_operand = Some(operands);
        }

        let script_count = buf.g1()? as usize;
        if script_count > 0 {
            let mut scripts = Vec::with_capacity(script_count);
            for _ in 0..script_count {
                let word_count = buf.g2()? as usize;
                let mut words = Vec::with_capacity(word_count);
                for _ in 0..word_count {
                    words.push(buf.g2()?);
                }
                scripts.push(words);
            }
            component.scripts = Some(scripts);
        }

        if component.r#type == 0 {
            component.scroll = buf.g2()?;
            component.hide = buf.g1()? == 1;
            let child_count = buf.g2()? as usize;
            let mut children = Vec::with_capacity(child_count);
            let mut child_x = Vec::with_capacity(child_count);
            let mut child_y = Vec::with_capacity(child_count);
            for _ in 0..child_count {
                children.push(buf.g2()?);
                child_x.push(buf.g2b()?);
                child_y.push(buf.g2b()?);
            }
            component.children = Some(children);
            component.child_x = Some(child_x);
            component.child_y = Some(child_y);
        }

        if component.r#type == 1 {
            component.field_707 = buf.g2()?;
            component.field_715 = buf.g1()? == 1;
        }

        if component.r#type == 2 {
            let slot_count = inventory_slot_count(buf, component.width, component.height)?;
            component.inv_slot_obj_id = Some(zeroed_slots(buf, slot_count)?);
            component.inv_slot_obj_count = Some(zeroed_slots(buf, slot_count)?);

            component.draggable = buf.g1()? == 1;
            component.interactable = buf.g1()? == 1;
            component.usable = buf.g1()? == 1;
            component.swappable = buf.g1()? == 1;
            component.margin_x = buf.g1()?;
            component.margin_y = buf.g1()?;

            let mut offset_x = vec![0; 20];
            let mut offset_y = vec![0; 20];
            let mut graphics = Vec::with_capacity(20);
            for (slot_x, slot_y) in offset_x.iter_mut().zip(offset_y.iter_mut()) {
                if buf.g1()? == 1 {
                    *slot_x = buf.g2b()?;
                    *slot_y = buf.g2b()?;
                    graphics.push(sprite_ref(buf.gjstr()?, buf)?);
                } else {
                    graphics.push(None);
                }
            }
            component.inv_slot_offset_x = Some(offset_x);
            component.inv_slot_offset_y = Some(offset_y);
            component.inv_slot_graphic = Some(graphics);
            component.iop = Some(read_iop(buf)?);
        }

        if component.r#type == 3 {
            component.fill = buf.g1()? == 1;
        }

        if component.r#type == 4 || component.r#type == 1 {
            component.center = buf.g1()? == 1;
            component.font_index = buf.g1()?;
            component.shadowed = buf.g1()? == 1;
        }

        if component.r#type == 4 {
            component.text = Some(buf.gjstr()?);
            component.active_text = Some(buf.gjstr()?);
        }

        if component.r#type == 1 || component.r#type == 3 || component.r#type == 4 {
            component.colour = buf.g4()?;
        }

        if component.r#type == 3 || component.r#type == 4 {
            component.active_colour = buf.g4()?;
            component.over_colour = buf.g4()?;
            component.active_over_colour = buf.g4()?;
        }

        if component.r#type == 5 {
            component.graphic = sprite_ref(buf.gjstr()?, buf)?;
            component.active_graphic = sprite_ref(buf.gjstr()?, buf)?;
        }

        if component.r#type == 6 {
            let model = buf.g1()?;
            if model != 0 {
                component.model_type = 1;
                component.model = ((model - 1) << 8) + buf.g1()?;
            }

            let active_model = buf.g1()?;
            if active_model != 0 {
                component.active_model_type = 1;
                component.active_model = ((active_model - 1) << 8) + buf.g1()?;
            }

            let anim = buf.g1()?;
            component.anim = if anim == 0 {
                -1
            } else {
                ((anim - 1) << 8) + buf.g1()?
            };

            let active_anim = buf.g1()?;
            component.active_anim = if active_anim == 0 {
                -1
            } else {
                ((active_anim - 1) << 8) + buf.g1()?
            };

            component.zoom = buf.g2()?;
            component.xan = buf.g2()?;
            component.yan = buf.g2()?;
        }

        if component.r#type == 7 {
            let slot_count = inventory_slot_count(buf, component.width, component.height)?;
            component.inv_slot_obj_id = Some(zeroed_slots(buf, slot_count)?);
            component.inv_slot_obj_count = Some(zeroed_slots(buf, slot_count)?);

            component.center = buf.g1()? == 1;
            component.font_index = buf.g1()?;
            component.shadowed = buf.g1()? == 1;
            component.colour = buf.g4()?;
            component.margin_x = buf.g2b()?;
            component.margin_y = buf.g2b()?;
            component.interactable = buf.g1()? == 1;
            component.iop = Some(read_iop(buf)?);
        }

        if component.r#type == 8 {
            component.text = Some(buf.gjstr()?);
        }

        if component.button_type == 2 || component.r#type == 2 {
            component.target_verb = Some(buf.gjstr()?);
            component.target_text = Some(buf.gjstr()?);
            component.target_mask = buf.g2()?;
        }

        if matches!(component.button_type, 1 | 4 | 5 | 6) {
            let option = buf.gjstr()?;
            component.option = Some(if option.is_empty() {
                match component.button_type {
                    1 => "Ok".to_owned(),
                    4 | 5 => "Select".to_owned(),
                    6 => "Continue".to_owned(),
                    _ => unreachable!(),
                }
            } else {
                option
            });
        }

        Ok(component)
    }
}

fn inventory_slot_count(
    buf: &Reader377<'_>,
    width: i32,
    height: i32,
) -> Result<usize, DecodeError377> {
    let product = width.checked_mul(height).ok_or_else(|| {
        buf.error(DecodeErrorKind377::InvalidValue(
            "inventory slot count overflow",
        ))
    })?;
    usize::try_from(product).map_err(|_| {
        buf.error(DecodeErrorKind377::InvalidValue(
            "negative inventory slot count",
        ))
    })
}

fn zeroed_slots(buf: &Reader377<'_>, count: usize) -> Result<Vec<i32>, DecodeError377> {
    let mut slots = Vec::new();
    slots.try_reserve_exact(count).map_err(|_| {
        buf.error(DecodeErrorKind377::InvalidValue(
            "inventory slot allocation failed",
        ))
    })?;
    slots.resize(count, 0);
    Ok(slots)
}

fn read_iop(buf: &mut Reader377<'_>) -> Result<[Option<String>; 5], DecodeError377> {
    let mut actions = std::array::from_fn(|_| None);
    for action in &mut actions {
        let text = buf.gjstr()?;
        if !text.is_empty() {
            *action = Some(text);
        }
    }
    Ok(actions)
}

fn sprite_ref(raw: String, buf: &Reader377<'_>) -> Result<Option<SpriteRef377>, DecodeError377> {
    if raw.is_empty() {
        return Ok(None);
    }
    let Some(separator) = raw.rfind(',') else {
        return Err(buf.error(DecodeErrorKind377::InvalidValue(
            "sprite reference lacks index separator",
        )));
    };
    let index = raw[separator + 1..].parse::<i32>().map_err(|_| {
        buf.error(DecodeErrorKind377::InvalidValue(
            "sprite reference has invalid index",
        ))
    })?;
    Ok(Some(SpriteRef377 {
        name: raw[..separator].to_owned(),
        index,
    }))
}

/// The three static layer values changed by `Component.decode` client codes.
///
/// Java names these fields `field728`, `field737` and `field759`. The archive
/// retains scan-time decode order, then updates them again on first `get` and
/// each post-unload decode; cached hits have no decode side effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientCodeLayers377 {
    pub field_728: i32,
    pub field_737: i32,
    pub field_759: i32,
}

impl Default for ClientCodeLayers377 {
    fn default() -> Self {
        Self {
            field_728: -1,
            field_737: -1,
            field_759: -1,
        }
    }
}

impl ClientCodeLayers377 {
    fn apply(&mut self, component: &IfType377) {
        match component.client_code {
            600 => self.field_728 = component.layer,
            650 => self.field_737 = component.layer,
            655 => self.field_759 = component.layer,
            _ => {}
        }
    }
}

/// Revision-377 `Client.executeClientScript` instruction IDs.
///
/// These IDs are source-derived from revision-377 `client/Client.java:11184-11297`.
/// Client-Java 289 `client/Client.java:10286-10399` also uses IDs 0-20,
/// but an unrecognized 377 word is never assigned 289 semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ScriptOpcode377 {
    Terminate = 0,
    SkillLevel = 1,
    SkillBaseLevel = 2,
    SkillExperience = 3,
    ComponentInventoryCount = 4,
    Varp = 5,
    ExperienceForNextLevel = 6,
    VarpPercent = 7,
    CombatLevel = 8,
    TotalSkillLevel = 9,
    ComponentInventoryContains = 10,
    RunEnergy = 11,
    RunWeight = 12,
    VarpBit = 13,
    Varbit = 14,
    Subtract = 15,
    Divide = 16,
    Multiply = 17,
    PlayerTileX = 18,
    PlayerTileZ = 19,
    Literal = 20,
}

impl ScriptOpcode377 {
    /// Return only opcodes present in the 377 Java evaluator.
    pub fn from_id(id: i32) -> Option<Self> {
        Some(match id {
            0 => Self::Terminate,
            1 => Self::SkillLevel,
            2 => Self::SkillBaseLevel,
            3 => Self::SkillExperience,
            4 => Self::ComponentInventoryCount,
            5 => Self::Varp,
            6 => Self::ExperienceForNextLevel,
            7 => Self::VarpPercent,
            8 => Self::CombatLevel,
            9 => Self::TotalSkillLevel,
            10 => Self::ComponentInventoryContains,
            11 => Self::RunEnergy,
            12 => Self::RunWeight,
            13 => Self::VarpBit,
            14 => Self::Varbit,
            15 => Self::Subtract,
            16 => Self::Divide,
            17 => Self::Multiply,
            18 => Self::PlayerTileX,
            19 => Self::PlayerTileZ,
            20 => Self::Literal,
            _ => return None,
        })
    }

    fn operand_words(self) -> usize {
        match self {
            Self::SkillLevel
            | Self::SkillBaseLevel
            | Self::SkillExperience
            | Self::Varp
            | Self::ExperienceForNextLevel
            | Self::VarpPercent
            | Self::Varbit => 1,
            Self::ComponentInventoryCount | Self::ComponentInventoryContains | Self::VarpBit => 2,
            Self::Literal => 1,
            Self::Terminate
            | Self::CombatLevel
            | Self::TotalSkillLevel
            | Self::RunEnergy
            | Self::RunWeight
            | Self::Subtract
            | Self::Divide
            | Self::Multiply
            | Self::PlayerTileX
            | Self::PlayerTileZ => 0,
        }
    }
}

/// A source-word span for one parsed script instruction.
#[derive(Debug, PartialEq, Eq)]
pub struct ScriptInstruction377 {
    pub opcode: ScriptOpcode377,
    pub word_index: usize,
    pub operands: Range<usize>,
}

/// A malformed or unknown script word. The original script words remain
/// untouched in `IfType377::scripts`; this parser does not evaluate them.
#[derive(Debug, PartialEq, Eq)]
pub enum ScriptParseError377 {
    UnknownOpcode {
        word_index: usize,
        opcode: i32,
    },
    TruncatedOperands {
        word_index: usize,
        opcode: ScriptOpcode377,
        required: usize,
        available: usize,
    },
}

fn walk_script_words(
    words: &[i32],
    mut visit: impl FnMut(ScriptOpcode377, usize, Range<usize>),
) -> Result<(), ScriptParseError377> {
    let mut cursor = 0;
    while cursor < words.len() {
        let word_index = cursor;
        let raw_opcode = words[cursor];
        let opcode =
            ScriptOpcode377::from_id(raw_opcode).ok_or(ScriptParseError377::UnknownOpcode {
                word_index,
                opcode: raw_opcode,
            })?;
        cursor += 1;
        if opcode == ScriptOpcode377::Terminate {
            visit(opcode, word_index, cursor..cursor);
            return Ok(());
        }
        let operand_count = opcode.operand_words();
        let available = words.len() - cursor;
        if available < operand_count {
            return Err(ScriptParseError377::TruncatedOperands {
                word_index,
                opcode,
                required: operand_count,
                available,
            });
        }
        let operands = cursor..cursor + operand_count;
        visit(opcode, word_index, operands.clone());
        cursor = operands.end;
    }
    Ok(())
}

/// Parse instruction boundaries from the 377 Java opcode table. Parsing stops
/// at Java's opcode-0 terminator. Unknown opcodes and short operands are errors.
pub fn parse_script_377(words: &[i32]) -> Result<Vec<ScriptInstruction377>, ScriptParseError377> {
    let mut instructions = Vec::new();
    walk_script_words(words, |opcode, word_index, operands| {
        instructions.push(ScriptInstruction377 {
            opcode,
            word_index,
            operands,
        });
    })?;
    Ok(instructions)
}

/// Statistics collected while scanning every component record.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InterfaceScanStats377 {
    pub declared_total: usize,
    pub record_count: usize,
    pub occupied_slots: usize,
    pub input_bytes: usize,
    /// Sum of the Java-saved layer prefix plus body for the final record at
    /// each populated slot. The source archive itself is retained once.
    pub saved_record_bytes: usize,
    /// Counts every parsed record, including duplicate IDs that Java overwrites.
    pub component_type_counts: BTreeMap<i32, usize>,
    pub script_count: usize,
    pub script_word_count: usize,
    pub script_opcode_counts: BTreeMap<i32, usize>,
    pub unknown_script_opcode_counts: BTreeMap<i32, usize>,
    pub malformed_script_count: usize,
}

fn count_script_words(stats: &mut InterfaceScanStats377, words: &[i32]) {
    stats.script_count += 1;
    stats.script_word_count += words.len();
    match walk_script_words(words, |opcode, _, _| {
        *stats.script_opcode_counts.entry(opcode as i32).or_default() += 1;
    }) {
        Ok(()) => {}
        Err(ScriptParseError377::UnknownOpcode { opcode, .. }) => {
            *stats
                .unknown_script_opcode_counts
                .entry(opcode)
                .or_default() += 1;
            stats.malformed_script_count += 1;
        }
        Err(ScriptParseError377::TruncatedOperands { .. }) => {
            stats.malformed_script_count += 1;
        }
    }
}

struct RecordRuntime377 {
    decoded: Option<Arc<IfType377>>,
}

struct InterfaceRecord377 {
    id: i32,
    layer: i32,
    component_type: i32,
    payload: Range<usize>,
    runtime: Mutex<RecordRuntime377>,
}

/// Java's per-component `data[id]` value without copying the shared payload.
///
/// `as_java_bytes` materializes `[layer_hi, layer_lo, payload..]` only when a
/// caller requests the Java-compatible saved byte sequence.
pub struct SavedRecord377<'a> {
    pub layer: i32,
    payload: &'a [u8],
}

impl SavedRecord377<'_> {
    pub fn payload(&self) -> &[u8] {
        self.payload
    }

    pub fn byte_len(&self) -> usize {
        self.payload.len() + 2
    }

    pub fn as_java_bytes(&self) -> Vec<u8> {
        let layer = self.layer as u16;
        let mut bytes = Vec::with_capacity(self.byte_len());
        bytes.extend_from_slice(&layer.to_be_bytes());
        bytes.extend_from_slice(self.payload);
        bytes
    }
}

/// Fully scanned interface member with Java-compatible lazy definitions and
/// raw record ranges. Scan-time decoding reproduces Java `unpack` side effects;
/// the first `get` then decodes again, as Java's initially empty `types` array
/// requires. The source member backs every raw record; no per-record payload
/// copy is retained.
pub struct InterfaceArchive377 {
    data: Arc<[u8]>,
    records: Vec<Option<InterfaceRecord377>>,
    stats: InterfaceScanStats377,
    client_code_layers: Mutex<ClientCodeLayers377>,
}

impl InterfaceArchive377 {
    /// Copy one interface member once into an immutable shared backing buffer,
    /// decode every component as Java `unpack` does, then retain its sparse raw
    /// ranges and metadata for lazy `get` and layer unload/reload.
    pub fn scan(data: &[u8]) -> Result<Self, DecodeError377> {
        Self::scan_shared(Arc::from(data))
    }

    /// Scan an existing shared interface member without copying its bytes.
    pub fn scan_shared(data: Arc<[u8]>) -> Result<Self, DecodeError377> {
        let mut reader = Reader377::new(&data, "interface", 0);
        let declared_total = reader.g2()? as usize;
        let mut records: Vec<Option<InterfaceRecord377>> = Vec::with_capacity(declared_total);
        records.resize_with(declared_total, || None);
        let mut stats = InterfaceScanStats377 {
            declared_total,
            input_bytes: data.len(),
            ..InterfaceScanStats377::default()
        };
        let mut layer = -1;
        let mut client_code_layers = ClientCodeLayers377::default();

        while reader.pos < reader.data.len() {
            reader.record = stats.record_count;
            let mut id = reader.g2()?;
            if id == 65_535 {
                layer = reader.g2()?;
                id = reader.g2()?;
            }
            let slot = usize::try_from(id).map_err(|_| {
                reader.error(DecodeErrorKind377::InvalidValue("negative component ID"))
            })?;
            if slot >= declared_total {
                return Err(reader.error(DecodeErrorKind377::InvalidValue(
                    "component ID exceeds declared interface total",
                )));
            }

            let payload_start = reader.pos;
            let component = IfType377::decode(id, layer, &mut reader)?;
            let payload_end = reader.pos;
            client_code_layers.apply(&component);
            let component_type = component.r#type;
            *stats
                .component_type_counts
                .entry(component_type)
                .or_default() += 1;
            if let Some(scripts) = &component.scripts {
                for words in scripts {
                    count_script_words(&mut stats, words);
                }
            }
            stats.record_count += 1;

            records[slot] = Some(InterfaceRecord377 {
                id,
                layer,
                component_type,
                payload: payload_start..payload_end,
                runtime: Mutex::new(RecordRuntime377 { decoded: None }),
            });
        }
        reader.finish()?;

        stats.occupied_slots = records.iter().filter(|record| record.is_some()).count();
        stats.saved_record_bytes = records
            .iter()
            .filter_map(Option::as_ref)
            .map(|record| record.payload.len() + 2)
            .sum();

        Ok(Self {
            data,
            records,
            stats,
            client_code_layers: Mutex::new(client_code_layers),
        })
    }

    pub fn declared_total(&self) -> usize {
        self.records.len()
    }

    pub fn occupied_slots(&self) -> usize {
        self.stats.occupied_slots
    }

    pub fn stats(&self) -> &InterfaceScanStats377 {
        &self.stats
    }

    /// The latest Java client-code layer values after scan or any lazy decode.
    pub fn client_code_layers(&self) -> ClientCodeLayers377 {
        self.client_code_layers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Return the Java-saved record representation for one occupied slot.
    /// The returned view borrows the shared member until `as_java_bytes` is
    /// requested.
    pub fn saved_record(&self, id: i32) -> Option<SavedRecord377<'_>> {
        let slot = usize::try_from(id).ok()?;
        let record = self.records.get(slot)?.as_ref()?;
        Some(SavedRecord377 {
            layer: record.layer,
            payload: &self.data[record.payload.clone()],
        })
    }

    /// Java `Component.get(id)`: return its cached definition, or decode the
    /// saved raw record on first access and after `unload_layer`.
    pub fn get(&self, id: i32) -> Result<Option<Arc<IfType377>>, DecodeError377> {
        let slot = usize::try_from(id).map_err(|_| DecodeError377 {
            table: "interface",
            record: 0,
            offset: 0,
            kind: DecodeErrorKind377::InvalidValue("negative component ID"),
        })?;
        let Some(record) = self.records.get(slot).and_then(Option::as_ref) else {
            if slot >= self.records.len() {
                return Err(DecodeError377 {
                    table: "interface",
                    record: slot,
                    offset: 0,
                    kind: DecodeErrorKind377::InvalidValue(
                        "component ID exceeds declared interface total",
                    ),
                });
            }
            return Ok(None);
        };

        let mut runtime = record
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(component) = &runtime.decoded {
            return Ok(Some(Arc::clone(component)));
        }

        let payload = &self.data[record.payload.clone()];
        let mut reader = Reader377::new(payload, "interface", slot);
        let component = IfType377::decode(record.id, record.layer as u16 as i32, &mut reader)?;
        reader.finish()?;
        if component.r#type != record.component_type {
            return Err(reader.error(DecodeErrorKind377::InvalidValue(
                "saved component type differs from scan metadata",
            )));
        }
        let component = Arc::new(component);
        self.client_code_layers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .apply(&component);
        runtime.decoded = Some(Arc::clone(&component));
        Ok(Some(component))
    }

    /// Java `Component.unloadCom(layer)`: ignore layer -1 and retain inventory
    /// components (type 2); clear only loaded matching component definitions.
    /// The loaded component's decoded layer is authoritative (the first
    /// unprefixed record decodes as 65535 even though its saved layer is -1).
    /// Raw saved bytes remain available for subsequent lazy `get` calls.
    pub fn unload_layer(&self, layer: i32) -> usize {
        if layer == -1 {
            return 0;
        }
        let mut unloaded = 0;
        for record in self.records.iter().filter_map(Option::as_ref) {
            let mut runtime = record
                .runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let should_unload = match runtime.decoded.as_ref() {
                Some(component) => component.layer == layer && component.r#type != 2,
                None => false,
            };
            if should_unload {
                runtime.decoded = None;
                unloaded += 1;
            }
        }
        unloaded
    }
}
