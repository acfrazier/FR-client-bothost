//! Revision-377 startup config tables, separate from the 274/289 Cache.
//!
//! Source: Client-Java 377 `client/Client.java:2046-2055`. Indexed tables use
//! the original dat/idx record boundaries (LocType.java:198-207,
//! NpcType.java:289-302, ObjType.java:246-259). All records are decoded here,
//! rather than Java's twenty-entry loc/npc and ten-entry obj caches. Model building,
//! morph selection and rendering are not part of this archive decoder.

#[path = "decode_377.rs"]
pub mod decode_377;
#[path = "flo_type_377.rs"]
pub mod flo_type_377;
#[path = "idk_type_377.rs"]
pub mod idk_type_377;
#[path = "loc_type_377.rs"]
pub mod loc_type_377;
#[path = "npc_type_377.rs"]
pub mod npc_type_377;
#[path = "obj_type_377.rs"]
pub mod obj_type_377;
#[path = "seq_type_377.rs"]
pub mod seq_type_377;
#[path = "spot_type_377.rs"]
pub mod spot_type_377;
#[path = "varbit_type_377.rs"]
pub mod varbit_type_377;
#[path = "varp_type_377.rs"]
pub mod varp_type_377;

pub use decode_377::{DecodeError377, DecodeErrorKind377, Reader377};
pub use flo_type_377::FloType377;
pub use idk_type_377::IdkType377;
pub use loc_type_377::LocType377;
pub use npc_type_377::NpcType377;
pub use obj_type_377::ObjType377;
pub use seq_type_377::SeqType377;
pub use spot_type_377::SpotType377;
pub use varbit_type_377::VarbitType377;
pub use varp_type_377::VarpType377;

use crate::io::JagFile;
use crate::util::JavaRandom;

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigCoverage377 {
    pub member: &'static str,
    pub records: usize,
    pub bytes: usize,
}

#[derive(Debug)]
pub struct Cache377 {
    pub seqs: Vec<SeqType377>,
    pub locs: Vec<LocType377>,
    pub flos: Vec<FloType377>,
    /// Raw object definitions. Certificates and member filtering occur on get,
    /// not while decoding the archive (ObjType.java get() / toCertificate()).
    pub objs: Vec<ObjType377>,
    pub npcs: Vec<NpcType377>,
    pub idks: Vec<IdkType377>,
    pub spots: Vec<SpotType377>,
    pub varps: Vec<VarpType377>,
    pub varbits: Vec<VarbitType377>,
    pub tracked_varps: Vec<usize>,
    pub coverage: Vec<ConfigCoverage377>,
}

impl Cache377 {
    pub fn unpack(jag: &JagFile) -> Result<Self, DecodeError377> {
        let mut random = JavaRandom::now();
        Self::unpack_with_random(jag, &mut || random.next_double())
    }

    /// Identical decoding with caller-supplied [0,1) draws for floor jitter.
    pub fn unpack_with_random(
        jag: &JagFile,
        random: &mut impl FnMut() -> f64,
    ) -> Result<Self, DecodeError377> {
        Self::unpack_with_options(jag, false, random)
    }

