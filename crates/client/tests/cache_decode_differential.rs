//! Explicit corpus job for the DEPS-BATCH-025 cache decoder comparison.
//!
//! Run from `vendor/fr-client-rust` with frozen evidence assets and a private
//! output root:
//!
//! ```text
//! CACHE_DECODE_CORPUS_ROOT=/path/to/corpus/cache \
//! CACHE_DECODE_CAPTURE_ROOT=/path/to/private/capture \
//! cargo test -p client --test cache_decode_differential -- --ignored
//! ```
//!
//! Set `CACHE_DECODE_BASELINE_ROOT` to an earlier capture root to require a
//! byte-for-byte comparison. The corpus itself is never written. Compressed
//! malformed fixtures are fixed bytes; gzip boundary streams are assembled by
//! this test's deterministic stored-DEFLATE writer, not by flate2 encoders.

use std::collections::{btree_map::Entry, BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use client::io::jagfile::JagFile;
use client::io::ondemand::OnDemand;
use client::io::{bunzip2, try_bunzip2, ClientRevision, Packet};
use client::unpack::{
    snapshot_state, unpack_cache_from_store, ArchiveStats, Manifest, SnapshotState,
};

const CORPUS_ENV: &str = "CACHE_DECODE_CORPUS_ROOT";
const CAPTURE_ENV: &str = "CACHE_DECODE_CAPTURE_ROOT";
const BASELINE_ENV: &str = "CACHE_DECODE_BASELINE_ROOT";
const JAGS: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];
const ARCHIVES: [ArchiveSpec; 4] = [
    ArchiveSpec {
        name: "models",
        version_table: "model_version",
        crc_table: "model_crc",
        store_index: 1,
        ondemand_archive: 0,
        snapshot_file: "models.bin",
    },
    ArchiveSpec {
        name: "anims",
        version_table: "anim_version",
        crc_table: "anim_crc",
        store_index: 2,
        ondemand_archive: 1,
        snapshot_file: "anims.bin",
    },
    ArchiveSpec {
        name: "midi",
        version_table: "midi_version",
        crc_table: "midi_crc",
        store_index: 3,
        ondemand_archive: 2,
        snapshot_file: "midi.bin",
    },
    ArchiveSpec {
        name: "maps",
        version_table: "map_version",
        crc_table: "map_crc",
        store_index: 4,
        ondemand_archive: 3,
        snapshot_file: "maps.bin",
    },
];

// Frozen headerless bzip streams. They were encoded once before the version
// matrix; no candidate bzip2 encoder runs while these inputs are compared.
const BZ_EMPTY: &str = "17724538509000000000";
const BZ_SMALL: &str = "3141592653593a429d210000051580400020902724c06020003100000a69b4981a32802ae23268992fa2ee48a70a12074853a420";
const BZ_MULTI: &str = "31415926535928e7cfbc0061a30400300020003080291942505aa1282e6282b24ca6b3b02535ce00c3460800600040006100523542505942505cc50564994d64a39f3ef001868c1000c0008000c200a46509416a84a0b98a0ac9329ac8eb347c0800007020018001000184026a32351c5dc914e14241af741a1c";
const BZ_LEVEL_9: &str =
    "3141592653593e39032700c34f8400300020003080291812505812505c5dc914e14240f8e40c9c";

// Static outer-compressed versionlists generated once with their version and
// CRC tables frozen. The gzip bytes they describe are generated below from
// stored-DEFLATE blocks and patched deterministically for malformed cases.
const GZIP_CASES_VERSIONLIST: &str = "314159265359c958c78f0000127ffffed184037258240010424081000190c8400000900280000211c24010411014062000742a9ea698464c2683132334d26d342790d9420a0d260130d008c230134c020294db185ffd8a63edb57979344592158801a2049d4042e210039084c427202792903104720cd2a4606cf149c3f3aa5d2c52d8710fe3ccec2ec50489843c68306e2ee48a70a12192b18f1e";
const BAD_DEFLATE_VERSIONLIST: &str = "3141592653599912dc6100001276bff4c1800332580040408110c800900200000210c24000011004062000545348c8d1901a64c801bd506a7aa0d000d001a7a4d3421c725be602f7241605048005a5a54030a86a54c219073648788f7cb6f213f79a0046a6f88109f42afc5dc914e14242644b7184";
const BAD_GZIP_CRC_VERSIONLIST: &str = "3141592653597847e2cb0000127ffff4c18003325820000002408100011080400000900200000210c04000011004062000545468068d1a34c4c134c9e41a6a83400d34001934d0b3862a7cc02dcd8280a09000b4f45006141a14610c83bb921e39ed9be13a34245112b48169042438d45dc914e14241e11f8b2c";

#[derive(Clone, Copy)]
struct ArchiveSpec {
    name: &'static str,
    version_table: &'static str,
    crc_table: &'static str,
    store_index: i32,
    ondemand_archive: i32,
    snapshot_file: &'static str,
}

struct CaptureSet {
    root: PathBuf,
    artifacts: BTreeSet<String>,
}

