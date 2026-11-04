//! Startup adaptation for revision-377 config and interface archives.
//!
//! This is the bridge from the strict, revision-specific decoders to the
//! existing render/scene/model tables. It never calls a legacy `unpack`.
//! Source anchors: Client-Java 377 `client/Client.java:2044-2055` unpacks
//! config tables and sets `ObjType.membersWorld`; `config/Component.java:
//! 256-266,313-587` lazily decodes the interface archive;
//! `config/ObjType.java:179-203,246-260,262-285,339-445`,
//! `config/NpcType.java:125-220,359-387`, and
//! `config/LocType.java:155-185,381-529` govern config decode and morphs.
//!
//! Fields that the shared runtime tables cannot represent are moved into
//! aligned 377 sidecars. The source cache and interface archive are consumed;
//! no raw duplicate config or interface table remains in `LoadedCache377`.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::io::JagFile;
use crate::util::JavaRandom;

use super::config_377::{
    Cache377, FloType377, IdkType377, LocType377, NpcType377, ObjType377, SeqType377, SpotType377,
    VarbitType377, VarpType377,
};
use super::flo_type::FloType;
use super::idk_type::IdkType;
use super::if_type::{IfType, IfTypeMut};
use super::if_type_377::{ClientCodeLayers377, IfType377, InterfaceArchive377, SpriteRef377};
use super::loc_type::LocType;
use super::npc_type::NpcType;
use super::obj_type::ObjType;
use super::seq_type::SeqType;
use super::spot_type::SpotType;
use super::varbit_type::VarBitType;
use super::varp_type::VarpType;
use super::Cache;

/// One 377 morph selector and its ordered target list.
///
/// The final target is Java's fallback entry. `-1` remains the no-target
/// sentinel, including at arbitrary intermediate entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphTable377 {
    pub varbit: i32,
    pub varp: i32,
    pub targets: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjFields377 {
    /// Java ObjType opcode 10 (`field854`); the shared ObjType has no slot.
    pub field_854: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcFields377 {
    /// Java fields 1451, 1430 and 1440 (opcodes 90-92).
    pub field_1451: i32,
    pub field_1430: i32,
    pub field_1440: i32,
    /// Java `field1434`, cleared by opcode 107 and consulted by Client.java.
    pub field_1434: bool,
    /// Java opcode 106 morph selector, absent unless the opcode was present.
    pub morph: Option<MorphTable377>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocFields377 {
    /// Java opcode 77 morph selector, absent unless the opcode was present.
    pub morph: Option<MorphTable377>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeqFields377 {
    /// Java SeqType opcode 12 (`field790`); retained although the pinned Java
    /// client does not read it.
    pub reserved: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarpFields377 {
    pub code1: i32,
    pub code2: i32,
    pub tracked: bool,
    pub code4: bool,
    pub code6: bool,
    pub code7: i32,
    pub scope: i32,
    pub transmit: bool,
    pub code12: i32,
    pub code14: bool,
    pub debugname: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarbitFields377 {
    pub transmit: bool,
    pub code3: i32,
    pub code4: i32,
    pub code5: bool,
}

/// Config facts without a corresponding field in the existing shared tables.
/// Every vector is in the same record-index order as `LoadedCache377.cache`.
#[derive(Debug, Default)]
pub struct CacheFields377 {
    pub objs: Vec<ObjFields377>,
    pub npcs: Vec<NpcFields377>,
    pub locs: Vec<LocFields377>,
    pub seqs: Vec<SeqFields377>,
    pub varps: Vec<VarpFields377>,
    pub varbits: Vec<VarbitFields377>,
    /// Java's VarpType opcode-3 list, including duplicate occurrences.
    pub tracked_varps: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfType1Fields377 {
    /// Java Component fields 707 and 715, read only for component type 1.
    pub field_707: i32,
    pub field_715: bool,
}

/// Bit flag: Java `text` was null.
pub const INTERFACE_NULL_TEXT_377: u8 = 1 << 0;
/// Bit flag: Java `activeText` was null.
pub const INTERFACE_NULL_ACTIVE_TEXT_377: u8 = 1 << 1;
/// Bit flag: Java `targetVerb` was null.
pub const INTERFACE_NULL_TARGET_VERB_377: u8 = 1 << 2;
/// Bit flag: Java `targetText` was null.
pub const INTERFACE_NULL_TARGET_TEXT_377: u8 = 1 << 3;
/// Bit flag: Java `option` was null.
pub const INTERFACE_NULL_OPTION_377: u8 = 1 << 4;
/// The component had no Java `iop` array (its five element-level Option
/// values remain in `IfType::iop`).
pub const INTERFACE_NULL_IOP_377: u8 = 1 << 5;

/// Interface decode side effects and component fields absent from `IfType`.
#[derive(Debug)]
pub struct InterfaceFields377 {
    /// Java Component static fields 728, 737 and 759 at scan completion.
    pub client_code_layers: ClientCodeLayers377,
    /// Sparse by component ID; only type-1 components have values.
    pub type1_fields: Vec<Option<IfType1Fields377>>,
    /// Null-vs-empty Option<String> facts aligned by component ID. Missing
    /// components have a zero mask; set bits are defined by INTERFACE_NULL_*.
    pub null_string_masks: Vec<u8>,
}

/// Adapted runtime tables plus lossless, index-aligned 377-only facts.
pub struct LoadedCache377 {
    pub cache: Cache,
    pub ifaces: Vec<Option<Box<IfType>>>,
    pub ifaces_mut: Vec<Option<Arc<IfTypeMut>>>,
    pub cache_fields_377: CacheFields377,
    pub interface_fields_377: InterfaceFields377,
}

/// Read and adapt the extracted revision-377 `config` and `interface` JAGs.
///
/// The config and interface contents pass only through `Cache377` and
/// `InterfaceArchive377`. Missing files/members and decoder failures are
/// returned as errors; they are never replaced with empty legacy tables.
pub fn load_cache_377(cache_dir: &str, low_mem: bool) -> Result<LoadedCache377, String> {
    let dir = Path::new(cache_dir);
    let config_jag = read_jag(dir, "config")?;
    let mut random = JavaRandom::now();
    let mut draws = || random.next_double();
    let decoded_config = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Cache377::unpack_with_options(&config_jag, low_mem, &mut draws)
    }))
    .map_err(|_| "config JAG panicked in the revision-377 decoder".to_owned())?
    .map_err(|error| format!("config JAG: {error}"))?;

    let interface_jag = read_jag(dir, "interface")?;
    let interface_data =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| interface_jag.read("data")))
            .map_err(|_| "interface JAG panicked while reading its data member".to_owned())?
            .ok_or_else(|| "interface JAG is missing its data member".to_owned())?;
    let interfaces = InterfaceArchive377::scan_shared(Arc::from(interface_data))
        .map_err(|error| format!("interface data: {error}"))?;

    adapt_cache_377(decoded_config, interfaces)
}

fn read_jag(dir: &Path, name: &str) -> Result<JagFile, String> {
    let path = dir.join(name);
    let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| JagFile::new(bytes)))
        .map_err(|_| format!("{}: invalid JAG container", path.display()))
}

