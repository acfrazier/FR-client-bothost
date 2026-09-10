//! Revision 289 offline cache/config loader helpers.
//!
//! Distinguishes game-cache JAG archive names (login CRC slots) from the
//! client/deob JAR. Does **not** claim an authentic 289 game cache: the live
//! pairing remains an external gate. Offline tests exercise production
//! `Cache::unpack` / `IfType::unpack` through `Client::new*` load paths with
//! independently justified synthetic fixtures only.
//!
//! JAG outer header: two g3 values are the packed/unpacked size of the
//! payload **after** the six-byte header (source JagFile layout). Member
//! entries still use per-file packed/unpacked sizes.

use std::fs;
use std::path::Path;

use crate::config::{Cache, IfType};
use super::jagfile::JagFile;

/// Login CRC slot layout for revision 289 matches the 274 applet contract:
/// slot 0 is unused (always 0 in the 9×g4 login checksum block); slots 1–8
/// correspond to the named JAG packs below. Primary Java still posts nine
/// `p4` checksums after the revision word (client.java login wrapper).
pub const CACHE_JAG_NAMES_289: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];

/// Logical archive kind for offline manifest rows. Not a claim that bytes
/// came from a live 289 world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheArchiveKind {
    Title,
    Config,
    Interface,
    Media,
    Versionlist,
    Textures,
    Wordenc,
    Sounds,
}

impl CacheArchiveKind {
    pub fn jag_name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Config => "config",
            Self::Interface => "interface",
            Self::Media => "media",
            Self::Versionlist => "versionlist",
            Self::Textures => "textures",
            Self::Wordenc => "wordenc",
            Self::Sounds => "sounds",
        }
    }

    /// Login CRC slot index (1–8). Slot 0 has no pack file.
    pub fn crc_slot(self) -> usize {
        match self {
            Self::Title => 1,
            Self::Config => 2,
            Self::Interface => 3,
            Self::Media => 4,
            Self::Versionlist => 5,
            Self::Textures => 6,
            Self::Wordenc => 7,
            Self::Sounds => 8,
        }
    }

    pub fn from_jag_name(name: &str) -> Option<Self> {
        match name {
            "title" => Some(Self::Title),
            "config" => Some(Self::Config),
            "interface" => Some(Self::Interface),
            "media" => Some(Self::Media),
            "versionlist" => Some(Self::Versionlist),
            "textures" => Some(Self::Textures),
            "wordenc" => Some(Self::Wordenc),
            "sounds" => Some(Self::Sounds),
            _ => None,
        }
    }
}

/// One offline fixture archive entry with provenance (not live cache proof).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheFixtureEntry {
    pub kind: CacheArchiveKind,
    pub path: String,
    /// Short provenance note: how the bytes were produced and why they are
    /// public-safe. Must not claim live-server authenticity.
    pub provenance: String,
    /// Optional SHA-256 hex of the fixture file when pinned.
    pub sha256_hex: Option<String>,
}

/// Offline 289 cache manifest: which synthetic archives are present and how
/// they were derived. Missing authentic cache is recorded, not invented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheManifest289 {
    pub entries: Vec<CacheFixtureEntry>,
    /// True only when an operator-supplied authentic 289 cache directory is
    /// wired; offline synthetic fixtures keep this false.
    pub authentic_cache_present: bool,
    pub notes: String,
}