impl CaptureSet {
    fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root)
            .map_err(|error| format!("create capture directory {}: {error}", root.display()))?;
        Ok(Self {
            root,
            artifacts: BTreeSet::new(),
        })
    }

    fn write(&mut self, relative: &str, bytes: &[u8]) -> Result<(), String> {
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(format!("capture path is not relative: {relative}"));
        }
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("create capture directory {}: {error}", parent.display())
            })?;
        }
        fs::write(&path, bytes)
            .map_err(|error| format!("write capture artifact {}: {error}", path.display()))?;
        self.artifacts.insert(relative.replace('\\', "/"));
        Ok(())
    }

    fn finish(&self, baseline: Option<&Path>, revision: &str) -> Result<(), String> {
        let mut listing = self
            .artifacts
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if !listing.is_empty() {
            listing.push('\n');
        }
        let list_path = self.root.join("artifact-list.txt");
        fs::write(&list_path, listing.as_bytes())
            .map_err(|error| format!("write artifact list {}: {error}", list_path.display()))?;

        let Some(baseline) = baseline else {
            return Ok(());
        };
        let baseline_listing_path = baseline.join("artifact-list.txt");
        let baseline_listing = fs::read_to_string(&baseline_listing_path).map_err(|error| {
            format!(
                "{revision}: baseline artifact list {} is required: {error}",
                baseline_listing_path.display()
            )
        })?;
        let baseline_names = baseline_listing
            .lines()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        if baseline_names.len() != baseline_listing.lines().count() {
            return Err(format!(
                "{revision}: baseline artifact list {} contains duplicate paths",
                baseline_listing_path.display()
            ));
        }
        if let Some(missing) = baseline_names.difference(&self.artifacts).next() {
            return Err(format!(
                "{revision}: archive={}, member={}, offset=0, current=<missing>, baseline=<present> (artifact-set difference: {missing})",
                artifact_archive(missing), artifact_member(missing)
            ));
        }
        if let Some(extra) = self.artifacts.difference(&baseline_names).next() {
            return Err(format!(
                "{revision}: archive={}, member={}, offset=0, current=<present>, baseline=<missing> (artifact-set difference: {extra})",
                artifact_archive(extra), artifact_member(extra)
            ));
        }

        for relative in &self.artifacts {
            let current_path = self.root.join(relative);
            let baseline_path = baseline.join(relative);
            let current = fs::read(&current_path).map_err(|error| {
                format!(
                    "{revision}: read current artifact {}: {error}",
                    current_path.display()
                )
            })?;
            let expected = fs::read(&baseline_path).map_err(|error| {
                format!(
                    "{revision}: archive={}, member={}, offset=0, current=<present>, baseline=<missing> ({}: {error})",
                    artifact_archive(relative),
                    artifact_member(relative),
                    baseline_path.display()
                )
            })?;
            if let Some(offset) = first_difference(&current, &expected) {
                return Err(byte_difference_report(
                    revision, relative, offset, &current, &expected,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct JagCoverage {
    layout: &'static str,
    members: usize,
    decoded: usize,
    missing: usize,
    duplicate_hashes: usize,
    input_bytes: usize,
    outer_decoded_bytes: Option<usize>,
}

struct ArchiveCoverage {
    spec: ArchiveSpec,
    versions: Vec<i32>,
    crcs: Vec<i32>,
    required: Vec<i32>,
    zero_versions: usize,
    store_entries: usize,
    missing_store: Vec<i32>,
    ondemand_records: BTreeMap<i32, Vec<u8>>,
    ondemand_duplicates: usize,
    missing_ondemand: Vec<i32>,
    ondemand_timeout: bool,
    ondemand_bytes: usize,
    snapshot_stats: Option<ArchiveStats>,
}

struct JagTable {
    hash: i32,
    unpacked_size: usize,
    packed_size: usize,
}

struct WorkDir(PathBuf);

impl WorkDir {
    fn new(label: &str) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = capture_root()?;
        let path = root.join(format!(
            ".cache-decode-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path)
            .map_err(|error| format!("create scratch directory {}: {error}", path.display()))?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn capture_root() -> Result<PathBuf, String> {
    env_path(CAPTURE_ENV)
}

fn corpus_root() -> Result<PathBuf, String> {
    env_path(CORPUS_ENV)
}

fn baseline_root() -> Result<Option<PathBuf>, String> {
    match std::env::var_os(BASELINE_ENV) {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => Ok(Some(PathBuf::from(value))),
    }
}

fn env_path(name: &str) -> Result<PathBuf, String> {
    let value = std::env::var_os(name).ok_or_else(|| {
        format!("{name} is required; see the module documentation for the corpus job")
    })?;
    if value.is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    Ok(PathBuf::from(value))
}

fn group_path(root: &Path, group: &str) -> PathBuf {
    root.join(group)
}

fn frozen_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0, "frozen hex fixture has an odd length");
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
            u8::from_str_radix(digits, 16).expect("fixture hex is valid")
        })
        .collect()
}

fn read_u24(bytes: &[u8], offset: usize, context: &str) -> Result<usize, String> {
    let slice = bytes
        .get(offset..offset + 3)
        .ok_or_else(|| format!("{context}: truncated 24-bit integer at {offset}"))?;
    Ok(((slice[0] as usize) << 16) | ((slice[1] as usize) << 8) | slice[2] as usize)
}

fn append_u24(bytes: &mut Vec<u8>, value: usize) {
    bytes.extend_from_slice(&[
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ]);
}

fn frozen_outer_jag(packed_hex: &str, unpacked_size: usize) -> Vec<u8> {
    let packed = frozen_hex(packed_hex);
    let mut jag = Vec::with_capacity(6 + packed.len());
    append_u24(&mut jag, unpacked_size);
    append_u24(&mut jag, packed.len());
    jag.extend_from_slice(&packed);
    jag
}

fn first_difference(current: &[u8], expected: &[u8]) -> Option<usize> {
    let shared = current.len().min(expected.len());
    current
        .iter()
        .zip(expected)
        .position(|(left, right)| left != right)
        .or_else(|| (current.len() != expected.len()).then_some(shared))
}

fn byte_at(bytes: &[u8], offset: usize) -> String {
    bytes
        .get(offset)
        .map(|byte| format!("0x{byte:02x}"))
        .unwrap_or_else(|| "<EOF>".to_string())
}

fn artifact_archive(path: &str) -> String {
    let parts = path.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["jag", archive, ..] => (*archive).to_string(),
        ["snapshot", name] | ["ondemand", name] => {
            name.strip_suffix(".bin").unwrap_or(name).to_string()
        }
        [first, ..] => (*first).to_string(),
        [] => "cache".to_string(),
    }
}

fn artifact_member(path: &str) -> String {
    let parts = path.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["jag", _, member, ..] => (*member).to_string(),
        [_, member, ..] => (*member).to_string(),
        [member] => (*member).to_string(),
        [] => "unknown".to_string(),
    }
}

fn record_at_offset(bytes: &[u8], offset: usize) -> Option<(u32, usize)> {
    let mut pos = 0usize;
    while pos.checked_add(8)? <= bytes.len() {
        let id = u32::from_le_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?);
        let length = u32::from_le_bytes(bytes.get(pos + 4..pos + 8)?.try_into().ok()?) as usize;
        let payload = pos.checked_add(8)?;
        let end = payload.checked_add(length)?;
        if end > bytes.len() {
            return None;
        }
        if offset < payload {
            return Some((id, offset - pos));
        }
        if offset < end {
            return Some((id, offset - payload));
        }
        pos = end;
    }
    None
}

fn byte_difference_report(
    revision: &str,
    path: &str,
    offset: usize,
    current: &[u8],
    expected: &[u8],
) -> String {
    let record_context = if path.starts_with("snapshot/") || path.starts_with("ondemand/") {
        record_at_offset(current, offset).or_else(|| record_at_offset(expected, offset))
    } else {
        None
    };
    let (archive, member, reported_offset) = if let Some((id, member_offset)) = record_context {
        (
            artifact_archive(path),
            format!("id={id}, payload/record offset={member_offset}"),
            member_offset,
        )
    } else if let Some(rest) = path.strip_prefix("jag/") {
        let mut parts = rest.split('/');
        let archive = parts.next().unwrap_or("jag").to_string();
        let member = parts.next().unwrap_or("container").to_string();
        (archive, member, offset)
    } else {
        (artifact_archive(path), artifact_member(path), offset)
    };
    format!(
        "{revision}: archive={archive}, member={member}, offset={reported_offset} (artifact={path}, stream offset={offset}), current={}, baseline={}",
        byte_at(current, offset),
        byte_at(expected, offset)
    )
}

fn assert_bytes_equal(context: &str, current: &[u8], expected: &[u8]) -> Result<(), String> {
    if let Some(offset) = first_difference(current, expected) {
        return Err(format!(
            "{context}: first differing byte at {offset}: current={}, expected={}",
            byte_at(current, offset),
            byte_at(expected, offset)
        ));
    }
    Ok(())
}

fn jag_sized(bytes: &[u8], unpacked_size: usize) -> Vec<u8> {
    let mut out = vec![0; unpacked_size];
    let copy_len = bytes.len().min(unpacked_size);
    out[..copy_len].copy_from_slice(&bytes[..copy_len]);
    out
}

fn capture_jag_bytes(
    capture: &mut CaptureSet,
    name: &str,
    input: &[u8],
) -> Result<(JagFile, JagCoverage), String> {
    capture.write(&format!("input/{name}.jag"), input)?;
    if input.len() < 6 {
        return Err(format!("JAG {name}: truncated outer header"));
    }
    let unpacked_size = read_u24(input, 0, name)?;
    let packed_size = read_u24(input, 3, name)?;
    let is_outer = unpacked_size != packed_size;
    let outer_body = if is_outer {
        Some(
            try_bunzip2(&input[6..])
                .map_err(|error| format!("JAG {name}: outer bzip decode failed: {error}"))?,
        )
    } else {
        None
    };
    let body = outer_body.as_deref().unwrap_or(&input[6..]);
    let body_for_parse = if is_outer {
        jag_sized(body, unpacked_size)
    } else {
        body.to_vec()
    };
    if body_for_parse.len() < 2 {
        return Err(format!("JAG {name}: missing member count"));
    }
    let declared_members = u16::from_be_bytes([body_for_parse[0], body_for_parse[1]]) as usize;
    let table_end = 2usize
        .checked_add(
            declared_members
                .checked_mul(10)
                .ok_or_else(|| format!("JAG {name}: member table size overflows"))?,
        )
        .ok_or_else(|| format!("JAG {name}: member table end overflows"))?;
    if table_end > body_for_parse.len() {
        return Err(format!(
            "JAG {name}: member table ends at {table_end}, body has {} bytes",
            body_for_parse.len()
        ));
    }

    let mut table = Vec::with_capacity(declared_members);
    let mut hashes = HashSet::with_capacity(declared_members);
    let mut duplicate_hashes = 0usize;
    let mut table_pos = 2usize;
    for _ in 0..declared_members {
        let hash = i32::from_be_bytes(
            body_for_parse[table_pos..table_pos + 4]
                .try_into()
                .map_err(|_| format!("JAG {name}: malformed member hash"))?,
        );
        let member_unpacked = read_u24(&body_for_parse, table_pos + 4, name)?;
        let member_packed = read_u24(&body_for_parse, table_pos + 7, name)?;
        if !hashes.insert(hash) {
            duplicate_hashes += 1;
        }
        table.push(JagTable {
            hash,
            unpacked_size: member_unpacked,
            packed_size: member_packed,
        });
        table_pos += 10;
    }

    let jag = JagFile::new(input.to_vec());
    if jag.file_count < 0 || jag.file_count as usize != declared_members {
        return Err(format!(
            "JAG {name}: production JagFile reports {} members, raw table reports {declared_members}",
            jag.file_count
        ));
    }
    let mut coverage = String::from("index\thash\tunpacked_bytes\tpacked_bytes\tdecoded_bytes\n");
    let mut cursor = table_end;
    let mut decoded = 0usize;
    let mut missing = 0usize;
    for (index, entry) in table.iter().enumerate() {
        let end = cursor
            .checked_add(entry.packed_size)
            .ok_or_else(|| format!("JAG {name} member {index}: data end overflows"))?;
        let packed = body_for_parse
            .get(cursor..end)
            .ok_or_else(|| format!("JAG {name} member {index}: packed bytes are truncated"))?;
        let expected = if is_outer {
            jag_sized(packed, entry.unpacked_size)
        } else {
            let expanded = try_bunzip2(packed).map_err(|error| {
                format!("JAG {name} member {index}: bzip wrapper failed: {error}")
            })?;
            jag_sized(&expanded, entry.unpacked_size)
        };
        match jag.read_index(index as i32) {
            Some(actual) => {
                assert_bytes_equal(
                    &format!("JAG {name} member {index} through bzip wrapper and JagFile"),
                    &actual,
                    &expected,
                )?;
                capture.write(&format!("jag/{name}/member-{index:06}.bin"), &actual)?;
                decoded += 1;
                coverage.push_str(&format!(
                    "{index}\t{}\t{}\t{}\t{}\n",
                    entry.hash,
                    entry.unpacked_size,
                    entry.packed_size,
                    actual.len()
                ));
            }
            None => {
                missing += 1;
                coverage.push_str(&format!(
                    "{index}\t{}\t{}\t{}\tmissing\n",
                    entry.hash, entry.unpacked_size, entry.packed_size
                ));
            }
        }
        cursor = end;
    }
    capture.write(&format!("jag/{name}/members.tsv"), coverage.as_bytes())?;
    if let Some(outer) = &outer_body {
        capture.write(&format!("jag/{name}/outer-decoded.bin"), outer)?;
    }
    Ok((
        jag,
        JagCoverage {
            layout: if is_outer {
                "outer-compressed"
            } else {
                "member-compressed"
            },
            members: declared_members,
            decoded,
            missing,
            duplicate_hashes,
            input_bytes: input.len(),
            outer_decoded_bytes: outer_body.as_ref().map(Vec::len),
        },
    ))
}

fn parse_version_table(jag: &JagFile, spec: ArchiveSpec) -> Result<(Vec<i32>, Vec<i32>), String> {
    let version_bytes = jag
        .read(spec.version_table)
        .ok_or_else(|| format!("versionlist missing {}", spec.version_table))?;
    let crc_bytes = jag
        .read(spec.crc_table)
        .ok_or_else(|| format!("versionlist missing {}", spec.crc_table))?;
    if version_bytes.len() % 2 != 0 {
        return Err(format!(
            "{} has a partial 16-bit version entry",
            spec.version_table
        ));
    }
    if crc_bytes.len() % 4 != 0 {
        return Err(format!("{} has a partial 32-bit CRC entry", spec.crc_table));
    }
    let versions = version_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|entry| i32::from(u16::from_be_bytes(*entry)))
        .collect::<Vec<_>>();
    let crcs = crc_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|entry| i32::from_be_bytes(*entry))
        .collect::<Vec<_>>();
    if crcs.len() < versions.len() {
        return Err(format!(
            "{} covers {} of {} entries in {}",
            spec.crc_table,
            crcs.len(),
            versions.len(),
            spec.version_table
        ));
    }
    Ok((versions, crcs))
}