/// Adapt already decoded 377 data. This is public so a caller that owns JAG
/// admission can share the exact same adaptation path as `load_cache_377`.
pub fn adapt_cache_377(
    decoded: Cache377,
    interfaces: InterfaceArchive377,
) -> Result<LoadedCache377, String> {
    // Preserve the Java `unpack` scan-order values before `get` lazily decodes
    // each component again and updates those static fields in ID order.
    let client_code_layers = interfaces.client_code_layers();
    let AdaptedInterfaces377 {
        ifaces,
        ifaces_mut,
        type1_fields,
        null_string_masks,
    } = adapt_interfaces_377(interfaces)?;

    let Cache377 {
        seqs,
        locs,
        flos,
        objs,
        npcs,
        idks,
        spots,
        varps,
        varbits,
        tracked_varps,
        coverage: _,
    } = decoded;

    let mut cache_fields_377 = CacheFields377 {
        tracked_varps,
        ..CacheFields377::default()
    };

    let mut runtime_objs = Vec::with_capacity(objs.len());
    cache_fields_377.objs.reserve(objs.len());
    for obj in objs {
        cache_fields_377.objs.push(ObjFields377 {
            field_854: obj.field_854,
        });
        runtime_objs.push(obj.into());
    }
    apply_certificates_377(&mut runtime_objs)?;

    let mut runtime_npcs = Vec::with_capacity(npcs.len());
    cache_fields_377.npcs.reserve(npcs.len());
    for npc in npcs {
        let (npc, fields) = into_npc_parts(npc);
        runtime_npcs.push(npc);
        cache_fields_377.npcs.push(fields);
    }

    let mut runtime_locs = Vec::with_capacity(locs.len());
    cache_fields_377.locs.reserve(locs.len());
    for loc in locs {
        let (loc, fields) = into_loc_parts(loc);
        runtime_locs.push(loc);
        cache_fields_377.locs.push(fields);
    }

    let mut runtime_seqs = Vec::with_capacity(seqs.len());
    cache_fields_377.seqs.reserve(seqs.len());
    for seq in seqs {
        cache_fields_377.seqs.push(SeqFields377 {
            reserved: seq.reserved,
        });
        runtime_seqs.push(seq.into());
    }

    let mut runtime_varps = Vec::with_capacity(varps.len());
    cache_fields_377.varps.reserve(varps.len());
    for varp in varps {
        let (varp, fields) = into_varp_parts(varp);
        runtime_varps.push(varp);
        cache_fields_377.varps.push(fields);
    }

    let mut runtime_varbits = Vec::with_capacity(varbits.len());
    cache_fields_377.varbits.reserve(varbits.len());
    for varbit in varbits {
        let (varbit, fields) = into_varbit_parts(varbit);
        runtime_varbits.push(varbit);
        cache_fields_377.varbits.push(fields);
    }

    let cache = Cache {
        objs: runtime_objs,
        npcs: runtime_npcs,
        locs: runtime_locs,
        flos: flos.into_iter().map(Into::into).collect(),
        idks: idks.into_iter().map(Into::into).collect(),
        seqs: runtime_seqs,
        spots: spots.into_iter().map(Into::into).collect(),
        varbits: runtime_varbits,
        varps: runtime_varps,
        revision_377: None,
    };

    Ok(LoadedCache377 {
        cache,
        ifaces,
        ifaces_mut,
        cache_fields_377,
        interface_fields_377: InterfaceFields377 {
            client_code_layers,
            type1_fields,
            null_string_masks,
        },
    })
}

struct AdaptedInterfaces377 {
    ifaces: Vec<Option<Box<IfType>>>,
    ifaces_mut: Vec<Option<Arc<IfTypeMut>>>,
    type1_fields: Vec<Option<IfType1Fields377>>,
    null_string_masks: Vec<u8>,
}

