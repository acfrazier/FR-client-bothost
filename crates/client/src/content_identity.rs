//! Versioned decoded game-cache content identity (`274DCI01`).
//!
//! Read-only helper: hashes unpacked JAG members and snapshot bin records
//! into a tagged, domain-separated SHA-256. Packed CRC / transfer identity
//! stays outside this codec. No sidecar, memoizer, network, or writes.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::io::jagfile::JagFile;
use crate::io::try_bunzip2;

/// Canonical encoding magic / format tag.
pub const FORMAT_MAGIC: &[u8; 8] = b"274DCI01";
/// Format version encoded as `u16` BE after the magic.
pub const FORMAT_VERSION: u16 = 1;

/// Section kind: unpacked JAG members sorted by unsigned 32-bit id.
pub const SECTION_KIND_JAG_MEMBERS: u8 = 1;
/// Section kind: snapshot bin archive (`u32le` id/len/bytes records).
pub const SECTION_KIND_BIN_ARCHIVE: u8 = 2;

/// Tagged ASCII layout recorded inside every bin-archive payload.
pub const BIN_LAYOUT: &[u8] = b"u32le_id,u32le_len,bytes";

/// Content JAG packs read from the selected jag directory (not versionlist).
pub const CONTENT_JAGS: [&str; 7] = [
    "title",
    "config",
    "interface",
    "media",
    "textures",
    "wordenc",
    "sounds",
];

/// Full JAG section order in the canonical encoding, including versionlist.
pub const JAG_SECTIONS: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];

/// Bin section name, snapshot file, version-table member, crc-table member.
pub const BIN_SECTIONS: [(&str, &str, &str, &str); 4] = [
    ("models", "models.bin", "model_version", "model_crc"),
    ("anims", "anims.bin", "anim_version", "anim_crc"),
    ("midi", "midi.bin", "midi_version", "midi_crc"),
    ("maps", "maps.bin", "map_version", "map_crc"),
];

/// Exact versionlist member names whose IDs are excluded from the semantic
/// digest. Exclusion is by hashed unsigned id, not a suffix guess.
pub const VERSIONLIST_CRC_NAMES: [&str; 4] = ["model_crc", "anim_crc", "midi_crc", "map_crc"];

/// Versionlist members parsed for bin missingness / coverage (not excluded).
pub const VERSIONLIST_VERSION_NAMES: [&str; 4] = [
    "model_version",
    "anim_version",
    "midi_version",
    "map_version",
];

/// Hashed unsigned 32-bit ids of the four versionlist CRC tables.
pub fn versionlist_crc_member_ids() -> [u32; 4] {
    [
        JagFile::gen_hash(VERSIONLIST_CRC_NAMES[0]) as u32,
        JagFile::gen_hash(VERSIONLIST_CRC_NAMES[1]) as u32,
        JagFile::gen_hash(VERSIONLIST_CRC_NAMES[2]) as u32,
        JagFile::gen_hash(VERSIONLIST_CRC_NAMES[3]) as u32,
    ]
}

fn crc_exclude_set() -> HashSet<u32> {
    versionlist_crc_member_ids().into_iter().collect()
}

/// Kind of one named section in the canonical encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    JagMembers,
    BinArchive,
}

impl SectionKind {
    pub fn tag(self) -> u8 {
        match self {
            SectionKind::JagMembers => SECTION_KIND_JAG_MEMBERS,
            SectionKind::BinArchive => SECTION_KIND_BIN_ARCHIVE,
        }
    }
}

/// SHA-256 of one named section payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSectionDigest {
    pub name: String,
    pub kind: SectionKind,
    pub digest: [u8; 32],
}

impl NamedSectionDigest {
    pub fn digest_hex(&self) -> String {
        hex(&self.digest)
    }
}

/// Versioned decoded content identity: full `content_id` plus per-section
/// digests. `content_id` is SHA-256 of the canonical encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedContentIdentity {
    pub format: u16,
    pub revision: u16,
    pub content_id: [u8; 32],
    pub sections: Vec<NamedSectionDigest>,
}

impl DecodedContentIdentity {
    pub fn content_id_hex(&self) -> String {
        hex(&self.content_id)
    }

    pub fn section(&self, name: &str) -> Option<&NamedSectionDigest> {
        self.sections.iter().find(|s| s.name == name)
    }
}

/// Fallible identity error. Never a panic on malformed input.
#[derive(Debug)]
pub struct ContentIdentityError {
    message: String,
}

impl ContentIdentityError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn io(context: &str, err: io::Error) -> Self {
        Self::new(format!("{context}: {err}"))
    }
}

impl fmt::Display for ContentIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ContentIdentityError {}