#[derive(Clone, Copy)]
struct StoreEntry {
    size: usize,
    sector: usize,
}

fn read_store_index(store_root: &Path, index: i32) -> Result<Vec<StoreEntry>, String> {
    let path = store_root.join(format!("main_file_cache.idx{index}"));
    let bytes = fs::read(&path)
        .map_err(|error| format!("read required store index {}: {error}", path.display()))?;
    if bytes.len() % 6 != 0 {
        return Err(format!(
            "store index {} has {} trailing bytes",
            path.display(),
            bytes.len() % 6
        ));
    }
    bytes
        .as_chunks::<6>()
        .0
        .iter()
        .map(|entry| {
            Ok(StoreEntry {
                size: read_u24(entry, 0, &path.display().to_string())?,
                sector: read_u24(entry, 3, &path.display().to_string())?,
            })
        })
        .collect()
}

fn fetch_archive(
    source: &mut OnDemand,
    archive: i32,
    required: &[i32],
    timeout: Duration,
) -> (BTreeMap<i32, Vec<u8>>, usize, bool) {
    let mut records = BTreeMap::new();
    let mut duplicates = 0usize;
    let mut timed_out = false;
    for batch in required.chunks(32) {
        for &file in batch {
            source.request(archive, file);
        }
        let deadline = Instant::now() + timeout;
        while source.remaining() > 0 {
            source.run(false);
            while let Some(mut request) = source.loop_request() {
                if request.archive != archive {
                    continue;
                }
                let Some(bytes) = request.data.take() else {
                    continue;
                };
                if let Entry::Vacant(entry) = records.entry(request.file) {
                    entry.insert(bytes);
                } else {
                    duplicates += 1;
                }
            }
            if source.remaining() == 0 {
                break;
            }
            if Instant::now() >= deadline {
                timed_out = true;
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        if timed_out {
            break;
        }
    }
    (records, duplicates, timed_out)
}

fn record_stream(records: &BTreeMap<i32, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut size = 0usize;
    for bytes in records.values() {
        size = size
            .checked_add(8)
            .and_then(|total| total.checked_add(bytes.len()))
            .ok_or_else(|| "record stream length overflows usize".to_string())?;
    }
    let mut output = Vec::with_capacity(size);
    for (&id, bytes) in records {
        let id = u32::try_from(id).map_err(|_| format!("negative record id {id}"))?;
        let length = u32::try_from(bytes.len())
            .map_err(|_| format!("record {id} payload length exceeds u32"))?;
        output.extend_from_slice(&id.to_le_bytes());
        output.extend_from_slice(&length.to_le_bytes());
        output.extend_from_slice(bytes);
    }
    Ok(output)
}

fn parse_record_ids(bytes: &[u8]) -> Result<Vec<u32>, String> {
    let mut ids = Vec::new();
    let mut position = 0usize;
    while position < bytes.len() {
        let header_end = position
            .checked_add(8)
            .ok_or_else(|| "record header position overflows".to_string())?;
        let header = bytes
            .get(position..header_end)
            .ok_or_else(|| format!("truncated record header at {position}"))?;
        let id = u32::from_le_bytes(header[..4].try_into().expect("four-byte record id"));
        let length =
            u32::from_le_bytes(header[4..].try_into().expect("four-byte record length")) as usize;
        let end = header_end
            .checked_add(length)
            .ok_or_else(|| format!("record {id} end overflows"))?;
        if end > bytes.len() {
            return Err(format!("truncated record {id} at offset {position}"));
        }
        ids.push(id);
        position = end;
    }
    Ok(ids)
}

fn ids_text(ids: impl IntoIterator<Item = u32>) -> Vec<u8> {
    let mut text = String::new();
    for id in ids {
        text.push_str(&format!("{id}\n"));
    }
    text.into_bytes()
}

fn manifest_with_normalized_paths(path: &Path) -> Result<Vec<u8>, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("read manifest {}: {error}", path.display()))?;
    let mut dir_fields = 0usize;
    let mut source_fields = 0usize;
    let mut normalized = String::with_capacity(raw.len());
    for line in raw.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let newline = if line.ends_with('\n') { "\n" } else { "" };
        if content.starts_with("dir=") {
            dir_fields += 1;
            normalized.push_str("dir=<PATH>");
        } else if content.starts_with("source=") {
            source_fields += 1;
            normalized.push_str("source=<SOURCE>");
        } else {
            normalized.push_str(content);
        }
        normalized.push_str(newline);
    }
    if dir_fields != 1 || source_fields != 1 {
        return Err(format!(
            "manifest must contain exactly one dir and source field (got {dir_fields} and {source_fields})"
        ));
    }
    Ok(normalized.into_bytes())
}