fn adapt_interfaces_377(archive: InterfaceArchive377) -> Result<AdaptedInterfaces377, String> {
    let total = archive.declared_total();
    let mut ifaces = Vec::with_capacity(total);
    let mut ifaces_mut = Vec::with_capacity(total);
    let mut type1_fields = Vec::with_capacity(total);
    let mut null_string_masks = vec![0; total];
    ifaces.resize_with(total, || None);
    ifaces_mut.resize_with(total, || None);
    type1_fields.resize_with(total, || None);

    for id in 0..total {
        let component = archive
            .get(id as i32)
            .map_err(|error| format!("interface component {id}: {error}"))?;
        let Some(component) = component else {
            continue;
        };
        if component.r#type == 1 {
            type1_fields[id] = Some(IfType1Fields377 {
                field_707: component.field_707,
                field_715: component.field_715,
            });
        }
        null_string_masks[id] = interface_null_string_mask_377(&component);
        let (component, overlay) = into_interface_parts(&component);
        ifaces[id] = Some(Box::new(component));
        ifaces_mut[id] = Some(Arc::new(overlay));
    }

    Ok(AdaptedInterfaces377 {
        ifaces,
        ifaces_mut,
        type1_fields,
        null_string_masks,
    })
}

fn interface_null_string_mask_377(source: &IfType377) -> u8 {
    let mut mask = 0;
    if source.text.is_none() {
        mask |= INTERFACE_NULL_TEXT_377;
    }
    if source.active_text.is_none() {
        mask |= INTERFACE_NULL_ACTIVE_TEXT_377;
    }
    if source.target_verb.is_none() {
        mask |= INTERFACE_NULL_TARGET_VERB_377;
    }
    if source.target_text.is_none() {
        mask |= INTERFACE_NULL_TARGET_TEXT_377;
    }
    if source.option.is_none() {
        mask |= INTERFACE_NULL_OPTION_377;
    }
    if source.iop.is_none() {
        mask |= INTERFACE_NULL_IOP_377;
    }
    mask
}

fn into_interface_parts(source: &IfType377) -> (IfType, IfTypeMut) {
    let component = IfType {
        id: source.id,
        layer_id: source.layer,
        r#type: source.r#type,
        client_code: source.client_code,
        width: source.width,
        height: source.height,
        trans: source.trans,
        over_layer_id: source.overlayer,
        scripts: source.scripts.clone(),
        script_comparator: source.script_comparator.clone(),
        script_operand: source.script_operand.clone(),
        children: source.children.clone(),
        child_x: source.child_x.clone(),
        child_y: source.child_y.clone(),
        obj_swap: source.draggable,
        obj_ops: source.interactable,
        obj_use: source.usable,
        obj_replace: source.swappable,
        margin_x: source.margin_x,
        margin_y: source.margin_y,
        inv_background_x: source.inv_slot_offset_x.clone(),
        inv_background_y: source.inv_slot_offset_y.clone(),
        inv_background_name: source.inv_slot_graphic.as_ref().map(|graphics| {
            graphics
                .iter()
                .map(|graphic| graphic.as_ref().map(sprite_ref_name))
                .collect()
        }),
        iop: source.iop.clone().unwrap_or_default(),
        fill: source.fill,
        centre: source.center,
        font: source.font_index,
        shadow: source.shadowed,
        text2: source.active_text.clone().unwrap_or_default(),
        colour2: source.active_colour,
        colour_over: source.over_colour,
        colour2_over: source.active_over_colour,
        model2_type: source.active_model_type,
        model2_id: source.active_model,
        model_anim2: source.active_anim,
        graphic2_name: source
            .active_graphic
            .as_ref()
            .map(sprite_ref_name)
            .unwrap_or_default(),
        target_verb: source.target_verb.clone().unwrap_or_default(),
        target_base: source.target_text.clone().unwrap_or_default(),
        target_mask: source.target_mask,
        button_text: source.option.clone().unwrap_or_default(),
    };
    let overlay = IfTypeMut {
        hide: source.hide,
        link_obj_type: source.inv_slot_obj_id.clone(),
        link_obj_number: source.inv_slot_obj_count.clone(),
        text: source.text.clone().unwrap_or_default(),
        button_type: source.button_type,
        scroll_height: source.scroll,
        model1_type: source.model_type,
        model1_id: source.model,
        model_xan: source.xan,
        model_yan: source.yan,
        model_zoom: source.zoom,
        model_anim: source.anim,
        graphic_name: source
            .graphic
            .as_ref()
            .map(sprite_ref_name)
            .unwrap_or_default(),
        colour: source.colour,
        ..IfTypeMut::default()
    };

    (component, overlay)
}

fn sprite_ref_name(sprite: &SpriteRef377) -> String {
    format!("{},{}", sprite.name, sprite.index)
}

fn raw_bytes_to_string(bytes: Option<Vec<u8>>) -> String {
    bytes
        .unwrap_or_default()
        .into_iter()
        .map(char::from)
        .collect()
}

