//! Revision 289 offline cache/config loader seam.
//!
//! Distinguishes game-cache JAG archive names (login CRC slots) from the
//! client/deob JAR. Does **not** claim an authentic 289 game cache: the live
//! pairing remains an external gate. Offline tests may exercise this loader
//! with independently justified synthetic fixtures only.

use std::fs;
use std::path::Path;

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

/// Result of an offline config/interface load attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineCacheLoad {
    pub config_present: bool,
    pub interface_present: bool,
    /// True when config bytes parse as a JAG container (header + file table).
    pub config_jag_ok: bool,
    /// True when interface bytes parse as a JAG container.
    pub interface_jag_ok: bool,
    pub file_names_seen: Vec<String>,
}

/// Load offline config/interface JAGs from a directory. Fail-closed: missing
/// files yield `config_present=false` rather than inventing tables. Does not
/// run full `Cache::unpack` config decoders (those need authentic type
/// tables); it only validates JAG container structure for the seam tests.
pub fn load_offline_config_seam(dir: &Path) -> OfflineCacheLoad {
    let mut out = OfflineCacheLoad {
        config_present: false,
        interface_present: false,
        config_jag_ok: false,
        interface_jag_ok: false,
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
            }
        }
    }

    out
}

/// Build a minimal public-safe JAG container with named members. Each member
/// is bzip2-packed (Jagex omits the `BZh` header; we strip it after encode)
/// under an uncompressed archive header — the same layout real packs use.
/// Suitable only for offline seam tests — not game assets.
pub fn synthetic_jag(files: &[(&str, &[u8])]) -> Vec<u8> {
    let packed: Vec<Vec<u8>> = files.iter().map(|(_, d)| bz2_jagex(d)).collect();
    let data_len: usize = packed.iter().map(|d| d.len()).sum();
    let total = (8 + 10 * files.len() + data_len) as i32;
    let mut out = Vec::new();
    push_g3(&mut out, total);
    push_g3(&mut out, total); // packed == unpacked → archive-level raw; members bzip
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
    fn synthetic_jag_roundtrip_file_count() {
        let bytes = synthetic_jag(&[("flo.dat", &[1, 2, 3]), ("npc.dat", &[4])]);
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
    }

    #[test]
    fn offline_loader_synthetic_config() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("r289_cache_seam_{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        let cfg = synthetic_jag(&[("idk.dat", &[0, 0])]);
        fs::write(dir.join("config"), &cfg).unwrap();
        let load = load_offline_config_seam(&dir);
        assert!(load.config_present);
        assert!(load.config_jag_ok);
        assert!(!load.interface_present);
        let man = CacheManifest289::discover_offline(&dir, "synthetic stage3 fixture");
        assert!(man.has(CacheArchiveKind::Config));
        assert!(!man.authentic_cache_present);
        let _ = fs::remove_dir_all(&dir);
    }
}