fn archive_table_text(coverage: &ArchiveCoverage, store_entries: &[StoreEntry]) -> Vec<u8> {
    let mut text = String::from("id\tversion\tcrc\tstore_size\tstore_sector\tstore_present\n");
    for (id, (&version, &crc)) in coverage.versions.iter().zip(&coverage.crcs).enumerate() {
        let entry = store_entries.get(id).copied();
        let present = entry.is_some_and(|entry| entry.size > 0 && entry.sector > 0);
        let (size, sector) = entry.map_or((0, 0), |entry| (entry.size, entry.sector));
        text.push_str(&format!(
            "{id}\t{version}\t{crc}\t{size}\t{sector}\t{}\n",
            u8::from(present)
        ));
    }
    text.into_bytes()
}

fn coverage_text(
    revision: &str,
    jags: &[(String, JagCoverage)],
    archives: &[ArchiveCoverage],
) -> Vec<u8> {
    let mut text = String::from(
        "kind\trevision\tname\tlayout_or_store_index\ttotal\tdecoded_or_required\tzero_skips\tmissing\tduplicates\tbytes\tdetail\n",
    );
    for (name, jag) in jags {
        text.push_str(&format!(
            "jag\t{revision}\t{name}\t{}\t{}\t{}\t0\t{}\t{}\t{}\touter_decoded_bytes={}\n",
            jag.layout,
            jag.members,
            jag.decoded,
            jag.missing,
            jag.duplicate_hashes,
            jag.input_bytes,
            jag.outer_decoded_bytes
                .map_or_else(|| "n/a".to_string(), |bytes| bytes.to_string())
        ));
    }
    for archive in archives {
        let (snapshot_total, snapshot_unpacked, snapshot_skipped, snapshot_bytes) = archive
            .snapshot_stats
            .as_ref()
            .map_or((0, 0, 0, 0), |stats| {
                (stats.total, stats.unpacked, stats.skipped, stats.bytes)
            });
        let missing = archive.missing_store.len() + archive.missing_ondemand.len();
        text.push_str(&format!(
            "archive\t{revision}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\tstore_entries={},missing_ondemand={},ondemand_records={},ondemand_bytes={},snapshot_total={},snapshot_unpacked={},snapshot_skipped={},snapshot_bytes={},timeout={}\n",
            archive.spec.name,
            archive.spec.store_index,
            archive.versions.len(),
            archive.ondemand_records.len(),
            archive.zero_versions,
            missing,
            archive.ondemand_duplicates,
            snapshot_bytes,
            archive.store_entries,
            archive.missing_ondemand.len(),
            archive.ondemand_records.len(),
            archive.ondemand_bytes,
            snapshot_total,
            snapshot_unpacked,
            snapshot_skipped,
            snapshot_bytes,
            u8::from(archive.ondemand_timeout)
        ));
    }
    text.into_bytes()
}

fn check_snapshot_stats(
    archive: &ArchiveCoverage,
    stats: &ArchiveStats,
    snapshot_bytes: usize,
) -> Result<(), String> {
    let required_count = archive.required.len() as u32;
    if stats.total as usize != archive.versions.len()
        || stats.unpacked != required_count
        || stats.skipped as usize != archive.zero_versions
        || stats.bytes != snapshot_bytes as u64
    {
        return Err(format!(
            "snapshot {} coverage differs from versionlist: total={}/{}, unpacked={}/{required_count}, skipped={}/{}, bytes={}/{}",
            archive.spec.name,
            stats.total,
            archive.versions.len(),
            stats.unpacked,
            stats.skipped,
            archive.zero_versions,
            stats.bytes,
            snapshot_bytes
        ));
    }
    Ok(())
}