impl CacheManifest289 {
    pub fn offline_empty(notes: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            authentic_cache_present: false,
            notes: notes.into(),
        }
    }

    pub fn jag_names(&self) -> &'static [&'static str; 8] {
        &CACHE_JAG_NAMES_289
    }

    /// Discover named JAG files under `dir` without unpacking. Does not mark
    /// the result authentic.
    pub fn discover_offline(dir: &Path, provenance: impl Into<String>) -> Self {
        let prov = provenance.into();
        let mut entries = Vec::new();
        for name in CACHE_JAG_NAMES_289 {
            let path = dir.join(name);
            if path.is_file() {
                if let Some(kind) = CacheArchiveKind::from_jag_name(name) {
                    entries.push(CacheFixtureEntry {
                        kind,
                        path: path.display().to_string(),
                        provenance: prov.clone(),
                        sha256_hex: None,
                    });
                }
            }
        }
        Self {
            entries,
            authentic_cache_present: false,
            notes: format!(
                "offline discovery under {}; authentic 289 cache pairing unverified",
                dir.display()
            ),
        }
    }

    pub fn has(&self, kind: CacheArchiveKind) -> bool {
        self.entries.iter().any(|e| e.kind == kind)
    }
}

/// Result of an offline config/interface load through production unpackers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineCacheLoad {
    pub config_present: bool,
    pub interface_present: bool,
    /// True when config bytes parse as a JAG container (header + file table).
    pub config_jag_ok: bool,
    /// True when interface bytes parse as a JAG container.
    pub interface_jag_ok: bool,
    /// True when `Cache::unpack` completed without panic on the config jag.
    pub config_unpack_ok: bool,
    /// True when `IfType::unpack` completed without panic on the interface jag.
    pub interface_unpack_ok: bool,
    /// Counts observed after production unpack (empty when unpack failed).
    pub flo_count: usize,
    pub varp_count: usize,
    pub idk_count: usize,
    pub iface_count: usize,
    pub file_names_seen: Vec<String>,
}

/// Load offline config/interface JAGs from a directory through the same
/// unpackers production `Client::load_cache` uses (`Cache::unpack`,
/// `IfType::unpack`). Fail-closed: missing files yield present=false rather
/// than inventing tables. Does not claim authentic game assets.
pub fn load_offline_config_seam(dir: &Path) -> OfflineCacheLoad {
    let mut out = OfflineCacheLoad {
        config_present: false,
        interface_present: false,
        config_jag_ok: false,
        interface_jag_ok: false,
        config_unpack_ok: false,
        interface_unpack_ok: false,
        flo_count: 0,
        varp_count: 0,
        idk_count: 0,
        iface_count: 0,
        file_names_seen: Vec::new(),
    };

    let config_path = dir.join("config");
    if config_path.is_file() {
        out.config_present = true;
        if let Ok(bytes) = fs::read(&config_path) {
            if let Ok(jag) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| JagFile::new(bytes)))
            {
                out.config_jag_ok = jag.file_count >= 0;
                out.file_names_seen
                    .push(format!("config:files={}", jag.file_count));
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Cache::unpack(&jag)))
                {
                    Ok(cache) => {
                        out.config_unpack_ok = true;
                        out.flo_count = cache.flos.len();
                        out.varp_count = cache.varps.len();
                        out.idk_count = cache.idks.len();
                    }
                    Err(_) => out.config_unpack_ok = false,
                }
            }
        }
    }

    let iface_path = dir.join("interface");
    if iface_path.is_file() {
        out.interface_present = true;
        if let Ok(bytes) = fs::read(&iface_path) {
            if let Ok(jag) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| JagFile::new(bytes)))
            {
                out.interface_jag_ok = jag.file_count >= 0;
                out.file_names_seen
                    .push(format!("interface:files={}", jag.file_count));
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    IfType::unpack(&jag)
                })) {
                    Ok((ifaces, _)) => {
                        out.interface_unpack_ok = true;
                        out.iface_count = ifaces.iter().filter(|c| c.is_some()).count();
                    }
                    Err(_) => out.interface_unpack_ok = false,
                }
            }
        }
    }

    out
}