/// Compute the versioned decoded content identity from a revision, a selected
/// jag directory (title/config/interface/media/textures/wordenc/sounds), and a
/// snapshot directory (versionlist + the four `*.bin` archives).
///
/// Reads exactly those files. No network, fallback, write, sidecar, or
/// process-wide memoizer. Bin records are streamed; payloads are hashed and
/// dropped rather than retained.
pub fn compute_decoded_content_identity(
    revision: u16,
    jag_dir: impl AsRef<Path>,
    snapshot_dir: impl AsRef<Path>,
) -> Result<DecodedContentIdentity, ContentIdentityError> {
    let jag_dir = jag_dir.as_ref();
    let snapshot_dir = snapshot_dir.as_ref();

    let mut encoding = Vec::new();
    encoding.extend_from_slice(FORMAT_MAGIC);
    encoding.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    encoding.extend_from_slice(&revision.to_be_bytes());

    let mut sections = Vec::with_capacity(JAG_SECTIONS.len() + BIN_SECTIONS.len());
    let mut versionlist: Option<ParsedJag> = None;

    for name in JAG_SECTIONS {
        let path = if name == "versionlist" {
            snapshot_dir.join(name)
        } else {
            jag_dir.join(name)
        };
        let bytes = std::fs::read(&path)
            .map_err(|e| ContentIdentityError::io(&format!("read {}", path.display()), e))?;
        let parsed =
            parse_jag(&bytes).map_err(|e| ContentIdentityError::new(format!("{name}: {e}")))?;
        let payload = if name == "versionlist" {
            validate_versionlist_tables(&parsed)?;
            jag_members_payload(&parsed, Some(&crc_exclude_set()))?
        } else {
            jag_members_payload(&parsed, None)?
        };
        push_section(
            &mut encoding,
            &mut sections,
            name,
            SectionKind::JagMembers,
            &payload,
        );
        if name == "versionlist" {
            versionlist = Some(parsed);
        }
    }

    let versionlist = versionlist
        .ok_or_else(|| ContentIdentityError::new("internal: versionlist missing after scan"))?;

    for (name, file_name, version_table, crc_table) in BIN_SECTIONS {
        let versions = version_table_values(&versionlist, version_table)?;
        let crcs = crc_table_values(&versionlist, crc_table)?;
        if crcs.len() < versions.len() {
            return Err(ContentIdentityError::new(format!(
                "versionlist `{crc_table}` covers {} of {} `{version_table}` entries",
                crcs.len(),
                versions.len()
            )));
        }
        let path = snapshot_dir.join(file_name);
        let payload = bin_archive_payload(&path, &versions)?;
        push_section(
            &mut encoding,
            &mut sections,
            name,
            SectionKind::BinArchive,
            &payload,
        );
    }

    Ok(DecodedContentIdentity {
        format: FORMAT_VERSION,
        revision,
        content_id: sha256(&encoding),
        sections,
    })
}

fn push_section(
    encoding: &mut Vec<u8>,
    sections: &mut Vec<NamedSectionDigest>,
    name: &str,
    kind: SectionKind,
    payload: &[u8],
) {
    let name_bytes = name.as_bytes();
    encoding.push(name_bytes.len() as u8);
    encoding.extend_from_slice(name_bytes);
    encoding.push(kind.tag());
    encoding.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    encoding.extend_from_slice(payload);
    sections.push(NamedSectionDigest {
        name: name.to_string(),
        kind,
        digest: sha256(payload),
    });
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

struct JagMember {
    id: u32,
    unpacked_size: u32,
    packed_size: u32,
    offset: usize,
}

struct ParsedJag {
    data: Vec<u8>,
    members_raw: bool,
    members: Vec<JagMember>,
}

fn parse_jag(src: &[u8]) -> Result<ParsedJag, String> {
    if src.len() < 6 {
        return Err("truncated jag header".to_string());
    }
    let unpacked_size = read_g3(src, 0)?;
    let packed_size = read_g3(src, 3)?;
    let (data, pos, members_raw) = if unpacked_size != packed_size {
        let decompressed = try_bunzip2(&src[6..]).map_err(|e| format!("jag bunzip2: {e}"))?;
        if decompressed.len() as u32 != unpacked_size {
            return Err(format!(
                "jag unpacked size {unpacked_size} != decompressed {}",
                decompressed.len()
            ));
        }
        (decompressed, 0usize, true)
    } else {
        (src.to_vec(), 6usize, false)
    };
    if pos + 2 > data.len() {
        return Err("truncated jag file count".to_string());
    }
    let file_count = read_g2(&data, pos)?;
    let mut pos = pos + 2;
    let catalog_bytes = file_count as usize * 10;
    if pos + catalog_bytes > data.len() {
        return Err("truncated jag catalog".to_string());
    }
    let mut offset = pos + catalog_bytes;
    let mut members = Vec::with_capacity(file_count as usize);
    let mut seen = HashSet::new();
    for _ in 0..file_count {
        let id = read_g4_u32(&data, pos)?;
        let unpacked = read_g3(&data, pos + 4)?;
        let packed = read_g3(&data, pos + 7)?;
        pos += 10;
        if !seen.insert(id) {
            return Err(format!("duplicate jag member id {id}"));
        }
        let packed_usize = packed as usize;
        let end = offset
            .checked_add(packed_usize)
            .ok_or_else(|| format!("jag member {id} packed size overflow"))?;
        if end > data.len() {
            return Err(format!(
                "jag member {id} packed bounds {offset}+{packed} exceed {}",
                data.len()
            ));
        }
        members.push(JagMember {
            id,
            unpacked_size: unpacked,
            packed_size: packed,
            offset,
        });
        offset = end;
    }
    Ok(ParsedJag {
        data,
        members_raw,
        members,
    })
}

fn jag_members_payload(
    jag: &ParsedJag,
    exclude: Option<&HashSet<u32>>,
) -> Result<Vec<u8>, ContentIdentityError> {
    let mut by_id: BTreeMap<u32, (u32, [u8; 32])> = BTreeMap::new();
    for member in &jag.members {
        if exclude.is_some_and(|set| set.contains(&member.id)) {
            continue;
        }
        let bytes = member_bytes(jag, member).map_err(|e| ContentIdentityError::new(e))?;
        let len = bytes.len() as u32;
        by_id.insert(member.id, (len, sha256(&bytes)));
    }
    let mut payload = Vec::new();
    payload.extend_from_slice(&(by_id.len() as u32).to_be_bytes());
    for (id, (len, digest)) in &by_id {
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&len.to_be_bytes());
        payload.extend_from_slice(digest);
    }
    Ok(payload)
}

