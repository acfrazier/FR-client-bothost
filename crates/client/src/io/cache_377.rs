//! Revision-377 JAG selection and checksum admission.
//!
//! Client-Java `Client.java:1701-1735` keeps the eight archive names and CRC
//! slots used by 289. `Jagfile.java:33-84` also keeps the shared container
//! format, so this module reuses `JagFile` instead of copying its decoder.
//! Config and interface *member* decoders are separate revision-specific work.
//! This module neither calls the 274/289 unpackers nor installs revision dispatch.

use std::fmt;
use std::fs;
use std::path::Path;

use super::jagfile::JagFile;
use super::packet::Packet;

/// JAG names in login CRC order. Slot zero has no archive.
pub const CACHE_JAG_NAMES_377: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];

/// A JAG in idx0, not an OnDemand model/animation/midi/map archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum CacheArchive377 {
    Title,
    Config,
    Interface,
    Media,
    Versionlist,
    Textures,
    Wordenc,
    Sounds,
}

impl CacheArchive377 {
    pub const ALL: [Self; 8] = [
        Self::Title,
        Self::Config,
        Self::Interface,
        Self::Media,
        Self::Versionlist,
        Self::Textures,
        Self::Wordenc,
        Self::Sounds,
    ];

    pub const fn jag_name(self) -> &'static str {
        CACHE_JAG_NAMES_377[self as usize]
    }

    /// `Client.java:2719-2723` writes all nine CRCs after the revision word.
    pub const fn crc_slot(self) -> usize {
        self as usize + 1
    }

    pub fn from_jag_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.jag_name() == name)
    }
}

/// Offline archive admission failure. Missing files never become empty JAGs.
#[derive(Debug)]
pub enum CacheLoadError377 {
    Read {
        archive: CacheArchive377,
        source: std::io::Error,
    },
    Checksum {
        archive: CacheArchive377,
        expected: i32,
        actual: i32,
    },
}

impl fmt::Display for CacheLoadError377 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { archive, source } => write!(f, "{}: {source}", archive.jag_name()),
            Self::Checksum {
                archive,
                expected,
                actual,
            } => write!(
                f,
                "{}: CRC32 {actual:08x}, expected {expected:08x}",
                archive.jag_name()
            ),
        }
    }
}

impl std::error::Error for CacheLoadError377 {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Checksum { .. } => None,
        }
    }
}

/// Read one extracted JAG with the negotiated CRC, then use the shared decoder.
///
/// `Client.java:2414-2432` checks CRC32 over the entire packed JAG before
/// constructing `Jagfile`. This offline seam does not fetch missing archives.
/// The caller supplies the selected revision's checksum block, not 289's.
pub fn load_archive(
    dir: &Path,
    archive: CacheArchive377,
    checksums: &[i32; 9],
) -> Result<JagFile, CacheLoadError377> {
    let data = fs::read(dir.join(archive.jag_name()))
        .map_err(|source| CacheLoadError377::Read { archive, source })?;
    let actual = Packet::getcrc(&data, 0, data.len());
    let expected = checksums[archive.crc_slot()];
    if actual != expected {
        return Err(CacheLoadError377::Checksum {
            archive,
            expected,
            actual,
        });
    }
    Ok(JagFile::new(data))
}