fn run_corpus_revision(revision: &str, revision_kind: ClientRevision) -> Result<(), String> {
    let corpus_root = corpus_root()?;
    let source_root = corpus_root.join(revision);
    let client_dir = source_root.join("client");
    if !source_root.is_dir() {
        return Err(format!(
            "required frozen corpus revision is missing: {}",
            source_root.display()
        ));
    }
    let capture_root = capture_root()?;
    let baseline_root = baseline_root()?.map(|root| root.join("corpus").join(revision));
    let mut capture = CaptureSet::new(group_path(&capture_root, &format!("corpus/{revision}")))?;
    let mut jag_coverage = Vec::with_capacity(JAGS.len());
    let mut versionlist = None;
    for name in JAGS {
        let path = client_dir.join(name);
        let bytes = fs::read(&path)
            .map_err(|error| format!("required corpus JAG {}: {error}", path.display()))?;
        let (jag, coverage) = capture_jag_bytes(&mut capture, name, &bytes)?;
        if name == "versionlist" {
            versionlist = Some(jag);
        }
        jag_coverage.push((name.to_string(), coverage));
    }
    let versionlist = versionlist.ok_or_else(|| "versionlist JAG was not loaded".to_string())?;
    let outer_layouts = jag_coverage
        .iter()
        .filter(|(_, jag)| jag.layout == "outer-compressed")
        .count();
    let member_layouts = jag_coverage
        .iter()
        .filter(|(_, jag)| jag.layout == "member-compressed")
        .count();
    let mut failures = Vec::new();
    if outer_layouts == 0 || member_layouts == 0 {
        failures.push(format!(
            "revision {revision} does not cover both JAG layouts (outer={outer_layouts}, member={member_layouts})"
        ));
    }

    let mut archive_coverage = Vec::with_capacity(ARCHIVES.len());
    for spec in ARCHIVES {
        let (versions, crcs) = parse_version_table(&versionlist, spec)?;
        let required = versions
            .iter()
            .enumerate()
            .filter_map(|(id, &version)| (version != 0).then_some(id as i32))
            .collect::<Vec<_>>();
        let zero_versions = versions.iter().filter(|&&version| version == 0).count();
        let store_entries = read_store_index(&source_root, spec.store_index)?;
        let missing_store = required
            .iter()
            .copied()
            .filter(|&id| {
                store_entries
                    .get(id as usize)
                    .is_none_or(|entry| entry.size == 0 || entry.sector == 0)
            })
            .collect::<Vec<_>>();
        let coverage = ArchiveCoverage {
            spec,
            versions,
            crcs,
            required,
            zero_versions,
            store_entries: store_entries.len(),
            missing_store,
            ondemand_records: BTreeMap::new(),
            ondemand_duplicates: 0,
            missing_ondemand: Vec::new(),
            ondemand_timeout: false,
            ondemand_bytes: 0,
            snapshot_stats: None,
        };
        capture.write(
            &format!("coverage/{}-versions.tsv", spec.name),
            &archive_table_text(&coverage, &store_entries),
        )?;
        archive_coverage.push(coverage);
    }

    let port = unused_local_port()?;
    let cache_dir_text = client_dir
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 cache path {}", client_dir.display()))?;
    let mut source = OnDemand::new_with_revision(
        &versionlist,
        "127.0.0.1",
        port,
        cache_dir_text,
        Arc::new(AtomicBool::new(false)),
        revision_kind,
    )
    .ok_or_else(|| format!("revision {revision}: production OnDemand rejected versionlist"))?;
    let mut proceed = true;
    for archive in &mut archive_coverage {
        if proceed {
            let (records, duplicates, timed_out) = fetch_archive(
                &mut source,
                archive.spec.ondemand_archive,
                &archive.required,
                Duration::from_secs(5),
            );
            archive.ondemand_records = records;
            archive.ondemand_duplicates = duplicates;
            archive.ondemand_timeout = timed_out;
            proceed = !timed_out;
        } else {
            archive.ondemand_timeout = true;
        }
        let actual = archive
            .ondemand_records
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        let expected = archive.required.iter().copied().collect::<BTreeSet<_>>();
        archive.missing_ondemand = expected.difference(&actual).copied().collect();
        let unexpected = actual.difference(&expected).copied().collect::<Vec<_>>();
        if !unexpected.is_empty() {
            failures.push(format!(
                "revision {revision}, archive {}, unexpected OnDemand ids {unexpected:?}",
                archive.spec.name
            ));
        }
        let stream = record_stream(&archive.ondemand_records)?;
        archive.ondemand_bytes = stream.len();
        let ids = archive
            .ondemand_records
            .keys()
            .map(|&id| u32::try_from(id).expect("OnDemand ids are nonnegative"))
            .collect::<Vec<_>>();
        capture.write(
            &format!("ondemand/{}-records.bin", archive.spec.name),
            &stream,
        )?;
        capture.write(
            &format!("ondemand/{}-ids.txt", archive.spec.name),
            &ids_text(ids),
        )?;
        if archive.ondemand_timeout {
            failures.push(format!(
                "revision {revision}, archive {}: production OnDemand timed out with {} requests remaining",
                archive.spec.name,
                source.remaining()
            ));
        }
        if !archive.missing_store.is_empty() || !archive.missing_ondemand.is_empty() {
            failures.push(format!(
                "revision {revision}, archive {}: {} missing local store records and {} missing OnDemand records",
                archive.spec.name,
                archive.missing_store.len(),
                archive.missing_ondemand.len()
            ));
        }
        if archive.ondemand_duplicates != 0 {
            failures.push(format!(
                "revision {revision}, archive {}: {} duplicate OnDemand record ids",
                archive.spec.name, archive.ondemand_duplicates
            ));
        }
    }
    source.stop();

    let work = WorkDir::new(&format!("corpus-snapshot-{revision}"))?;
    let snapshot_root = work.path().join("snapshot-output");
    let snapshot_root_text = snapshot_root
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 snapshot path {}", snapshot_root.display()))?;
    let source_root_text = source_root
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 store path {}", source_root.display()))?;
    let client_dir_text = client_dir
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 cache path {}", client_dir.display()))?;
    match unpack_cache_from_store(client_dir_text, snapshot_root_text, source_root_text) {
        Ok(result) => {
            match snapshot_state(client_dir_text, snapshot_root_text) {
                Ok(SnapshotState::Ready(ready)) => {
                    if result != ready {
                        failures.push(format!(
                            "revision {revision}: unpack result and snapshot readiness manifest differ"
                        ));
                    }
                }
                Ok(other) => failures.push(format!(
                    "revision {revision}: unpack succeeded but snapshot state is {}",
                    other.describe()
                )),
                Err(error) => failures.push(format!(
                    "revision {revision}: snapshot state inspection failed: {error}"
                )),
            }
            let manifest_path = Path::new(&result.dir).join("manifest");
            match manifest_with_normalized_paths(&manifest_path) {
                Ok(normalized) => capture.write("snapshot/manifest.normalized", &normalized)?,
                Err(error) => failures.push(format!("revision {revision}: {error}")),
            }
            for archive in &mut archive_coverage {
                let path = Path::new(&result.dir).join(archive.spec.snapshot_file);
                match fs::read(&path) {
                    Ok(bytes) => {
                        let ids = match parse_record_ids(&bytes) {
                            Ok(ids) => ids,
                            Err(error) => {
                                failures.push(format!(
                                    "revision {revision}, archive {}: invalid snapshot record stream: {error}",
                                    archive.spec.name
                                ));
                                Vec::new()
                            }
                        };
                        let unique = ids.iter().copied().collect::<BTreeSet<_>>();
                        if unique.len() != ids.len() {
                            failures.push(format!(
                                "revision {revision}, archive {}: snapshot stream contains duplicate ids",
                                archive.spec.name
                            ));
                        }
                        let expected_ids = archive
                            .required
                            .iter()
                            .map(|&id| u32::try_from(id).expect("versionlist ids are nonnegative"))
                            .collect::<BTreeSet<_>>();
                        if unique != expected_ids {
                            failures.push(format!(
                                "revision {revision}, archive {}: snapshot id set differs from required versionlist ids",
                                archive.spec.name
                            ));
                        }
                        let ondemand = record_stream(&archive.ondemand_records)?;
                        if let Some(offset) = first_difference(&bytes, &ondemand) {
                            failures.push(byte_difference_report(
                                revision,
                                &format!("snapshot/{}", archive.spec.snapshot_file),
                                offset,
                                &bytes,
                                &ondemand,
                            ));
                        }
                        capture
                            .write(&format!("snapshot/{}", archive.spec.snapshot_file), &bytes)?;
                        capture.write(
                            &format!("snapshot/{}-ids.txt", archive.spec.name),
                            &ids_text(ids),
                        )?;
                        if let Some(stats) = manifest_stats(&result, archive.spec.name) {
                            if let Err(error) = check_snapshot_stats(archive, stats, bytes.len()) {
                                failures.push(format!("revision {revision}: {error}"));
                            }
                            archive.snapshot_stats = Some(stats.clone());
                        }
                    }
                    Err(error) => failures.push(format!(
                        "revision {revision}, archive {}: read snapshot {}: {error}",
                        archive.spec.name,
                        path.display()
                    )),
                }
            }
        }
        Err(error) => {
            capture.write("snapshot/status.txt", b"hard-error\n")?;
            failures.push(format!(
                "revision {revision}: local-store snapshot failed: {error}"
            ));
        }
    }

    capture.write(
        "coverage.tsv",
        &coverage_text(revision, &jag_coverage, &archive_coverage),
    )?;
    capture.finish(baseline_root.as_deref(), revision)?;
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    Ok(())
}

fn manifest_stats<'a>(manifest: &'a Manifest, name: &str) -> Option<&'a ArchiveStats> {
    match name {
        "models" => Some(&manifest.models),
        "anims" => Some(&manifest.anims),
        "midi" => Some(&manifest.midi),
        "maps" => Some(&manifest.maps),
        _ => None,
    }
}

fn unused_local_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("reserve local OnDemand port: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("read local OnDemand port: {error}"))?
        .port();
    drop(listener);
    Ok(port)
}

fn make_stored_gzip(input: &[u8]) -> Vec<u8> {
    let mut gzip = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    if input.is_empty() {
        gzip.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    } else {
        for (index, block) in input.chunks(u16::MAX as usize).enumerate() {
            let final_block = (index + 1) * u16::MAX as usize >= input.len();
            let length = u16::try_from(block.len()).expect("stored block fits u16");
            gzip.push(u8::from(final_block));
            gzip.extend_from_slice(&length.to_le_bytes());
            gzip.extend_from_slice(&(!length).to_le_bytes());
            gzip.extend_from_slice(block);
        }
    }
    let crc = Packet::getcrc(input, 0, input.len()) as u32;
    gzip.extend_from_slice(&crc.to_le_bytes());
    gzip.extend_from_slice(&(input.len() as u32).to_le_bytes());
    gzip
}

fn large_gzip_payload() -> Vec<u8> {
    (0usize..70_000)
        .map(|index| (index.wrapping_mul(73).wrapping_add(index / 251) & 0xff) as u8)
        .collect()
}

struct GzipCase {
    name: &'static str,
    gzip: Vec<u8>,
    output: Vec<u8>,
    version: u16,
}