fn into_npc_parts(source: NpcType377) -> (NpcType, NpcFields377) {
    let NpcType377 {
        id,
        name,
        desc,
        size,
        model,
        head,
        readyanim,
        walkanim,
        walkanim_b,
        walkanim_r,
        walkanim_l,
        recol_s,
        recol_d,
        op,
        minimap,
        vislevel,
        resizeh,
        resizev,
        alwaysontop,
        ambient,
        contrast,
        headicon,
        turnspeed,
        field_1451,
        field_1430,
        field_1440,
        field_1434,
        morph_varbit,
        morph_varp,
        morphs,
    } = source;
    let runtime = NpcType {
        id,
        name,
        desc: raw_bytes_to_string(desc),
        size,
        model,
        head,
        readyanim,
        walkanim,
        walkanim_b,
        walkanim_r,
        walkanim_l,
        recol_s,
        recol_d,
        op: op.map_or_else(Vec::new, |actions| actions.into_iter().collect()),
        minimap,
        vislevel,
        resizeh,
        resizev,
        alwaysontop,
        ambient,
        contrast,
        headicon,
        turnspeed,
    };
    let fields = NpcFields377 {
        field_1451,
        field_1430,
        field_1440,
        field_1434,
        morph: morphs.map(|targets| MorphTable377 {
            varbit: morph_varbit,
            varp: morph_varp,
            targets,
        }),
    };
    (runtime, fields)
}

fn into_loc_parts(source: LocType377) -> (LocType, LocFields377) {
    let LocType377 {
        id,
        model,
        shape,
        name,
        desc,
        recol_s,
        recol_d,
        width,
        length,
        blockwalk,
        blockrange,
        active,
        hillskew,
        sharelight,
        occlude,
        anim,
        wallwidth,
        ambient,
        contrast,
        op,
        mapfunction,
        mapscene,
        mirror,
        shadow,
        resizex,
        resizey,
        resizez,
        offsetx,
        offsety,
        offsetz,
        forceapproach,
        forcedecor,
        breakroutefinding,
        raiseobject,
        morph_varbit,
        morph_varp,
        morphs,
    } = source;
    let runtime = LocType {
        id,
        model,
        shape,
        name,
        desc: raw_bytes_to_string(desc),
        recol_s,
        recol_d,
        width,
        length,
        blockwalk,
        blockrange,
        active,
        hillskew,
        sharelight,
        occlude,
        anim,
        wallwidth,
        ambient,
        contrast,
        op: op.map_or_else(Vec::new, |actions| actions.into_iter().collect()),
        mapfunction,
        mapscene,
        mirror,
        shadow,
        resizex,
        resizey,
        resizez,
        offsetx,
        offsety,
        offsetz,
        forceapproach,
        forcedecor,
        breakroutefinding,
        raiseobject,
    };
    let fields = LocFields377 {
        morph: morphs.map(|targets| MorphTable377 {
            varbit: morph_varbit,
            varp: morph_varp,
            targets,
        }),
    };
    (runtime, fields)
}

fn into_varp_parts(source: VarpType377) -> (VarpType, VarpFields377) {
    let VarpType377 {
        code1,
        code2,
        tracked,
        code4,
        clientcode,
        code6,
        code7,
        scope,
        transmit,
        code12,
        code14,
        debugname,
    } = source;
    (
        VarpType { clientcode },
        VarpFields377 {
            code1,
            code2,
            tracked,
            code4,
            code6,
            code7,
            scope,
            transmit,
            code12,
            code14,
            debugname,
        },
    )
}

fn into_varbit_parts(source: VarbitType377) -> (VarBitType, VarbitFields377) {
    let VarbitType377 {
        basevar,
        startbit,
        endbit,
        transmit,
        code3,
        code4,
        code5,
        debugname,
    } = source;
    (
        VarBitType {
            basevar,
            startbit,
            endbit,
            debugname: debugname.unwrap_or_default(),
        },
        VarbitFields377 {
            transmit,
            code3,
            code4,
            code5,
        },
    )
}

fn apply_certificates_377(objs: &mut [ObjType]) -> Result<(), String> {
    let mut state = vec![0u8; objs.len()];
    for id in 0..objs.len() {
        resolve_certificate_377(id, objs, &mut state)?;
    }
    Ok(())
}

fn resolve_certificate_377(
    id: usize,
    objs: &mut [ObjType],
    state: &mut [u8],
) -> Result<(), String> {
    match state.get(id).copied() {
        Some(2) => return Ok(()),
        Some(1) => return Err(format!("object certificate cycle at id {id}")),
        Some(0) => {}
        _ => return Err(format!("object certificate id {id} is out of range")),
    }
    state[id] = 1;

    let (template_id, link_id) = (objs[id].certtemplate, objs[id].certlink);
    if template_id != -1 {
        let template_index = usize::try_from(template_id)
            .ok()
            .filter(|&index| index < objs.len())
            .ok_or_else(|| {
                format!("object {id} certificate template {template_id} is out of range")
            })?;
        let link_index = usize::try_from(link_id)
            .ok()
            .filter(|&index| index < objs.len())
            .ok_or_else(|| format!("object {id} certificate link {link_id} is out of range"))?;
        resolve_certificate_377(template_index, objs, state)?;
        resolve_certificate_377(link_index, objs, state)?;

        let template = objs[template_index].clone();
        let link = objs[link_index].clone();
        apply_certificate_fields_377(&mut objs[id], &template, &link)?;
    }
    state[id] = 2;
    Ok(())
}

fn apply_certificate_fields_377(
    certificate: &mut ObjType,
    template: &ObjType,
    link: &ObjType,
) -> Result<(), String> {
    let first = link
        .name
        .chars()
        .next()
        .ok_or_else(|| "certificate link has an empty name".to_owned())?;
    // Client-Java 377 ObjType.method224 checks uppercase ASCII vowels.
    let article = if matches!(first, 'A' | 'E' | 'I' | 'O' | 'U') {
        "an"
    } else {
        "a"
    };

    certificate.model = template.model;
    certificate.zoom2d = template.zoom2d;
    certificate.xan2d = template.xan2d;
    certificate.yan2d = template.yan2d;
    certificate.zan2d = template.zan2d;
    certificate.xof2d = template.xof2d;
    certificate.yof2d = template.yof2d;
    certificate.recol_s.clone_from(&template.recol_s);
    certificate.recol_d.clone_from(&template.recol_d);
    certificate.name.clone_from(&link.name);
    certificate.members = link.members;
    certificate.cost = link.cost;
    certificate.desc = format!("Swap this note at any bank for {article} {}.", link.name);
    certificate.stackable = true;
    Ok(())
}