fn member_bytes(jag: &ParsedJag, member: &JagMember) -> Result<Vec<u8>, String> {
    let start = member.offset;
    let end = start + member.packed_size as usize;
    let src = &jag.data[start..end];
    if member.packed_size == 0 {
        if member.unpacked_size != 0 {
            return Err(format!(
                "jag member {} packed size 0 but unpacked {}",
                member.id, member.unpacked_size
            ));
        }
        return Ok(Vec::new());
    }
    let data = if jag.members_raw {
        src.to_vec()
    } else {
        try_bunzip2(src).map_err(|e| format!("jag member {} bunzip2: {e}", member.id))?
    };
    if data.len() as u32 != member.unpacked_size {
        return Err(format!(
            "jag member {} unpacked size {} != decompressed {}",
            member.id,
            member.unpacked_size,
            data.len()
        ));
    }
    Ok(data)
}

fn member_payload(jag: &ParsedJag, name: &str) -> Result<Vec<u8>, ContentIdentityError> {
    let id = JagFile::gen_hash(name) as u32;
    let member = jag
        .members
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| ContentIdentityError::new(format!("versionlist missing `{name}`")))?;
    member_bytes(jag, member).map_err(ContentIdentityError::new)
}

fn validate_versionlist_tables(jag: &ParsedJag) -> Result<(), ContentIdentityError> {
    for name in VERSIONLIST_VERSION_NAMES {
        let raw = member_payload(jag, name)?;
        if raw.len() % 2 != 0 {
            return Err(ContentIdentityError::new(format!(
                "versionlist `{name}` width {} is not a multiple of 2",
                raw.len()
            )));
        }
    }
    for name in VERSIONLIST_CRC_NAMES {
        let raw = member_payload(jag, name)?;
        if raw.len() % 4 != 0 {
            return Err(ContentIdentityError::new(format!(
                "versionlist `{name}` width {} is not a multiple of 4",
                raw.len()
            )));
        }
    }
    Ok(())
}

fn version_table_values(jag: &ParsedJag, name: &str) -> Result<Vec<u16>, ContentIdentityError> {
    let raw = member_payload(jag, name)?;
    if raw.len() % 2 != 0 {
        return Err(ContentIdentityError::new(format!(
            "versionlist `{name}` width {} is not a multiple of 2",
            raw.len()
        )));
    }
    Ok(raw
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect())
}