fn gzip_cases() -> Vec<GzipCase> {
    let empty = make_stored_gzip(&[]);
    let large_output = large_gzip_payload();
    let large = make_stored_gzip(&large_output);
    let mut trailing = make_stored_gzip(b"trailing-member");
    trailing.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let mut bad_deflate = make_stored_gzip(b"bad-deflate-payload");
    bad_deflate[10] = (bad_deflate[10] & 0xf8) | 0x06;
    let mut bad_crc = make_stored_gzip(b"bad-gzip-crc");
    let crc_offset = bad_crc.len() - 8;
    bad_crc[crc_offset] ^= 1;
    vec![
        GzipCase {
            name: "empty",
            gzip: empty,
            output: Vec::new(),
            version: 1,
        },
        GzipCase {
            name: "large-multi-block",
            gzip: large,
            output: large_output,
            version: 2,
        },
        GzipCase {
            name: "trailing-bytes",
            gzip: trailing,
            output: b"trailing-member".to_vec(),
            version: 3,
        },
        GzipCase {
            name: "corrupt-deflate",
            output: bad_deflate.clone(),
            gzip: bad_deflate,
            version: 4,
        },
        GzipCase {
            name: "corrupt-gzip-crc",
            output: bad_crc.clone(),
            gzip: bad_crc,
            version: 5,
        },
    ]
}

fn stored_record(gzip: &[u8], version: u16) -> Vec<u8> {
    let mut record = Vec::with_capacity(gzip.len() + 2);
    record.extend_from_slice(gzip);
    record.extend_from_slice(&version.to_be_bytes());
    record
}

fn write_cache_bundle(
    root: &Path,
    versionlist: &[u8],
    model_records: &[Vec<u8>],
) -> Result<PathBuf, String> {
    let client_dir = root.join("client");
    fs::create_dir_all(&client_dir)
        .map_err(|error| format!("create synthetic cache {}: {error}", client_dir.display()))?;
    fs::write(client_dir.join("versionlist"), versionlist)
        .map_err(|error| format!("write synthetic versionlist: {error}"))?;
    for name in JAGS {
        if name != "versionlist" {
            fs::write(client_dir.join(name), b"\0\0\x02\0\0\x02\0\0")
                .map_err(|error| format!("write synthetic {name} JAG: {error}"))?;
        }
    }
    fs::write(root.join("main_file_cache.idx0"), b"")
        .map_err(|error| format!("write synthetic idx0: {error}"))?;
    let (data, model_index) = build_model_store(model_records)?;
    fs::write(root.join("main_file_cache.dat"), data)
        .map_err(|error| format!("write synthetic cache data: {error}"))?;
    fs::write(root.join("main_file_cache.idx1"), model_index)
        .map_err(|error| format!("write synthetic model index: {error}"))?;
    for index in 2..=4 {
        fs::write(root.join(format!("main_file_cache.idx{index}")), b"")
            .map_err(|error| format!("write synthetic idx{index}: {error}"))?;
    }
    Ok(client_dir)
}

fn build_model_store(records: &[Vec<u8>]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut data = vec![0u8; 520];
    let mut index = vec![0u8; records.len() * 6];
    for (file_id, payload) in records.iter().enumerate() {
        if payload.is_empty() {
            return Err(format!(
                "synthetic store record {file_id} must not be empty"
            ));
        }
        let file_id = u16::try_from(file_id)
            .map_err(|_| "synthetic store record id exceeds u16".to_string())?;
        let size = u32::try_from(payload.len())
            .map_err(|_| format!("synthetic store record {file_id} exceeds u32"))?;
        if size > 0x00ff_ffff {
            return Err(format!(
                "synthetic store record {file_id} exceeds 24-bit size"
            ));
        }
        let mut first_sector = 0usize;
        for (part, bytes) in payload.chunks(512).enumerate() {
            let sector = data.len() / 520;
            if first_sector == 0 {
                first_sector = sector;
            }
            let next_sector = if (part + 1) * 512 < payload.len() {
                sector + 1
            } else {
                0
            };
            let mut block = [0u8; 520];
            block[..2].copy_from_slice(&file_id.to_be_bytes());
            block[2..4].copy_from_slice(
                &u16::try_from(part)
                    .map_err(|_| format!("synthetic store record {file_id} has too many sectors"))?
                    .to_be_bytes(),
            );
            block[4] = ((next_sector >> 16) & 0xff) as u8;
            block[5] = ((next_sector >> 8) & 0xff) as u8;
            block[6] = (next_sector & 0xff) as u8;
            block[7] = 2; // store index 1 is archive id 2 in the data sectors.
            block[8..8 + bytes.len()].copy_from_slice(bytes);
            data.extend_from_slice(&block);
        }
        let entry = &mut index[file_id as usize * 6..file_id as usize * 6 + 6];
        entry[0] = ((size >> 16) & 0xff) as u8;
        entry[1] = ((size >> 8) & 0xff) as u8;
        entry[2] = (size & 0xff) as u8;
        entry[3] = ((first_sector >> 16) & 0xff) as u8;
        entry[4] = ((first_sector >> 8) & 0xff) as u8;
        entry[5] = (first_sector & 0xff) as u8;
    }
    Ok((data, index))
}

fn new_on_demand(
    versionlist: &[u8],
    cache_dir: &Path,
    port: u16,
    revision: ClientRevision,
) -> Result<OnDemand, String> {
    let jag = JagFile::new(versionlist.to_vec());
    let cache_dir = cache_dir
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 cache path {}", cache_dir.display()))?;
    OnDemand::new_with_revision(
        &jag,
        "127.0.0.1",
        port,
        cache_dir,
        Arc::new(AtomicBool::new(false)),
        revision,
    )
    .ok_or_else(|| "production OnDemand rejected fixture versionlist".to_string())
}

fn fixture_versions(jag: &JagFile) -> Result<(Vec<i32>, Vec<i32>), String> {
    parse_version_table(
        jag,
        ArchiveSpec {
            name: "models",
            version_table: "model_version",
            crc_table: "model_crc",
            store_index: 1,
            ondemand_archive: 0,
            snapshot_file: "models.bin",
        },
    )
}

fn empty_network_response(
    versionlist: &[u8],
    cache_dir: &Path,
    port: u16,
) -> Result<Vec<u8>, String> {
    let mut source = new_on_demand(versionlist, cache_dir, port, ClientRevision::R274)?;
    source.request(0, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut result = None;
    let mut duplicate = false;
    while source.remaining() > 0 {
        source.run(false);
        while let Some(mut request) = source.loop_request() {
            if request.archive == 0 && request.file == 0 {
                if result.is_some() {
                    duplicate = true;
                }
                result = request.data.take();
            }
        }
        if source.remaining() == 0 {
            break;
        }
        if Instant::now() >= deadline {
            source.stop();
            return Err("OnDemand fallback request timed out".to_string());
        }
        thread::sleep(Duration::from_millis(1));
    }
    source.stop();
    if duplicate {
        return Err("OnDemand fallback produced duplicate (0, 0) records".to_string());
    }
    result.ok_or_else(|| "OnDemand fallback completed without data".to_string())
}

type ResponseWorker = thread::JoinHandle<Result<(i32, i32), String>>;

fn serve_one_entry(
    payload: Vec<u8>,
    expected_archive: i32,
    expected_file: i32,
) -> Result<(u16, ResponseWorker), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("bind local fallback server: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("set local fallback listener nonblocking: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("read local fallback port: {error}"))?
        .port();
    let handle = thread::spawn(move || -> Result<(i32, i32), String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut socket, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(
                            "OnDemand fallback server timed out waiting for connection".into()
                        );
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(format!("accept OnDemand fallback: {error}")),
            }
        };
        // The accept loop is nonblocking; the handshake below uses blocking I/O.
        socket
            .set_nonblocking(false)
            .map_err(|error| format!("set fallback socket blocking mode: {error}"))?;
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| format!("set fallback socket timeout: {error}"))?;
        let mut handshake = [0u8; 1];
        socket
            .read_exact(&mut handshake)
            .map_err(|error| format!("read OnDemand handshake: {error}"))?;
        if handshake[0] != 15 {
            return Err(format!(
                "unexpected OnDemand handshake byte {}",
                handshake[0]
            ));
        }
        socket
            .write_all(&[0; 8])
            .map_err(|error| format!("write OnDemand handshake: {error}"))?;
        let mut request = [0u8; 4];
        socket
            .read_exact(&mut request)
            .map_err(|error| format!("read OnDemand request: {error}"))?;
        let archive = i32::from(request[0]);
        let file = (i32::from(request[1]) << 8) | i32::from(request[2]);
        if (archive, file) != (expected_archive, expected_file) {
            return Err(format!(
                "unexpected OnDemand request ({archive}, {file}), expected ({expected_archive}, {expected_file})"
            ));
        }
        let length = u16::try_from(payload.len())
            .map_err(|_| "fallback payload exceeds update chunk size".to_string())?;
        let mut chunk = vec![archive as u8, (file >> 8) as u8, file as u8];
        chunk.extend_from_slice(&length.to_be_bytes());
        chunk.push(0);
        chunk.extend_from_slice(&payload);
        socket
            .write_all(&chunk)
            .map_err(|error| format!("write OnDemand fallback payload: {error}"))?;
        Ok((archive, file))
    });
    Ok((port, handle))
}