impl From<ObjType377> for ObjType {
    fn from(source: ObjType377) -> Self {
        let ObjType377 {
            id,
            model,
            name,
            desc,
            recol_s,
            recol_d,
            zoom2d,
            xan2d,
            yan2d,
            zan2d,
            xof2d,
            yof2d,
            stackable,
            cost,
            members,
            op,
            iop,
            manwear,
            manwear2,
            manwear_offset,
            womanwear,
            womanwear2,
            womanwear_offset,
            manwear3,
            womanwear3,
            manhead,
            womanhead,
            manhead2,
            womanhead2,
            countobj,
            countco,
            certlink,
            certtemplate,
            resizex,
            resizey,
            resizez,
            ambient,
            contrast,
            team,
            field_854: _,
        } = source;
        Self {
            id,
            model,
            name: name.unwrap_or_default(),
            desc: raw_bytes_to_string(desc),
            recol_s,
            recol_d,
            zoom2d,
            xan2d,
            yan2d,
            zan2d,
            xof2d,
            yof2d,
            stackable,
            cost,
            members,
            op: op.unwrap_or_default(),
            iop: iop.unwrap_or_default(),
            manwear,
            manwear2,
            manwear_offset,
            womanwear,
            womanwear2,
            womanwear_offset,
            manwear3,
            womanwear3,
            manhead,
            manhead2,
            womanhead,
            womanhead2,
            countobj: countobj.map(Vec::from),
            countco: countco.map(Vec::from),
            certlink,
            certtemplate,
            resizex,
            resizey,
            resizez,
            ambient,
            contrast,
            team,
        }
    }
}

impl From<NpcType377> for NpcType {
    fn from(source: NpcType377) -> Self {
        into_npc_parts(source).0
    }
}

impl From<LocType377> for LocType {
    fn from(source: LocType377) -> Self {
        into_loc_parts(source).0
    }
}

impl From<SeqType377> for SeqType {
    fn from(source: SeqType377) -> Self {
        let SeqType377 {
            num_frames,
            frames,
            iframes,
            delay,
            loops,
            walkmerge,
            reachforward,
            priority,
            replaceheldleft,
            replaceheldright,
            maxloops,
            preanim_move,
            postanim_move,
            duplicatebehaviour,
            reserved: _,
        } = source;
        Self {
            num_frames,
            frames: Some(frames),
            iframes: Some(iframes),
            delay: Some(delay),
            loops,
            walkmerge,
            reachforward,
            priority,
            replaceheldleft,
            replaceheldright,
            maxloops,
            preanim_move,
            postanim_move,
            duplicatebehaviour,
        }
    }
}

impl From<FloType377> for FloType {
    fn from(source: FloType377) -> Self {
        Self {
            colour: source.colour,
            texture: source.texture,
            overlay: source.overlay,
            occlude: source.occlude,
            debugname: source.debugname.unwrap_or_default(),
            hue: source.hue,
            saturation: source.saturation,
            lightness: source.lightness,
            chroma: source.chroma,
            underlay_hue: source.underlay_hue,
            overlay_hsl: source.overlay_hsl,
        }
    }
}

impl From<IdkType377> for IdkType {
    fn from(source: IdkType377) -> Self {
        Self {
            part: source.r#type,
            model: source.models,
            recol_s: source.recol_s,
            recol_d: source.recol_d,
            head: source.heads,
            disable: source.disable,
        }
    }
}