/// Tiny source-shaped config members for offline production unpack tests.
///
/// Each `*.dat` uses the primary decode loop (g2 count, then TLV records
/// ending at code 0). Values are synthetic public-safe oracles — not live
/// 289 world definitions. Obj/npc/loc omit idx pairs so those tables stay
/// empty (production unpack returns empty when idx is absent).
pub fn synthetic_config_members() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        // flo.dat: 1 floor, colour code 1 + g3 RGB, terminator 0
        ("flo.dat", vec![0x00, 0x01, 0x01, 0xFF, 0x00, 0x00, 0x00]),
        // varp.dat: 1 varp, clientcode 5 + g2=7, terminator 0
        ("varp.dat", vec![0x00, 0x01, 0x05, 0x00, 0x07, 0x00]),
        // idk.dat: 1 identity kit with only terminator (defaults)
        ("idk.dat", vec![0x00, 0x01, 0x00]),
        // empty count tables still present so jag file list is realistic
        ("seq.dat", vec![0x00, 0x00]),
        ("spotanim.dat", vec![0x00, 0x00]),
        ("varbit.dat", vec![0x00, 0x00]),
    ]
}

/// Minimal interface `data` blob: one TYPE_RECT component (id 1).
/// Field order matches `IfType::unpack` common header + RECT branches.
pub fn synthetic_interface_data() -> Vec<u8> {
    let mut d = Vec::new();
    // count (informational; decoder walks until EOF)
    d.extend_from_slice(&1u16.to_be_bytes());
    // id = 1
    d.extend_from_slice(&1u16.to_be_bytes());
    d.push(3); // TYPE_RECT
    d.push(0); // button_type
    d.extend_from_slice(&0u16.to_be_bytes()); // client_code
    d.extend_from_slice(&10u16.to_be_bytes()); // width
    d.extend_from_slice(&10u16.to_be_bytes()); // height
    d.push(0); // trans
    d.push(0); // over_layer → -1
    d.push(0); // script_stack_count
    d.push(0); // script_count
    d.push(0); // fills
    d.extend_from_slice(&0u32.to_be_bytes()); // colour
    d.extend_from_slice(&0u32.to_be_bytes()); // colour2
    d.extend_from_slice(&0u32.to_be_bytes()); // colour_over
    d.extend_from_slice(&0u32.to_be_bytes()); // colour2_over
    d
}

/// Write synthetic config (+ optional interface) JAGs under `dir` for offline
/// `Client::new*` / `load_offline_config_seam` production-path tests.
pub fn write_synthetic_cache_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let members = synthetic_config_members();
    let refs: Vec<(&str, &[u8])> = members
        .iter()
        .map(|(n, b)| (*n, b.as_slice()))
        .collect();
    fs::write(dir.join("config"), synthetic_jag(&refs))?;
    let iface = synthetic_interface_data();
    fs::write(
        dir.join("interface"),
        synthetic_jag(&[("data", iface.as_slice())]),
    )?;
    Ok(())
}

/// Build a minimal public-safe JAG container with named members. Each member
/// is bzip2-packed (Jagex omits the `BZh` header; we strip it after encode)
/// under an uncompressed archive header — the same layout real packs use.
///
/// Outer g3/g3 sizes are the payload length **excluding** the six-byte
/// header (`2 + 10*n + sum(packed)`), matching `JagFile::new` source layout.
pub fn synthetic_jag(files: &[(&str, &[u8])]) -> Vec<u8> {
    let packed: Vec<Vec<u8>> = files.iter().map(|(_, d)| bz2_jagex(d)).collect();
    let data_len: usize = packed.iter().map(|d| d.len()).sum();
    // Payload after the 6-byte outer header: g2 file_count + 10 bytes/file + data.
    let payload_len = (2 + 10 * files.len() + data_len) as i32;
    let mut out = Vec::new();
    push_g3(&mut out, payload_len);
    push_g3(&mut out, payload_len); // packed == unpacked → archive-level raw; members bzip
    out.push((files.len() >> 8) as u8);
    out.push(files.len() as u8);
    for ((name, data), packed_data) in files.iter().zip(packed.iter()) {
        out.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
        push_g3(&mut out, data.len() as i32);
        push_g3(&mut out, packed_data.len() as i32);
    }
    for d in &packed {
        out.extend_from_slice(d);
    }
    debug_assert_eq!(out.len(), 6 + payload_len as usize);
    out
}