fn synthetic_one_member_jag(member: &[u8], declared_size: usize) -> Vec<u8> {
    let mut body = Vec::with_capacity(12 + member.len());
    body.extend_from_slice(&1u16.to_be_bytes());
    body.extend_from_slice(&JagFile::gen_hash("payload").to_be_bytes());
    append_u24(&mut body, declared_size);
    append_u24(&mut body, member.len());
    body.extend_from_slice(member);
    let mut jag = Vec::with_capacity(body.len() + 6);
    append_u24(&mut jag, body.len());
    append_u24(&mut jag, body.len());
    jag.extend_from_slice(&body);
    jag
}

fn run_fixed_bzip() -> Result<(), String> {
    let capture_root = capture_root()?;
    let baseline_root = baseline_root()?.map(|root| root.join("fixed/bzip"));
    let mut capture = CaptureSet::new(group_path(&capture_root, "fixed/bzip"))?;
    let cases = [
        ("empty", frozen_hex(BZ_EMPTY), Vec::new()),
        (
            "bzh1-small",
            frozen_hex(BZ_SMALL),
            b"fixed JAG payload".to_vec(),
        ),
        (
            "bzh1-multiblock",
            frozen_hex(BZ_MULTI),
            b"AB".repeat(150_000),
        ),
        (
            "bzh9-fallback",
            frozen_hex(BZ_LEVEL_9),
            b"AB".repeat(100_000),
        ),
    ];
    for (name, compressed, expected) in cases {
        capture.write(&format!("inputs/{name}.bz"), &compressed)?;
        let decoded = try_bunzip2(&compressed)
            .map_err(|error| format!("fixed bzip {name} rejected: {error}"))?;
        assert_bytes_equal(&format!("fixed bzip {name}"), &decoded, &expected)?;
        capture.write(&format!("outputs/{name}.bin"), &decoded)?;
    }

    let small = frozen_hex(BZ_SMALL);
    let expected_small = b"fixed JAG payload";
    let mut outcome = String::new();
    for (name, declared, expected) in [
        ("zero-bound", 0usize, Vec::new()),
        (
            "short-bound",
            expected_small.len() - 2,
            expected_small[..expected_small.len() - 2].to_vec(),
        ),
        ("long-bound", expected_small.len() + 3, {
            let mut bytes = expected_small.to_vec();
            bytes.resize(expected_small.len() + 3, 0);
            bytes
        }),
    ] {
        let jag_bytes = synthetic_one_member_jag(&small, declared);
        capture.write(&format!("inputs/jag-{name}.bin"), &jag_bytes)?;
        let jag = JagFile::new(jag_bytes);
        let actual = jag
            .read_index(0)
            .ok_or_else(|| format!("JagFile omitted {name} member"))?;
        assert_bytes_equal(&format!("JagFile {name}"), &actual, &expected)?;
        capture.write(&format!("outputs/jag-{name}.bin"), &actual)?;
        outcome.push_str(&format!("{name}=ok\n"));
    }
    let empty_jag = synthetic_one_member_jag(&frozen_hex(BZ_EMPTY), 0);
    let empty_member = JagFile::new(empty_jag.clone())
        .read_index(0)
        .ok_or_else(|| "JagFile omitted zero-length bzip member".to_string())?;
    if !empty_member.is_empty() {
        return Err("zero-length declared bzip output was not empty".into());
    }
    capture.write("inputs/jag-empty-packed-zero-bound.bin", &empty_jag)?;
    capture.write("outputs/jag-empty-packed-zero-bound.bin", &empty_member)?;

    let original = frozen_hex(BZ_MULTI);
    let mut crc_bad = original.clone();
    crc_bad[6] ^= 0x80;
    let mut block_bad = original.clone();
    block_bad[0] ^= 0x01;
    let panic_result = std::panic::catch_unwind(|| bunzip2(&crc_bad));
    let mut malformed = vec![
        ("corrupt-crc".to_string(), crc_bad),
        ("corrupt-block-magic".to_string(), block_bad),
    ];
    let small_stream = frozen_hex(BZ_SMALL);
    for (index, end) in [0, 1, 5, small_stream.len() / 2, small_stream.len() - 1]
        .into_iter()
        .enumerate()
    {
        malformed.push((format!("truncated-{index}"), small_stream[..end].to_vec()));
    }
    for (name, bytes) in malformed {
        capture.write(&format!("inputs/{name}.bz"), &bytes)?;
        if try_bunzip2(&bytes).is_ok() {
            return Err(format!(
                "fixed malformed bzip input {name} unexpectedly decoded"
            ));
        }
        outcome.push_str(&format!("{name}=fallible-error\n"));
    }
    if panic_result.is_ok() {
        return Err("bunzip2 panic wrapper returned on corrupt CRC".into());
    }
    outcome.push_str("bunzip2-corrupt-crc=panic\n");
    capture.write("outcomes.txt", outcome.as_bytes())?;
    capture.finish(baseline_root.as_deref(), "fixed bzip")
}

fn strict_snapshot_outcome(
    capture: &mut CaptureSet,
    label: &str,
    versionlist: &[u8],
    stored_entry: &[u8],
) -> Result<(), String> {
    let work = WorkDir::new(&format!("gzip-strict-{label}"))?;
    let client_dir = write_cache_bundle(work.path(), versionlist, &[stored_entry.to_vec()])?;
    let output = work.path().join("snapshot-output");
    let client_dir_text = client_dir
        .to_str()
        .ok_or_else(|| "non-UTF-8 strict fixture cache path".to_string())?;
    let output_text = output
        .to_str()
        .ok_or_else(|| "non-UTF-8 strict fixture output path".to_string())?;
    let work_text = work
        .path()
        .to_str()
        .ok_or_else(|| "non-UTF-8 strict fixture root".to_string())?;
    match unpack_cache_from_store(client_dir_text, output_text, work_text) {
        Ok(_) => {
            capture.write(
                &format!("snapshot-strict/{label}.txt"),
                b"unexpected-success\n",
            )?;
            return Err(format!(
                "snapshot unexpectedly accepted malformed gzip {label}"
            ));
        }
        Err(error) => {
            let message = error.to_string();
            if !message.contains("corrupt gzip stream") {
                capture.write(
                    &format!("snapshot-strict/{label}.txt"),
                    b"wrong-hard-error\n",
                )?;
                return Err(format!(
                    "snapshot {label} failed for an unexpected reason: {message}"
                ));
            }
            capture.write(&format!("snapshot-strict/{label}.txt"), b"hard-error\n")?;
        }
    }
    let port = unused_local_port()?;
    let decoded = empty_network_response(versionlist, &client_dir, port)?;
    capture.write(&format!("ondemand-fallback/{label}.bin"), &decoded)?;
    let gzip_body = stored_entry
        .get(..stored_entry.len().saturating_sub(2))
        .unwrap_or(stored_entry);
    assert_bytes_equal(
        &format!("OnDemand malformed gzip fallback {label}"),
        &decoded,
        gzip_body,
    )
}