impl From<SpotType377> for SpotType {
    fn from(source: SpotType377) -> Self {
        Self {
            id: source.index as i32,
            model: source.model,
            anim: source.anim,
            seq: source.seq,
            recol_s: source.recol_s.map(|colour| colour as u16),
            recol_d: source.recol_d.map(|colour| colour as u16),
            resizeh: source.resizeh,
            resizev: source.resizev,
            angle: source.orientation,
            ambient: source.ambient,
            contrast: source.contrast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_interfaces() -> InterfaceArchive377 {
        InterfaceArchive377::scan(&[0, 0]).unwrap()
    }

    #[test]
    fn config_conversion_preserves_runtime_fields_certificates_and_377_extras() {
        let template = ObjType377 {
            id: 0,
            model: 42,
            name: Some("Template".to_owned()),
            recol_s: Some(vec![1]),
            recol_d: Some(vec![2]),
            zoom2d: 900,
            ..ObjType377::default()
        };
        let link = ObjType377 {
            id: 1,
            name: Some("Apple".to_owned()),
            cost: 17,
            members: true,
            ..ObjType377::default()
        };
        let certificate = ObjType377 {
            id: 2,
            certlink: 1,
            certtemplate: 0,
            field_854: 765,
            ..ObjType377::default()
        };
        let npc = NpcType377 {
            id: 0,
            name: "Morphing guard".to_owned(),
            desc: Some(vec![b'A', 0xe9]),
            model: Some(vec![12, 13]),
            field_1451: 14,
            field_1430: 15,
            field_1440: 16,
            field_1434: false,
            morph_varbit: 3,
            morph_varp: 4,
            morphs: Some(vec![8, -1, 9]),
            ..NpcType377::default()
        };
        let loc = LocType377 {
            id: 0,
            model: Some(vec![21]),
            shape: Some(vec![10]),
            morph_varbit: 5,
            morph_varp: -1,
            morphs: Some(vec![22, -1]),
            ..LocType377::default()
        };
        let sequence = SeqType377 {
            num_frames: 2,
            frames: vec![32_768, 65_535],
            iframes: vec![-1, 7],
            delay: vec![2, 400],
            loops: 11,
            walkmerge: Some(vec![1, 9_999_999]),
            reachforward: true,
            priority: 12,
            replaceheldleft: 13,
            replaceheldright: 14,
            maxloops: 15,
            preanim_move: 2,
            postanim_move: 1,
            duplicatebehaviour: 2,
            reserved: 0x1234,
        };
        let varp = VarpType377 {
            code1: 1,
            code2: 2,
            tracked: true,
            clientcode: 23,
            code7: 0x1020_3040,
            scope: 2,
            transmit: true,
            debugname: Some("tracked".to_owned()),
            ..VarpType377::default()
        };
        let varbit = VarbitType377 {
            basevar: 0,
            startbit: 4,
            endbit: 7,
            transmit: true,
            code3: 31,
            code4: 32,
            code5: false,
            debugname: Some("range".to_owned()),
        };
        let decoded = Cache377 {
            seqs: vec![sequence],
            locs: vec![loc],
            flos: vec![FloType377 {
                colour: 0x336699,
                texture: 4,
                overlay: true,
                occlude: false,
                debugname: Some("ground".to_owned()),
                hue: 17,
                saturation: 23,
                lightness: 41,
                underlay_hue: 51,
                chroma: 61,
                overlay_hsl: 71,
            }],
            objs: vec![template, link, certificate],
            npcs: vec![npc],
            idks: vec![IdkType377 {
                r#type: 6,
                models: Some(vec![31, 32]),
                recol_s: [1, 2, 3, 4, 5, 6],
                recol_d: [7, 8, 9, 10, 11, 12],
                heads: [13, 14, -1, -1, -1],
                disable: true,
            }],
            spots: vec![SpotType377 {
                index: 0,
                model: 41,
                anim: 0,
                seq: Some(0),
                recol_s: [1, 2, 3, 4, 5, 6],
                recol_d: [11, 12, 13, 14, 15, 16],
                resizeh: 128,
                resizev: 160,
                orientation: 512,
                ambient: 7,
                contrast: 8,
            }],
            varps: vec![varp],
            varbits: vec![varbit],
            tracked_varps: vec![0, 0],
            coverage: Vec::new(),
        };

        let loaded = adapt_cache_377(decoded, empty_interfaces()).unwrap();
        assert_eq!(
            loaded.cache.seqs[0].frames.as_deref(),
            Some(&[32_768, 65_535][..])
        );
        assert_eq!(loaded.cache.seqs[0].iframes.as_deref(), Some(&[-1, 7][..]));
        assert_eq!(loaded.cache.seqs[0].delay.as_deref(), Some(&[2, 400][..]));
        assert_eq!(loaded.cache.seqs[0].duplicatebehaviour, 2);
        assert_eq!(loaded.cache.locs[0].model.as_deref(), Some(&[21][..]));
        assert_eq!(loaded.cache.locs[0].shape.as_deref(), Some(&[10][..]));
        assert_eq!(loaded.cache.npcs[0].desc, "Aé");
        assert_eq!(loaded.cache.npcs[0].model.as_deref(), Some(&[12, 13][..]));
        assert_eq!(loaded.cache.flos[0].debugname, "ground");
        assert_eq!(loaded.cache.idks[0].part, 6);
        assert_eq!(loaded.cache.idks[0].head, [13, 14, -1, -1, -1]);
        assert_eq!(loaded.cache.spots[0].angle, 512);
        assert_eq!(loaded.cache.spots[0].resizev, 160);
        assert_eq!(loaded.cache.varps[0].clientcode, 23);
        assert_eq!(loaded.cache.varbits[0].basevar, 0);
        assert_eq!(loaded.cache.varbits[0].debugname, "range");

        let certificate = &loaded.cache.objs[2];
        assert_eq!(certificate.model, 42);
        assert_eq!(certificate.zoom2d, 900);
        assert_eq!(certificate.name, "Apple");
        assert_eq!(certificate.desc, "Swap this note at any bank for an Apple.");
        assert!(certificate.stackable);
        assert!(certificate.members);
        assert_eq!(certificate.cost, 17);
        assert_eq!(certificate.recol_s.as_deref(), Some(&[1][..]));
        assert_eq!(loaded.cache_fields_377.objs[2].field_854, 765);
        assert!(!loaded.cache_fields_377.npcs[0].field_1434);
        assert_eq!(loaded.cache_fields_377.npcs[0].field_1451, 14);
        assert_eq!(
            loaded.cache_fields_377.npcs[0]
                .morph
                .as_ref()
                .unwrap()
                .targets,
            [8, -1, 9]
        );
        assert_eq!(
            loaded.cache_fields_377.locs[0]
                .morph
                .as_ref()
                .unwrap()
                .varbit,
            5
        );
        assert_eq!(loaded.cache_fields_377.seqs[0].reserved, 0x1234);
        assert!(loaded.cache_fields_377.varps[0].tracked);
        assert_eq!(
            loaded.cache_fields_377.varps[0].debugname.as_deref(),
            Some("tracked")
        );
        assert!(loaded.cache_fields_377.varbits[0].transmit);
        assert_eq!(loaded.cache_fields_377.tracked_varps, [0, 0]);
    }

    fn put_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn put_i16(out: &mut Vec<u8>, value: i16) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn put_i32(out: &mut Vec<u8>, value: i32) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn put_string(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(value.as_bytes());
        out.push(10);
    }

    #[derive(Default)]
    struct ComponentHeader<'a> {
        id: u16,
        layer: Option<u16>,
        component_type: u8,
        button_type: u8,
        client_code: u16,
        width: u16,
        height: u16,
        trans: u8,
        overlayer: Option<u16>,
        script: Option<&'a [u16]>,
    }

    fn put_component_header(out: &mut Vec<u8>, header: ComponentHeader<'_>) {
        let ComponentHeader {
            id,
            layer,
            component_type,
            button_type,
            client_code,
            width,
            height,
            trans,
            overlayer,
            script,
        } = header;
        let overlayer = overlayer.map_or(-1, i32::from);
        if let Some(layer) = layer {
            put_u16(out, u16::MAX);
            put_u16(out, layer);
        }
        put_u16(out, id);
        out.push(component_type);
        out.push(button_type);
        put_u16(out, client_code);
        put_u16(out, width);
        put_u16(out, height);
        out.push(trans);
        if overlayer < 0 {
            out.push(0);
        } else {
            out.push(((overlayer >> 8) + 1) as u8);
            out.push(overlayer as u8);
        }
        out.push(0); // no comparator/operand pairs
        if let Some(words) = script {
            out.push(1);
            put_u16(out, words.len() as u16);
            for &word in words {
                put_u16(out, word);
            }
        } else {
            out.push(0);
        }
    }

    fn interface_fixture() -> Vec<u8> {
        let mut data = Vec::new();
        put_u16(&mut data, 10); // declared sparse ID space

        // Root layer, with a child and a 377 client-code layer side effect.
        put_component_header(
            &mut data,
            ComponentHeader {
                id: 1,
                layer: Some(400),
                client_code: 600,
                width: 100,
                height: 50,
                ..ComponentHeader::default()
            },
        );
        put_u16(&mut data, 300);
        data.push(1);
        put_u16(&mut data, 1);
        put_u16(&mut data, 2);
        put_i16(&mut data, -5);
        put_i16(&mut data, 7);

        // Inventory component with four zero-initialized slots and a frame.
        put_component_header(
            &mut data,
            ComponentHeader {
                id: 2,
                component_type: 2,
                button_type: 2,
                width: 2,
                height: 2,
                trans: 255,
                overlayer: Some(513),
                script: Some(&[20, 321, 0]),
                ..ComponentHeader::default()
            },
        );
        data.extend_from_slice(&[1, 1, 0, 1]); // draggable/interactable/usable/swappable
        data.extend_from_slice(&[3, 4]); // slot margins
        for slot in 0..20 {
            if slot == 0 {
                data.push(1);
                put_i16(&mut data, -2);
                put_i16(&mut data, 5);
                put_string(&mut data, "inv,8");
            } else {
                data.push(0);
            }
        }
        for action in ["Use", "", "", "Drop", ""] {
            put_string(&mut data, action);
        }
        put_string(&mut data, "Cast");
        put_string(&mut data, "item");
        put_u16(&mut data, 0x1234);

        // Text component: base and active text, font, colours, and OK option.
        put_component_header(
            &mut data,
            ComponentHeader {
                id: 4,
                component_type: 4,
                button_type: 1,
                width: 80,
                height: 20,
                ..ComponentHeader::default()
            },
        );
        data.extend_from_slice(&[1, 3, 1]); // centered, font index, shadow
        put_string(&mut data, "base text");
        put_string(&mut data, "");
        put_i32(&mut data, 0x11223344);
        put_i32(&mut data, 0x22334455);
        put_i32(&mut data, 0x33445566);
        put_i32(&mut data, 0x44556677);
        put_string(&mut data, "");

        // Graphic component with two media references.
        put_component_header(
            &mut data,
            ComponentHeader {
                id: 5,
                component_type: 5,
                width: 10,
                height: 10,
                ..ComponentHeader::default()
            },
        );
        put_string(&mut data, "ui,2");
        put_string(&mut data, "ui_active,4");

        // Model component with independent model/animation pairs.
        put_component_header(
            &mut data,
            ComponentHeader {
                id: 6,
                component_type: 6,
                width: 30,
                height: 30,
                ..ComponentHeader::default()
            },
        );
        data.extend_from_slice(&[3, 5, 2, 3, 2, 3, 0]);
        put_u16(&mut data, 1200);
        put_u16(&mut data, 64);
        put_u16(&mut data, 128);

        // 377-only type 8 text and type 1 attributes.
        put_component_header(
            &mut data,
            ComponentHeader {
                id: 8,
                component_type: 8,
                width: 40,
                height: 12,
                ..ComponentHeader::default()
            },
        );
        put_string(&mut data, "tooltip text");
        put_component_header(
            &mut data,
            ComponentHeader {
                id: 9,
                component_type: 1,
                width: 10,
                height: 10,
                ..ComponentHeader::default()
            },
        );
        put_u16(&mut data, 0x4567);
        data.push(1);
        data.extend_from_slice(&[0, 2, 0]);
        put_i32(&mut data, 0x55667788);

        data
    }

    #[test]
    fn interface_conversion_preserves_sparse_slots_text_sprites_and_inventory_capacity() {
        let archive = InterfaceArchive377::scan(&interface_fixture()).unwrap();
        let loaded = adapt_cache_377(
            Cache377 {
                seqs: Vec::new(),
                locs: Vec::new(),
                flos: Vec::new(),
                objs: Vec::new(),
                npcs: Vec::new(),
                idks: Vec::new(),
                spots: Vec::new(),
                varps: Vec::new(),
                varbits: Vec::new(),
                tracked_varps: Vec::new(),
                coverage: Vec::new(),
            },
            archive,
        )
        .unwrap();

        assert_eq!(loaded.ifaces.len(), 10);
        assert!(loaded.ifaces[0].is_none());
        assert!(loaded.ifaces[3].is_none());
        assert!(loaded.ifaces[7].is_none());
        assert_eq!(
            loaded.interface_fields_377.client_code_layers.field_728,
            400
        );
        assert_eq!(
            loaded.interface_fields_377.null_string_masks[1],
            INTERFACE_NULL_TEXT_377
                | INTERFACE_NULL_ACTIVE_TEXT_377
                | INTERFACE_NULL_TARGET_VERB_377
                | INTERFACE_NULL_TARGET_TEXT_377
                | INTERFACE_NULL_OPTION_377
                | INTERFACE_NULL_IOP_377
        );

        let root = loaded.ifaces[1].as_ref().unwrap();
        let root_mut = loaded.ifaces_mut[1].as_ref().unwrap();
        assert_eq!(root.layer_id, 400);
        assert_eq!(root.children.as_deref(), Some(&[2][..]));
        assert_eq!(root.child_x.as_deref(), Some(&[-5][..]));
        assert_eq!(root_mut.scroll_height, 300);
        assert!(root_mut.hide);

        let inventory = loaded.ifaces[2].as_ref().unwrap();
        let inventory_mut = loaded.ifaces_mut[2].as_ref().unwrap();
        assert_eq!(inventory.r#type, 2);
        assert_eq!(inventory.trans, -1);
        assert_eq!(inventory.over_layer_id, 513);
        assert_eq!(inventory.scripts.as_ref().unwrap()[0], [20, 321, 0]);
        assert_eq!(inventory.inv_background_x.as_ref().unwrap()[0], -2);
        assert_eq!(inventory.inv_background_y.as_ref().unwrap()[0], 5);
        assert_eq!(
            inventory.inv_background_name.as_ref().unwrap()[0].as_deref(),
            Some("inv,8")
        );
        assert_eq!(inventory.iop[0].as_deref(), Some("Use"));
        assert_eq!(inventory.target_verb, "Cast");
        assert_eq!(inventory.target_base, "item");
        assert_eq!(inventory.target_mask, 0x1234);
        assert!(inventory.obj_swap);
        assert!(inventory.obj_ops);
        assert!(!inventory.obj_use);
        assert!(inventory.obj_replace);
        assert_eq!(inventory.margin_x, 3);
        assert_eq!(inventory.margin_y, 4);
        assert_eq!(
            inventory_mut.link_obj_type.as_deref(),
            Some(&[0, 0, 0, 0][..])
        );
        assert_eq!(
            inventory_mut.link_obj_number.as_deref(),
            Some(&[0, 0, 0, 0][..])
        );
        assert_eq!(inventory_mut.button_type, 2);

        let text = loaded.ifaces[4].as_ref().unwrap();
        let text_mut = loaded.ifaces_mut[4].as_ref().unwrap();
        assert_eq!(text.text2, "");
        assert_eq!(text.colour2, 0x22334455);
        assert_eq!(text.colour_over, 0x33445566);
        assert_eq!(text.colour2_over, 0x44556677);
        assert_eq!(text.font, 3);
        assert_eq!(text_mut.text, "base text");
        assert_eq!(text.button_text, "Ok");
        assert_eq!(
            loaded.interface_fields_377.null_string_masks[4],
            INTERFACE_NULL_TARGET_VERB_377
                | INTERFACE_NULL_TARGET_TEXT_377
                | INTERFACE_NULL_IOP_377
        );

        let graphic = loaded.ifaces[5].as_ref().unwrap();
        let graphic_mut = loaded.ifaces_mut[5].as_ref().unwrap();
        assert_eq!(graphic.graphic2_name, "ui_active,4");
        assert_eq!(graphic_mut.graphic_name, "ui,2");

        let model = loaded.ifaces[6].as_ref().unwrap();
        let model_mut = loaded.ifaces_mut[6].as_ref().unwrap();
        assert_eq!((model_mut.model1_type, model_mut.model1_id), (1, 517));
        assert_eq!((model.model2_type, model.model2_id), (1, 259));
        assert_eq!(model_mut.model_anim, 259);
        assert_eq!(model.model_anim2, -1);
        assert_eq!(
            (
                model_mut.model_zoom,
                model_mut.model_xan,
                model_mut.model_yan
            ),
            (1200, 64, 128)
        );

        let type8 = loaded.ifaces[8].as_ref().unwrap();
        assert_eq!(type8.r#type, 8);
        assert_eq!(loaded.ifaces_mut[8].as_ref().unwrap().text, "tooltip text");
        assert_eq!(
            loaded.interface_fields_377.null_string_masks[8],
            INTERFACE_NULL_ACTIVE_TEXT_377
                | INTERFACE_NULL_TARGET_VERB_377
                | INTERFACE_NULL_TARGET_TEXT_377
                | INTERFACE_NULL_OPTION_377
                | INTERFACE_NULL_IOP_377
        );
        assert_eq!(
            loaded.interface_fields_377.type1_fields[9],
            Some(IfType1Fields377 {
                field_707: 0x4567,
                field_715: true,
            })
        );
    }
}