fn bz2_jagex(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    enc.write_all(data).expect("bz2 encode");
    let out = enc.finish().expect("bz2 finish");
    // Jagex streams omit the standard "BZh<digit>" header.
    assert!(out.starts_with(b"BZh"));
    out[4..].to_vec()
}

fn push_g3(out: &mut Vec<u8>, value: i32) {
    out.push((value >> 16) as u8);
    out.push((value >> 8) as u8);
    out.push(value as u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn jag_names_match_login_crc_slots() {
        assert_eq!(CACHE_JAG_NAMES_289.len(), 8);
        assert_eq!(CacheArchiveKind::Config.crc_slot(), 2);
        assert_eq!(CacheArchiveKind::Config.jag_name(), "config");
        assert_eq!(
            CacheArchiveKind::from_jag_name("config"),
            Some(CacheArchiveKind::Config)
        );
        assert_eq!(CacheArchiveKind::from_jag_name("client.jar"), None);
    }

    #[test]
    fn synthetic_jag_header_excludes_outer_six() {
        let bytes = synthetic_jag(&[("flo.dat", &[1, 2, 3]), ("npc.dat", &[4])]);
        assert_eq!(bytes.len() >= 6, true);
        let payload = ((bytes[0] as i32) << 16) | ((bytes[1] as i32) << 8) | (bytes[2] as i32);
        let packed = ((bytes[3] as i32) << 16) | ((bytes[4] as i32) << 8) | (bytes[5] as i32);
        assert_eq!(payload, packed);
        assert_eq!(payload as usize, bytes.len() - 6);
        let jag = JagFile::new(bytes);
        assert_eq!(jag.file_count, 2);
        assert_eq!(jag.read("flo.dat").as_deref(), Some(&[1, 2, 3][..]));
        assert_eq!(jag.read("npc.dat").as_deref(), Some(&[4][..]));
    }

    #[test]
    fn offline_loader_missing_dir_fail_closed() {
        let load = load_offline_config_seam(Path::new("/tmp/no-such-289-cache-dir-xyz"));
        assert!(!load.config_present);
        assert!(!load.interface_present);
        assert!(!load.config_jag_ok);
        assert!(!load.config_unpack_ok);
    }

    #[test]
    fn offline_loader_unpacks_synthetic_config_and_interface() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("r289_cache_seam_{stamp}"));
        write_synthetic_cache_dir(&dir).unwrap();
        let load = load_offline_config_seam(&dir);
        assert!(load.config_present && load.config_jag_ok && load.config_unpack_ok);
        assert!(load.interface_present && load.interface_jag_ok && load.interface_unpack_ok);
        assert_eq!(load.flo_count, 1);
        assert_eq!(load.varp_count, 1);
        assert_eq!(load.idk_count, 1);
        assert_eq!(load.iface_count, 1);
        let man = CacheManifest289::discover_offline(&dir, "synthetic stage3 fixture");
        assert!(man.has(CacheArchiveKind::Config));
        assert!(man.has(CacheArchiveKind::Interface));
        assert!(!man.authentic_cache_present);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_unpack_direct_oracle() {
        let members = synthetic_config_members();
        let refs: Vec<(&str, &[u8])> = members.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let jag = JagFile::new(synthetic_jag(&refs));
        let cache = Cache::unpack(&jag);
        assert_eq!(cache.flos.len(), 1);
        assert_eq!(cache.flos[0].colour, 0xFF_00_00);
        assert_eq!(cache.varps.len(), 1);
        assert_eq!(cache.varps[0].clientcode, 7);
        assert_eq!(cache.idks.len(), 1);
        assert!(cache.objs.is_empty());
        assert!(cache.npcs.is_empty());
        assert!(cache.locs.is_empty());
    }
}