fn crc_table_values(jag: &ParsedJag, name: &str) -> Result<Vec<i32>, ContentIdentityError> {
    let raw = member_payload(jag, name)?;
    if raw.len() % 4 != 0 {
        return Err(ContentIdentityError::new(format!(
            "versionlist `{name}` width {} is not a multiple of 4",
            raw.len()
        )));
    }
    Ok(raw
        .chunks_exact(4)
        .map(|c| i32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

struct BinRecord {
    id: u32,
    len: u32,
    digest: [u8; 32],
}

fn bin_archive_payload(path: &Path, versions: &[u16]) -> Result<Vec<u8>, ContentIdentityError> {
    let mut records: BTreeMap<u32, BinRecord> = BTreeMap::new();
    stream_bin_records(path, |id, len, digest| {
        if records.insert(id, BinRecord { id, len, digest }).is_some() {
            return Err(ContentIdentityError::new(format!(
                "{}: duplicate record id {id}",
                path.display()
            )));
        }
        Ok(())
    })?;

    let total = versions.len() as u32;
    let mut skipped_ids: Vec<u32> = Vec::new();
    let mut required: HashSet<u32> = HashSet::new();
    for (index, version) in versions.iter().enumerate() {
        let id = index as u32;
        if *version == 0 {
            skipped_ids.push(id);
        } else {
            required.insert(id);
        }
    }

    for id in records.keys() {
        if !required.contains(id) {
            return Err(ContentIdentityError::new(format!(
                "{}: unknown extra record id {id}",
                path.display()
            )));
        }
    }
    for id in &required {
        if !records.contains_key(id) {
            return Err(ContentIdentityError::new(format!(
                "{}: missing required record id {id}",
                path.display()
            )));
        }
    }

    let unpacked = records.len() as u32;
    let skipped = skipped_ids.len() as u32;
    if total != unpacked + skipped {
        return Err(ContentIdentityError::new(format!(
            "{}: {total} total != {unpacked} unpacked + {skipped} skipped",
            path.display()
        )));
    }

    let mut payload = Vec::new();
    payload.push(BIN_LAYOUT.len() as u8);
    payload.extend_from_slice(BIN_LAYOUT);
    payload.extend_from_slice(&total.to_be_bytes());
    payload.extend_from_slice(&unpacked.to_be_bytes());
    payload.extend_from_slice(&skipped.to_be_bytes());
    payload.extend_from_slice(&(skipped_ids.len() as u32).to_be_bytes());
    for id in &skipped_ids {
        payload.extend_from_slice(&id.to_be_bytes());
    }
    payload.extend_from_slice(&(records.len() as u32).to_be_bytes());
    for rec in records.values() {
        payload.extend_from_slice(&rec.id.to_be_bytes());
        payload.extend_from_slice(&rec.len.to_be_bytes());
        payload.extend_from_slice(&rec.digest);
    }
    Ok(payload)
}

fn stream_bin_records(
    path: &Path,
    mut visit: impl FnMut(u32, u32, [u8; 32]) -> Result<(), ContentIdentityError>,
) -> Result<(), ContentIdentityError> {
    let mut file = File::open(path)
        .map_err(|e| ContentIdentityError::io(&format!("open {}", path.display()), e))?;
    loop {
        let mut header = [0u8; 8];
        let n = read_some(&mut file, &mut header)
            .map_err(|e| ContentIdentityError::io(&format!("read {}", path.display()), e))?;
        if n == 0 {
            return Ok(());
        }
        if n < 8 {
            return Err(ContentIdentityError::new(format!(
                "{}: truncated record header",
                path.display()
            )));
        }
        let id = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let mut payload = vec![0u8; len as usize];
        if let Err(e) = file.read_exact(&mut payload) {
            return Err(ContentIdentityError::new(format!(
                "{}: truncated record id {id} length {len}: {e}",
                path.display()
            )));
        }
        let digest = sha256(&payload);
        drop(payload);
        visit(id, len, digest)?;
    }
}

fn read_some(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => return Ok(filled),
            n => filled += n,
        }
    }
    Ok(filled)
}

fn read_g2(data: &[u8], pos: usize) -> Result<u16, String> {
    if pos + 2 > data.len() {
        return Err("truncated g2".to_string());
    }
    Ok(u16::from_be_bytes([data[pos], data[pos + 1]]))
}

fn read_g3(data: &[u8], pos: usize) -> Result<u32, String> {
    if pos + 3 > data.len() {
        return Err("truncated g3".to_string());
    }
    Ok(((data[pos] as u32) << 16) | ((data[pos + 1] as u32) << 8) | data[pos + 2] as u32)
}

fn read_g4_u32(data: &[u8], pos: usize) -> Result<u32, String> {
    if pos + 4 > data.len() {
        return Err("truncated g4".to_string());
    }
    Ok(u32::from_be_bytes([
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::cache_289::synthetic_jag;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const EMPTY_TABLES: &[(&str, &[u8])] = &[
        ("model_version", &[]),
        ("model_crc", &[]),
        ("anim_version", &[]),
        ("anim_crc", &[]),
        ("midi_version", &[]),
        ("midi_crc", &[]),
        ("map_version", &[]),
        ("map_crc", &[]),
    ];

    fn tmp(tag: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("274bot-dci-{tag}-{}-{stamp}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn encode_bin(records: &[(u32, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (id, data) in records {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(data);
        }
        out
    }

    fn bz2_jagex(data: &[u8], level: bzip2::Compression) -> Vec<u8> {
        use std::io::Write;
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), level);
        enc.write_all(data).unwrap();
        let out = enc.finish().unwrap();
        assert!(out.starts_with(b"BZh"));
        out[4..].to_vec()
    }

    fn push_g3(out: &mut Vec<u8>, value: u32) {
        out.push((value >> 16) as u8);
        out.push((value >> 8) as u8);
        out.push(value as u8);
    }

    fn per_entry_jag(files: &[(&str, &[u8])], level: bzip2::Compression) -> Vec<u8> {
        let packed: Vec<Vec<u8>> = files.iter().map(|(_, d)| bz2_jagex(d, level)).collect();
        let data_len: usize = packed.iter().map(|d| d.len()).sum();
        let payload_len = (2 + 10 * files.len() + data_len) as u32;
        let mut out = Vec::new();
        push_g3(&mut out, payload_len);
        push_g3(&mut out, payload_len);
        out.push((files.len() >> 8) as u8);
        out.push(files.len() as u8);
        for ((name, data), packed_data) in files.iter().zip(packed.iter()) {
            out.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
            push_g3(&mut out, data.len() as u32);
            push_g3(&mut out, packed_data.len() as u32);
        }
        for d in &packed {
            out.extend_from_slice(d);
        }
        out
    }

    fn whole_archive_jag(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut inner = Vec::new();
        inner.push((files.len() >> 8) as u8);
        inner.push(files.len() as u8);
        for (name, data) in files {
            inner.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
            push_g3(&mut inner, data.len() as u32);
            push_g3(&mut inner, data.len() as u32);
        }
        for (_, data) in files {
            inner.extend_from_slice(data);
        }
        let packed = bz2_jagex(&inner, bzip2::Compression::best());
        assert_ne!(
            packed.len(),
            inner.len(),
            "whole-archive fixture needs packed != unpacked"
        );
        let mut out = Vec::new();
        push_g3(&mut out, inner.len() as u32);
        push_g3(&mut out, packed.len() as u32);
        out.extend_from_slice(&packed);
        out
    }

    fn empty_jag() -> Vec<u8> {
        synthetic_jag(&[])
    }

    fn versionlist(members: &[(&str, &[u8])]) -> Vec<u8> {
        synthetic_jag(members)
    }

    fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    fn write_empty_jags(dir: &Path) {
        for name in CONTENT_JAGS {
            write_bytes(dir, name, &empty_jag());
        }
    }

    fn write_empty_bins(dir: &Path) {
        for (_, file, _, _) in BIN_SECTIONS {
            write_bytes(dir, file, &[]);
        }
    }

    fn empty_fixture() -> (PathBuf, PathBuf) {
        let jag_dir = tmp("jags");
        let snap = tmp("snap");
        write_empty_jags(&jag_dir);
        write_empty_jags(&snap);
        write_bytes(&snap, "versionlist", &versionlist(EMPTY_TABLES));
        write_empty_bins(&snap);
        (jag_dir, snap)
    }

    fn file_sha(path: &Path) -> [u8; 32] {
        sha256(&std::fs::read(path).unwrap())
    }

    fn compute(rev: u16, jag: &Path, snap: &Path) -> DecodedContentIdentity {
        compute_decoded_content_identity(rev, jag, snap).unwrap()
    }

    #[test]
    fn empty_valid_fixture_succeeds() {
        let (jag, snap) = empty_fixture();
        let id = compute(289, &jag, &snap);
        assert_eq!(id.format, 1);
        assert_eq!(id.revision, 289);
        assert_eq!(id.sections.len(), 12);
        assert_eq!(id.content_id_hex().len(), 64);
        assert!(id.section("config").is_some());
        assert!(id.section("models").is_some());
        assert!(id.section("versionlist").is_some());
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn crc_only_change_same_content_different_raw() {
        let (jag, snap_a) = empty_fixture();
        let snap_b = tmp("snap-crc");
        for name in CONTENT_JAGS {
            write_bytes(&snap_b, name, &std::fs::read(jag.join(name)).unwrap());
        }
        write_empty_bins(&snap_b);
        let crc_a = vec![0u8, 0, 0, 1];
        let crc_b = vec![0u8, 0, 0, 2];
        let ver = vec![0u8, 0];
        let refs_a: Vec<(&str, Vec<u8>)> = vec![
            ("model_version", ver.clone()),
            ("model_crc", crc_a),
            ("anim_version", vec![]),
            ("anim_crc", vec![]),
            ("midi_version", vec![]),
            ("midi_crc", vec![]),
            ("map_version", vec![]),
            ("map_crc", vec![]),
            ("model_index", vec![1, 2, 3, 4]),
        ];
        let refs_b: Vec<(&str, Vec<u8>)> = vec![
            ("model_version", ver),
            ("model_crc", crc_b),
            ("anim_version", vec![]),
            ("anim_crc", vec![]),
            ("midi_version", vec![]),
            ("midi_crc", vec![]),
            ("map_version", vec![]),
            ("map_crc", vec![]),
            ("model_index", vec![1, 2, 3, 4]),
        ];
        let a_refs: Vec<(&str, &[u8])> = refs_a.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        let b_refs: Vec<(&str, &[u8])> = refs_b.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        write_bytes(&snap_a, "versionlist", &versionlist(&a_refs));
        write_bytes(&snap_b, "versionlist", &versionlist(&b_refs));
        write_empty_bins(&snap_a);

        let id_a = compute(289, &jag, &snap_a);
        let id_b = compute(289, &jag, &snap_b);
        assert_eq!(id_a.content_id, id_b.content_id);
        assert_eq!(
            id_a.section("versionlist").unwrap().digest,
            id_b.section("versionlist").unwrap().digest
        );
        assert_ne!(
            file_sha(&snap_a.join("versionlist")),
            file_sha(&snap_b.join("versionlist"))
        );
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap_a);
        let _ = std::fs::remove_dir_all(&snap_b);
    }

    #[test]
    fn recompressed_and_whole_vs_per_entry_same_content_different_raw() {
        let members = [("flo.dat", &b"config-bytes-for-recompress-0123456789"[..])];
        let per_best = per_entry_jag(&members, bzip2::Compression::best());
        let per_fast = per_entry_jag(&members, bzip2::Compression::fast());
        let whole = whole_archive_jag(&members);
        assert_ne!(per_best, whole);

        let make = |config: &[u8]| {
            let jag = tmp("recomp-jag");
            let snap = tmp("recomp-snap");
            write_empty_jags(&jag);
            write_bytes(&jag, "config", config);
            write_empty_jags(&snap);
            write_bytes(&snap, "versionlist", &versionlist(EMPTY_TABLES));
            write_empty_bins(&snap);
            (jag, snap)
        };
        let (j1, s1) = make(&per_best);
        let (j2, s2) = make(&per_fast);
        let (j3, s3) = make(&whole);
        let a = compute(289, &j1, &s1);
        let b = compute(289, &j2, &s2);
        let c = compute(289, &j3, &s3);
        assert_eq!(a.content_id, b.content_id);
        assert_eq!(a.content_id, c.content_id);
        assert_eq!(
            a.section("config").unwrap().digest,
            b.section("config").unwrap().digest
        );
        assert_ne!(file_sha(&j1.join("config")), file_sha(&j3.join("config")));
        if per_best != per_fast {
            assert_ne!(file_sha(&j1.join("config")), file_sha(&j2.join("config")));
        }
        for dir in [j1, s1, j2, s2, j3, s3] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn fixture_with_models(
        records: &[(u32, &[u8])],
        versions: &[u16],
        extra_vl: &[(&str, &[u8])],
        physical: &[(u32, &[u8])],
    ) -> (PathBuf, PathBuf) {
        let jag = tmp("models-jag");
        let snap = tmp("models-snap");
        write_empty_jags(&jag);
        write_empty_jags(&snap);
        let mut ver = Vec::new();
        for v in versions {
            ver.extend_from_slice(&v.to_be_bytes());
        }
        let crc = vec![0u8; versions.len() * 4];
        let mut vl: Vec<(&str, Vec<u8>)> = vec![
            ("model_version", ver),
            ("model_crc", crc),
            ("anim_version", vec![]),
            ("anim_crc", vec![]),
            ("midi_version", vec![]),
            ("midi_crc", vec![]),
            ("map_version", vec![]),
            ("map_crc", vec![]),
        ];
        for (n, b) in extra_vl {
            vl.push((n, b.to_vec()));
        }
        let refs: Vec<(&str, &[u8])> = vl.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        write_bytes(&snap, "versionlist", &versionlist(&refs));
        write_empty_bins(&snap);
        write_bytes(&snap, "models.bin", &encode_bin(physical));
        let _ = records;
        (jag, snap)
    }

    #[test]
    fn non_crc_version_index_member_mutation_changes_content() {
        let payload = b"model-a";
        let (j1, s1) = fixture_with_models(
            &[(0, payload)],
            &[1],
            &[("model_index", b"idx-a")],
            &[(0, payload)],
        );
        let (j2, s2) = fixture_with_models(
            &[(0, payload)],
            &[2],
            &[("model_index", b"idx-a")],
            &[(0, payload)],
        );
        let (j3, s3) = fixture_with_models(
            &[(0, payload)],
            &[1],
            &[("model_index", b"idx-b")],
            &[(0, payload)],
        );
        let jag_mut = tmp("member-jag");
        let snap_mut = tmp("member-snap");
        write_empty_jags(&jag_mut);
        write_bytes(
            &jag_mut,
            "config",
            &synthetic_jag(&[("flo.dat", b"changed")]),
        );
        write_empty_jags(&snap_mut);
        write_bytes(
            &snap_mut,
            "versionlist",
            &std::fs::read(s1.join("versionlist")).unwrap(),
        );
        write_empty_bins(&snap_mut);
        write_bytes(
            &snap_mut,
            "models.bin",
            &std::fs::read(s1.join("models.bin")).unwrap(),
        );

        let a = compute(289, &j1, &s1);
        let ver_mut = compute(289, &j2, &s2);
        let idx_mut = compute(289, &j3, &s3);
        let member_mut = compute(289, &jag_mut, &snap_mut);
        assert_ne!(a.content_id, ver_mut.content_id);
        assert_ne!(a.content_id, idx_mut.content_id);
        assert_ne!(a.content_id, member_mut.content_id);
        for dir in [j1, s1, j2, s2, j3, s3, jag_mut, snap_mut] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn distinct_payload_id_swap_changes_content() {
        let a = b"payload-aaa";
        let b = b"payload-bbb";
        let (j1, s1) = fixture_with_models(&[], &[1, 1], &[], &[(0, a), (1, b)]);
        let (j2, s2) = fixture_with_models(&[], &[1, 1], &[], &[(0, b), (1, a)]);
        let id1 = compute(289, &j1, &s1);
        let id2 = compute(289, &j2, &s2);
        assert_ne!(id1.content_id, id2.content_id);
        assert_ne!(
            id1.section("models").unwrap().digest,
            id2.section("models").unwrap().digest
        );
        for dir in [j1, s1, j2, s2] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn identical_payload_swap_same_content() {
        let a = b"same-payload";
        let (j1, s1) = fixture_with_models(&[], &[1, 1], &[], &[(0, a), (1, a)]);
        let (j2, s2) = fixture_with_models(&[], &[1, 1], &[], &[(1, a), (0, a)]);
        let id1 = compute(289, &j1, &s1);
        let id2 = compute(289, &j2, &s2);
        assert_eq!(id1.content_id, id2.content_id);
        for dir in [j1, s1, j2, s2] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn physical_order_alone_same_content() {
        let a = b"one";
        let b = b"two-xxxx";
        let (j1, s1) = fixture_with_models(&[], &[1, 1], &[], &[(0, a), (1, b)]);
        let (j2, s2) = fixture_with_models(&[], &[1, 1], &[], &[(1, b), (0, a)]);
        let id1 = compute(289, &j1, &s1);
        let id2 = compute(289, &j2, &s2);
        assert_eq!(id1.content_id, id2.content_id);
        for dir in [j1, s1, j2, s2] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn id_reassignment_changes_content() {
        let payload = b"only";
        let (j1, s1) = fixture_with_models(&[], &[1, 0], &[], &[(0, payload)]);
        let (j2, s2) = fixture_with_models(&[], &[0, 1], &[], &[(1, payload)]);
        let id1 = compute(289, &j1, &s1);
        let id2 = compute(289, &j2, &s2);
        assert_ne!(id1.content_id, id2.content_id);
        for dir in [j1, s1, j2, s2] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn missingness_and_count_change_content() {
        let payload = b"keep";
        let (j1, s1) = fixture_with_models(&[], &[1, 0], &[], &[(0, payload)]);
        let (j2, s2) = fixture_with_models(&[], &[1], &[], &[(0, payload)]);
        let id1 = compute(289, &j1, &s1);
        let id2 = compute(289, &j2, &s2);
        assert_ne!(id1.content_id, id2.content_id);
        for dir in [j1, s1, j2, s2] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn missing_required_fails() {
        let (jag, snap) = fixture_with_models(&[], &[1], &[], &[]);
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(err.to_string().contains("missing required"), "got {err}");
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn unknown_extra_record_fails() {
        let (jag, snap) = fixture_with_models(&[], &[0], &[], &[(0, b"nope")]);
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(err.to_string().contains("unknown extra"), "got {err}");
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn duplicate_jag_ids_fail() {
        let (jag, snap) = empty_fixture();
        let hash = JagFile::gen_hash("flo.dat").to_be_bytes();
        let member = [("flo.dat", &b"a"[..])];
        let packed_member = {
            let files = member;
            let packed: Vec<Vec<u8>> = files
                .iter()
                .map(|(_, d)| bz2_jagex(d, bzip2::Compression::best()))
                .collect();
            packed
        };
        let data_len = packed_member[0].len() * 2;
        let payload_len = (2 + 10 * 2 + data_len) as u32;
        let mut bytes = Vec::new();
        push_g3(&mut bytes, payload_len);
        push_g3(&mut bytes, payload_len);
        bytes.push(0);
        bytes.push(2);
        for _ in 0..2 {
            bytes.extend_from_slice(&hash);
            push_g3(&mut bytes, 1);
            push_g3(&mut bytes, packed_member[0].len() as u32);
        }
        bytes.extend_from_slice(&packed_member[0]);
        bytes.extend_from_slice(&packed_member[0]);
        write_bytes(&jag, "config", &bytes);
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(err.to_string().contains("duplicate"), "got {err}");
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn duplicate_bin_ids_fail() {
        let (jag, snap) = fixture_with_models(&[], &[1], &[], &[(0, b"a"), (0, b"b")]);
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(err.to_string().contains("duplicate"), "got {err}");
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn truncated_jag_fails() {
        let (jag, snap) = empty_fixture();
        write_bytes(&jag, "title", &[0, 0, 6]);
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(
            err.to_string().contains("truncated") || err.to_string().contains("title"),
            "got {err}"
        );
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn truncated_bin_fails() {
        let (jag, snap) = fixture_with_models(&[], &[1], &[], &[(0, b"abcdef")]);
        let mut bytes = std::fs::read(snap.join("models.bin")).unwrap();
        bytes.truncate(9);
        write_bytes(&snap, "models.bin", &bytes);
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(err.to_string().contains("truncated"), "got {err}");
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn malformed_bin_length_fails() {
        let (jag, snap) = empty_fixture();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(b"short");
        write_bytes(&snap, "models.bin", &bytes);
        write_bytes(
            &snap,
            "versionlist",
            &versionlist(&[
                ("model_version", &[0, 1][..]),
                ("model_crc", &[0, 0, 0, 0][..]),
                ("anim_version", &[]),
                ("anim_crc", &[]),
                ("midi_version", &[]),
                ("midi_crc", &[]),
                ("map_version", &[]),
                ("map_crc", &[]),
            ]),
        );
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(
            err.to_string().contains("truncated") || err.to_string().contains("length"),
            "got {err}"
        );
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn table_width_fails() {
        let (jag, snap) = empty_fixture();
        write_bytes(
            &snap,
            "versionlist",
            &versionlist(&[
                ("model_version", &[0][..]),
                ("model_crc", &[0, 0, 0, 0][..]),
                ("anim_version", &[]),
                ("anim_crc", &[]),
                ("midi_version", &[]),
                ("midi_crc", &[]),
                ("map_version", &[]),
                ("map_crc", &[]),
            ]),
        );
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(err.to_string().contains("width"), "got {err}");
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn crc_table_width_fails() {
        let (jag, snap) = empty_fixture();
        write_bytes(
            &snap,
            "versionlist",
            &versionlist(&[
                ("model_version", &[0, 1][..]),
                ("model_crc", &[0, 0][..]),
                ("anim_version", &[]),
                ("anim_crc", &[]),
                ("midi_version", &[]),
                ("midi_crc", &[]),
                ("map_version", &[]),
                ("map_crc", &[]),
            ]),
        );
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(err.to_string().contains("width"), "got {err}");
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn missing_required_jag_fails() {
        let (jag, snap) = empty_fixture();
        std::fs::remove_file(jag.join("title")).unwrap();
        let err = compute_decoded_content_identity(289, &jag, &snap).unwrap_err();
        assert!(err.to_string().contains("title"), "got {err}");
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn revision_274_vs_289_differ() {
        let (jag, snap) = empty_fixture();
        let a = compute(274, &jag, &snap);
        let b = compute(289, &jag, &snap);
        assert_ne!(a.content_id, b.content_id);
        assert_eq!(
            a.section("config").unwrap().digest,
            b.section("config").unwrap().digest
        );
        let _ = std::fs::remove_dir_all(&jag);
        let _ = std::fs::remove_dir_all(&snap);
    }

    #[test]
    fn crc_member_ids_are_hashed_unsigned_not_names() {
        let ids = versionlist_crc_member_ids();
        for (i, name) in VERSIONLIST_CRC_NAMES.iter().enumerate() {
            assert_eq!(ids[i], JagFile::gen_hash(name) as u32);
        }
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    #[ignore]
    fn real_retained_unpack_289_snapshots_have_equal_decoded_identity() {
        let home = std::env::var("HOME").unwrap();
        let a = PathBuf::from(format!("{home}/.274bot/unpack-289/37214163f1e6ceca"));
        let b = PathBuf::from(format!("{home}/.274bot/unpack-289/6dcb7c4ad1b85372"));
        assert!(a.is_dir(), "missing snapshot {}", a.display());
        assert!(b.is_dir(), "missing snapshot {}", b.display());
        let id_a = compute_decoded_content_identity(289, &a, &a).unwrap();
        let id_b = compute_decoded_content_identity(289, &b, &b).unwrap();
        eprintln!("snapshot-a {}", a.display());
        eprintln!("snapshot-b {}", b.display());
        eprintln!("content_id_a {}", id_a.content_id_hex());
        eprintln!("content_id_b {}", id_b.content_id_hex());
        for section in &id_a.sections {
            let other = id_b.section(&section.name).unwrap();
            eprintln!(
                "section {} a={} b={}",
                section.name,
                section.digest_hex(),
                other.digest_hex()
            );
        }
        for name in JAG_SECTIONS {
            eprintln!(
                "raw {name} a={} b={}",
                hex(&file_sha(&a.join(name))),
                hex(&file_sha(&b.join(name)))
            );
        }
        for (_, file, _, _) in BIN_SECTIONS {
            eprintln!(
                "raw {file} a={} b={}",
                hex(&file_sha(&a.join(file))),
                hex(&file_sha(&b.join(file)))
            );
        }
        let man_a = std::fs::read_to_string(a.join("manifest")).unwrap_or_default();
        let man_b = std::fs::read_to_string(b.join("manifest")).unwrap_or_default();
        eprintln!("manifest-a\n{man_a}");
        eprintln!("manifest-b\n{man_b}");
        assert_eq!(id_a.content_id, id_b.content_id);
    }
}
