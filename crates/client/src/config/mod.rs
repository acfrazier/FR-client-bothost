//! Config type tables, 1:1 of `~/experiments/Server/webclient/src/config/`.
//! Every `*Type::unpack` decodes one `config` jag into a table; the tables
//! live on the `Client`'s `Cache` (the spec keeps loaded `*Type`s per client,
//! not process-wide). The per-request model/sprite methods (`getModel`,
//! `getSprite`, ...) need `dash3d`/`graphics` and land with Tasks 14/15.

pub mod flo_type;
pub mod idk_type;
pub mod if_type;
pub mod loc_type;
pub mod npc_type;
pub mod obj_type;
pub mod seq_type;
pub mod spot_type;
pub mod varbit_type;
pub mod varp_type;

pub use flo_type::FloType;
pub use idk_type::IdkType;
pub use if_type::IfType;
pub use loc_type::LocType;
pub use npc_type::NpcType;
pub use obj_type::ObjType;
pub use seq_type::SeqType;
pub use spot_type::SpotType;
pub use varbit_type::VarBitType;
pub use varp_type::VarpType;

use crate::io::JagFile;

/// All config tables for one `Client`, replacing the TS class statics.
/// `list(id)` from client-ts becomes `cache.obj(id)` / `npc` / `loc` / ...
#[derive(Default)]
pub struct Cache {
    pub objs: Vec<ObjType>,
    pub npcs: Vec<NpcType>,
    pub locs: Vec<LocType>,
    pub flos: Vec<FloType>,
    pub idks: Vec<IdkType>,
    pub seqs: Vec<SeqType>,
    pub spots: Vec<SpotType>,
    pub varbits: Vec<VarBitType>,
    pub varps: Vec<VarpType>,
    pub ifaces: Vec<Option<IfType>>,
}

impl Cache {
    /// Load every table from a `config` jag (the TS `maininit` order: seq
    /// before spotanim, since `SpotType` links to loaded `SeqType`s).
    pub fn unpack(jag: &JagFile) -> Self {
        let seqs = SeqType::unpack(jag);
        let mut spots = SpotType::unpack(jag);
        for spot in &mut spots {
            spot.seq = (spot.anim >= 0 && (spot.anim as usize) < seqs.len())
                .then_some(spot.anim as usize);
        }
        Cache {
            objs: ObjType::unpack(jag),
            npcs: NpcType::unpack(jag),
            locs: LocType::unpack(jag),
            flos: FloType::unpack(jag),
            idks: IdkType::unpack(jag),
            seqs,
            spots,
            varbits: VarBitType::unpack(jag),
            varps: VarpType::unpack(jag),
            ifaces: IfType::unpack(jag),
        }
    }

    pub fn obj(&self, id: usize) -> &ObjType {
        &self.objs[id]
    }

    pub fn npc(&self, id: usize) -> &NpcType {
        &self.npcs[id]
    }

    pub fn loc(&self, id: usize) -> &LocType {
        &self.locs[id]
    }

    pub fn flo(&self, id: usize) -> &FloType {
        &self.flos[id]
    }

    pub fn idk(&self, id: usize) -> &IdkType {
        &self.idks[id]
    }

    pub fn seq(&self, id: usize) -> &SeqType {
        &self.seqs[id]
    }

    pub fn spot(&self, id: usize) -> &SpotType {
        &self.spots[id]
    }

    pub fn varbit(&self, id: usize) -> &VarBitType {
        &self.varbits[id]
    }

    pub fn varp(&self, id: usize) -> &VarpType {
        &self.varps[id]
    }

    pub fn if_(&self, id: usize) -> Option<&IfType> {
        self.ifaces.get(id).and_then(|o| o.as_ref())
    }
}