    /// Preserve the Java low-memory model-list override without global state.
    pub fn unpack_with_options(
        jag: &JagFile,
        low_mem: bool,
        random: &mut impl FnMut() -> f64,
    ) -> Result<Self, DecodeError377> {
        let mut coverage = Vec::with_capacity(12);
        let seqs = sequential(jag, "seq.dat", &mut coverage, SeqType377::decode)?;
        let locs = indexed(jag, "loc.dat", "loc.idx", &mut coverage, |dat| {
            if low_mem {
                LocType377::decode_with_lowmem(dat, true)
            } else {
                LocType377::decode(dat)
            }
        })?;
        let flos = sequential(jag, "flo.dat", &mut coverage, |dat| {
            FloType377::decode(dat, random)
        })?;
        let objs = indexed(jag, "obj.dat", "obj.idx", &mut coverage, ObjType377::decode)?;
        let npcs = indexed(jag, "npc.dat", "npc.idx", &mut coverage, NpcType377::decode)?;
        let idks = sequential(jag, "idk.dat", &mut coverage, IdkType377::decode)?;
        let mut spots = sequential(jag, "spotanim.dat", &mut coverage, SpotType377::decode)?;
        for spot in &mut spots {
            if spot.anim >= 0 {
                let index = spot.anim as usize;
                if index >= seqs.len() {
                    return Err(DecodeError377 {
                        table: "spotanim.dat",
                        record: spot.index,
                        offset: 0,
                        kind: DecodeErrorKind377::InvalidValue("sequence reference"),
                    });
                }
                spot.seq = Some(index);
            }
        }
        let mut tracked_varps = Vec::new();
        let mut varps = sequential(jag, "varp.dat", &mut coverage, |dat| {
            VarpType377::decode(dat, &mut tracked_varps)
        })?;
        let varbits = sequential(jag, "varbit.dat", &mut coverage, VarbitType377::decode)?;
        // VarbitType.java:50-53 modifies VarpType.field1519 after each decode.
        for (id, varbit) in varbits.iter().enumerate() {
            if varbit.transmit {
                let varp = varps
                    .get_mut(varbit.basevar as usize)
                    .ok_or(DecodeError377 {
                        table: "varbit.dat",
                        record: id,
                        offset: 0,
                        kind: DecodeErrorKind377::InvalidValue("varp reference"),
                    })?;
                varp.transmit = true;
            }
        }
        Ok(Self {
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
            coverage,
        })
    }
}

fn member(jag: &JagFile, name: &'static str) -> Result<Vec<u8>, DecodeError377> {
    jag.read(name).ok_or(DecodeError377 {
        table: name,
        record: 0,
        offset: 0,
        kind: DecodeErrorKind377::MissingMember,
    })
}

fn sequential<T>(
    jag: &JagFile,
    name: &'static str,
    coverage: &mut Vec<ConfigCoverage377>,
    mut decode: impl FnMut(&mut Reader377<'_>) -> Result<T, DecodeError377>,
) -> Result<Vec<T>, DecodeError377> {
    let bytes = member(jag, name)?;
    let mut dat = Reader377::new(&bytes, name, 0);
    let count = dat.g2()? as usize;
    let mut entries = Vec::with_capacity(count);
    for id in 0..count {
        dat.record = id;
        entries.push(decode(&mut dat)?);
    }
    dat.finish()?;
    coverage.push(ConfigCoverage377 {
        member: name,
        records: count,
        bytes: dat.pos,
    });
    Ok(entries)
}

fn indexed<T>(
    jag: &JagFile,
    name: &'static str,
    index_name: &'static str,
    coverage: &mut Vec<ConfigCoverage377>,
    mut decode: impl FnMut(&mut Reader377<'_>) -> Result<T, DecodeError377>,
) -> Result<Vec<T>, DecodeError377> {
    let bytes = member(jag, name)?;
    let indices = member(jag, index_name)?;
    let mut dat = Reader377::new(&bytes, name, 0);
    let mut idx = Reader377::new(&indices, index_name, 0);
    let count = idx.g2()? as usize;
    if dat.g2()? as usize != count {
        return Err(dat.error(DecodeErrorKind377::InvalidValue("dat/idx count mismatch")));
    }
    let mut entries = Vec::with_capacity(count);
    for id in 0..count {
        idx.record = id;
        dat.record = id;
        let size = idx.g2()? as usize;
        let start = dat.pos;
        let record = dat.take(size)?;
        let mut buf = Reader377::new(record, name, id);
        let result = (|| {
            let entry = decode(&mut buf)?;
            buf.finish()?;
            Ok(entry)
        })();
        entries.push(result.map_err(|mut error: DecodeError377| {
            error.offset += start;
            error
        })?);
    }
    dat.finish()?;
    idx.finish()?;
    coverage.push(ConfigCoverage377 {
        member: name,
        records: count,
        bytes: dat.pos,
    });
    coverage.push(ConfigCoverage377 {
        member: index_name,
        records: count,
        bytes: idx.pos,
    });
    Ok(entries)
}