fn run_fixed_gzip() -> Result<(), String> {
    let capture_root = capture_root()?;
    let baseline_root = baseline_root()?.map(|root| root.join("fixed/gzip"));
    let mut capture = CaptureSet::new(group_path(&capture_root, "fixed/gzip"))?;
    // The frozen constants contain headerless bzip payloads. Their decoded
    // JAG body lengths are 130 bytes here and 106 bytes in the two cases below.
    let versionlist = frozen_outer_jag(GZIP_CASES_VERSIONLIST, 130);
    let (versionlist_jag, versionlist_coverage) =
        capture_jag_bytes(&mut capture, "gzip-cases-versionlist", &versionlist)?;
    if versionlist_coverage.layout != "outer-compressed" {
        return Err("frozen gzip versionlist no longer exercises outer compression".into());
    }
    let (versions, crcs) = fixture_versions(&versionlist_jag)?;
    let cases = gzip_cases();
    if versions
        != cases
            .iter()
            .map(|case| case.version as i32)
            .collect::<Vec<_>>()
    {
        return Err(format!("frozen gzip fixture versions differ: {versions:?}"));
    }
    for (index, case) in cases.iter().enumerate() {
        let actual_crc = Packet::getcrc(&case.gzip, 0, case.gzip.len());
        if crcs.get(index).copied() != Some(actual_crc) {
            return Err(format!(
                "frozen gzip fixture CRC differs for {}: table={:?}, actual={actual_crc}",
                case.name,
                crcs.get(index)
            ));
        }
        capture.write(&format!("inputs/{}.gz", case.name), &case.gzip)?;
        let record = stored_record(&case.gzip, case.version);
        capture.write(&format!("inputs/{}.store-record", case.name), &record)?;
    }
    let work = WorkDir::new("gzip-cases")?;
    let model_records = cases
        .iter()
        .map(|case| stored_record(&case.gzip, case.version))
        .collect::<Vec<_>>();
    let client_dir = write_cache_bundle(work.path(), &versionlist, &model_records)?;
    let port = unused_local_port()?;
    let mut source = new_on_demand(&versionlist, &client_dir, port, ClientRevision::R274)?;
    let expected = (0..cases.len()).map(|id| id as i32).collect::<Vec<_>>();
    let (records, duplicates, timed_out) =
        fetch_archive(&mut source, 0, &expected, Duration::from_secs(5));
    source.stop();
    if timed_out || duplicates != 0 || records.len() != cases.len() {
        return Err(format!(
            "fixture OnDemand coverage incomplete: records={}, expected={}, duplicates={duplicates}, timeout={timed_out}",
            records.len(),
            cases.len()
        ));
    }
    let stream = record_stream(&records)?;
    capture.write("ondemand/models.bin", &stream)?;
    capture.write(
        "ondemand/models-ids.txt",
        &ids_text(records.keys().map(|&id| id as u32)),
    )?;
    for (index, case) in cases.iter().enumerate() {
        let actual = records
            .get(&(index as i32))
            .ok_or_else(|| format!("OnDemand omitted gzip case {}", case.name))?;
        assert_bytes_equal(
            &format!("OnDemand gzip case {}", case.name),
            actual,
            &case.output,
        )?;
        capture.write(&format!("outputs/{}.bin", case.name), actual)?;
    }

    for (label, fixture_hex, case_index) in [
        ("corrupt-deflate", BAD_DEFLATE_VERSIONLIST, 3usize),
        ("corrupt-gzip-crc", BAD_GZIP_CRC_VERSIONLIST, 4usize),
    ] {
        let malformed_versionlist = frozen_outer_jag(fixture_hex, 106);
        let (malformed_jag, coverage) = capture_jag_bytes(
            &mut capture,
            &format!("{label}-versionlist"),
            &malformed_versionlist,
        )?;
        if coverage.layout != "outer-compressed" {
            return Err(format!("{label} versionlist is not outer-compressed"));
        }
        let (malformed_versions, malformed_crcs) = fixture_versions(&malformed_jag)?;
        if malformed_versions != [1] {
            return Err(format!(
                "{label} versionlist must contain one required record"
            ));
        }
        let case = &cases[case_index];
        let actual_crc = Packet::getcrc(&case.gzip, 0, case.gzip.len());
        if malformed_crcs.first().copied() != Some(actual_crc) {
            return Err(format!(
                "{label} versionlist CRC does not match the fixed gzip"
            ));
        }
        let record = stored_record(&case.gzip, 1);
        strict_snapshot_outcome(&mut capture, label, &malformed_versionlist, &record)?;
    }

    let valid_empty = stored_record(&cases[0].gzip, cases[0].version);
    let mut wrong_version = valid_empty.clone();
    let trailer = wrong_version.len() - 2;
    wrong_version[trailer..].copy_from_slice(&2u16.to_be_bytes());
    let mut wrong_crc = valid_empty.clone();
    wrong_crc[10] ^= 1;
    for (label, invalid_record) in [
        ("version-mismatch", wrong_version),
        ("crc-mismatch", wrong_crc),
    ] {
        let work = WorkDir::new(&format!("gzip-{label}"))?;
        let mut records = model_records.clone();
        records[0] = invalid_record.clone();
        let client_dir = write_cache_bundle(work.path(), &versionlist, &records)?;
        let output = work.path().join("snapshot-output");
        let client_dir_text = client_dir
            .to_str()
            .ok_or_else(|| "non-UTF-8 mismatch cache path".to_string())?;
        let output_text = output
            .to_str()
            .ok_or_else(|| "non-UTF-8 mismatch output path".to_string())?;
        let work_text = work
            .path()
            .to_str()
            .ok_or_else(|| "non-UTF-8 mismatch root".to_string())?;
        let snapshot_error = unpack_cache_from_store(client_dir_text, output_text, work_text)
            .err()
            .ok_or_else(|| format!("snapshot accepted {label} local record"))?;
        if !snapshot_error.to_string().contains("version/CRC check") {
            return Err(format!(
                "snapshot {label} failed for an unexpected reason: {snapshot_error}"
            ));
        }
        capture.write(&format!("inputs/{label}.store-record"), &invalid_record)?;
        capture.write(&format!("snapshot-strict/{label}.txt"), b"hard-error\n")?;

        let (port, server) = serve_one_entry(valid_empty.clone(), 0, 0)?;
        let recovered = empty_network_response(&versionlist, &client_dir, port);
        let served = server
            .join()
            .map_err(|_| format!("fallback server panicked for {label}"))?
            .map_err(|error| format!("{label} fallback server: {error}"))?;
        let recovered = recovered.map_err(|error| format!("{label} fallback client: {error}"))?;
        if served != (0, 0) {
            return Err(format!(
                "fallback server served unexpected entry {served:?}"
            ));
        }
        assert_bytes_equal(&format!("OnDemand {label} fallback"), &recovered, &[])?;
        capture.write(&format!("ondemand-fallback/{label}.bin"), &recovered)?;
    }
    capture.finish(baseline_root.as_deref(), "fixed gzip")
}

fn run_cache_corpus() -> Result<(), String> {
    let root = corpus_root()?;
    if !root.is_dir() {
        return Err(format!(
            "required frozen corpus root is missing: {}",
            root.display()
        ));
    }
    run_corpus_revision("274", ClientRevision::R274)?;
    run_corpus_revision("289", ClientRevision::R289)
}

fn run_fixed_fixtures() -> Result<(), String> {
    run_fixed_bzip()?;
    run_fixed_gzip()
}

#[test]
#[ignore = "explicit DEPS-BATCH-025 evidence-corpus job; see module documentation"]
fn frozen_274_and_289_caches_match_the_selected_baseline() {
    run_cache_corpus().unwrap_or_else(|error| panic!("cache corpus differential failed:\n{error}"));
}

#[test]
#[ignore = "explicit DEPS-BATCH-025 evidence-corpus job; see module documentation"]
fn fixed_bzip_and_gzip_contract_fixtures_are_baseline_comparable() {
    run_fixed_fixtures()
        .unwrap_or_else(|error| panic!("fixed cache decoder fixtures failed:\n{error}"));
}
