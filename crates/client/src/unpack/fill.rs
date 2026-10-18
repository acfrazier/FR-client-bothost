//! Network cold fill in bounded steps. Every required entry of the four idx
//! archives is one position in a single sequence (archive, then index
//! order). The fill keeps up to [`FILL_WINDOW`] entries requested ahead of the
//! oldest unwritten one and writes records strictly in sequence order, so the
//! OnDemand worker always has queued demand and the record files are the
//! same bytes a batch-by-batch fill wrote. There is no batch barrier: the
//! worker's own limit of ten urgent requests in flight is the only pacing.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{
    create_staging_dir, encode_record, file_size, publish_staged, read_archive_tables,
    read_versionlist, snapshot_state_for_version, stage_jags, stage_manifest, verify_published,
    version_hash, ArchiveStats, ArchiveTables, EntrySource, FetchedEntry, Manifest, SnapshotState,
    UnpackError, ARCHIVES,
};
use crate::io::jagfile::JagFile;

/// Entries requested but not yet written. This is the fill's memory bound:
/// at most this many decoded payloads are held, and the worker queue never
/// holds more than this many requests from the fill.
pub const FILL_WINDOW: usize = 160;

/// No entry arriving for this long fails the fill (the caller reports the
/// degraded fallback honestly). Measured across steps, not per step.
pub(super) const FETCH_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) enum SnapshotStart {
    /// The selected version is already published under `out_dir`.
    Ready(Manifest),
    Fill(Box<NetworkSnapshot>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FillProgress {
    Continue {
        written: u32,
        total: u32,
    },
    /// Every record is written and synced; [`NetworkSnapshot::publish`] is next.
    Staged,
}

/// A network snapshot being staged under its own staging directory. Dropping
/// it before [`Self::publish`] removes that directory, so a cancelled fill
/// never publishes part of a snapshot.
pub(super) struct NetworkSnapshot {
    cache_dir: String,
    out_dir: String,
    version: String,
    versionlist: Vec<u8>,
    staging: PathBuf,
    dir_path: PathBuf,
    jags: Vec<(String, u64)>,
    window: EntryWindow,
}

impl NetworkSnapshot {
    /// Validate the selected versionlist tables, create the staging directory
    /// and copy the jag packs into it. No entry is requested yet.
    pub(super) fn begin(cache_dir: &str, out_dir: &str) -> Result<SnapshotStart, UnpackError> {
        let versionlist = read_versionlist(cache_dir)?;
        let version = version_hash(&versionlist);
        if let SnapshotState::Ready(existing) =
            snapshot_state_for_version(out_dir, &version, &versionlist)
        {
            return Ok(SnapshotStart::Ready(existing));
        }
        // Validate the source inputs before creating any output resources.
        let jag = JagFile::new(versionlist.to_vec());
        let tables = read_archive_tables(&jag)?;
        let window = EntryWindow::new(&tables);
        std::fs::create_dir_all(out_dir)
            .map_err(|error| UnpackError::io("create snapshot root", error))?;
        let staging = create_staging_dir(Path::new(out_dir), &version)?;
        let mut snapshot = NetworkSnapshot {
            cache_dir: cache_dir.to_string(),
            out_dir: out_dir.to_string(),
            dir_path: Path::new(out_dir).join(&version),
            version,
            versionlist,
            staging,
            jags: Vec::new(),
            window,
        };
        snapshot.jags = stage_jags(cache_dir, &snapshot.staging)?;
        Ok(SnapshotStart::Fill(Box::new(snapshot)))
    }

    /// Required entries in the fill.
    pub(super) fn total(&self) -> u32 {
        self.window.order.len() as u32
    }

    /// Fill until every record is staged or `until` passes. `None` waits as
    /// long as entries keep arriving (the blocking fill).
    pub(super) fn step(
        &mut self,
        source: &mut dyn EntrySource,
        until: Option<Instant>,
    ) -> Result<FillProgress, UnpackError> {
        if self.window.step(&self.staging, source, until)? {
            Ok(FillProgress::Staged)
        } else {
            Ok(FillProgress::Continue {
                written: self.window.next_write as u32,
                total: self.window.order.len() as u32,
            })
        }
    }

    /// Write the manifest and integrity metadata, then publish the staged
    /// snapshot. Only valid after [`FillProgress::Staged`].
    pub(super) fn publish(self) -> Result<Manifest, UnpackError> {
        debug_assert!(self.window.done(), "publish follows a staged fill");
        let dir = self.dir_path.to_string_lossy().into_owned();
        stage_manifest(
            &self.staging,
            &dir,
            &self.version,
            &self.versionlist,
            self.jags.clone(),
            self.window.stats.clone(),
            "update-server",
        )?;
        publish_staged(
            &self.cache_dir,
            &self.versionlist,
            &self.staging,
            &self.dir_path,
        )?;
        let _ = std::fs::remove_dir_all(&self.staging);
        verify_published(&self.out_dir, &self.version, &self.versionlist)
    }
}

impl Drop for NetworkSnapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.staging);
    }
}

struct EntryWindow {
    /// `(archive slot, file)` of every required entry: archive slot order
    /// (the OnDemand archive index), then index order. Sorted.
    order: Vec<(usize, i32)>,
    /// Sequence position where each archive's entries end.
    ends: [usize; ARCHIVES.len()],
    next_request: usize,
    next_write: usize,
    /// Payloads for `order[next_write..next_request]`, `None` until arrived.
    held: VecDeque<Option<Vec<u8>>>,
    /// Archive slot whose record file is being written.
    archive: usize,
    out: Option<File>,
    stats: [ArchiveStats; ARCHIVES.len()],
    last_progress: Option<Instant>,
    arrived: Vec<FetchedEntry>,
}

impl EntryWindow {
    fn new(tables: &[ArchiveTables; ARCHIVES.len()]) -> Self {
        let mut order = Vec::new();
        let mut ends = [0; ARCHIVES.len()];
        let mut stats: [ArchiveStats; ARCHIVES.len()] = Default::default();
        for (slot, tables) in tables.iter().enumerate() {
            order.extend(tables.required().map(|file| (slot, file)));
            ends[slot] = order.len();
            // Intentionally absent entries are counted up front so `total ==
            // unpacked + skipped` holds even when a required entry fails.
            stats[slot] = ArchiveStats {
                total: tables.total(),
                unpacked: 0,
                skipped: (0..tables.versions.len() as i32)
                    .filter(|file| tables.version(*file) == 0)
                    .count() as u32,
                bytes: 0,
            };
        }
        Self {
            order,
            ends,
            next_request: 0,
            next_write: 0,
            held: VecDeque::with_capacity(FILL_WINDOW),
            archive: 0,
            out: None,
            stats,
            last_progress: None,
            arrived: Vec::with_capacity(FILL_WINDOW),
        }
    }

    fn done(&self) -> bool {
        self.archive == ARCHIVES.len()
    }

    /// True once every archive file is written and synced.
    fn step(
        &mut self,
        staging: &Path,
        source: &mut dyn EntrySource,
        until: Option<Instant>,
    ) -> Result<bool, UnpackError> {
        let mut last_progress = *self.last_progress.get_or_insert_with(Instant::now);
        loop {
            self.write_ready(staging)?;
            if self.done() {
                return Ok(true);
            }
            self.request_ahead(source)?;
            let idle_at = last_progress + FETCH_IDLE_TIMEOUT;
            let wait_until = until.map_or(idle_at, |until| until.min(idle_at));
            source
                .take_entries(wait_until, &mut self.arrived)
                .map_err(|error| UnpackError::new(format!("update server: {error}")))?;
            let now = Instant::now();
            if self.arrived.is_empty() {
                if now >= idle_at {
                    return Err(UnpackError::new(format!(
                        "update server: no entry arrived for {FETCH_IDLE_TIMEOUT:?}; {}/{} written",
                        self.next_write,
                        self.order.len()
                    )));
                }
            } else {
                last_progress = now;
                self.last_progress = Some(now);
                self.place_arrived();
            }
            if until.is_some_and(|until| now >= until) {
                self.write_ready(staging)?;
                return Ok(self.done());
            }
        }
    }

    /// Keep the window full. The worker sends at most ten of these at once.
    fn request_ahead(&mut self, source: &mut dyn EntrySource) -> Result<(), UnpackError> {
        while self.next_request < self.order.len()
            && self.next_request - self.next_write < FILL_WINDOW
        {
            let (archive, file) = self.order[self.next_request];
            source
                .request_entry(archive as i32, file)
                .map_err(|error| UnpackError::new(format!("update server: {error}")))?;
            self.held.push_back(None);
            self.next_request += 1;
        }
        Ok(())
    }

    /// Hold each arrived payload in its window slot. Entries outside the
    /// window or already held are not this fill's demand and are dropped.
    fn place_arrived(&mut self) {
        for entry in self.arrived.drain(..) {
            let Ok(archive) = usize::try_from(entry.archive) else {
                continue;
            };
            let window = &self.order[self.next_write..self.next_request];
            if let Ok(offset) = window.binary_search(&(archive, entry.file)) {
                let slot = &mut self.held[offset];
                if slot.is_none() {
                    *slot = Some(entry.payload);
                }
            }
        }
    }

    /// Write every leading arrived payload in sequence order, closing each
    /// archive file (flush, sync, size) once its last entry is written.
    fn write_ready(&mut self, staging: &Path) -> Result<(), UnpackError> {
        loop {
            if self.done() {
                return Ok(());
            }
            if self.out.is_none() {
                let path = staging.join(ARCHIVES[self.archive].3);
                self.out = Some(
                    File::create(&path)
                        .map_err(|error| UnpackError::io("create archive file", error))?,
                );
            }
            if self.next_write == self.ends[self.archive] {
                self.finish_archive(staging)?;
                continue;
            }
            let Some(Some(payload)) = self.held.front_mut().map(Option::take) else {
                // The oldest entry has not arrived (the slot is untouched).
                return Ok(());
            };
            self.held.pop_front();
            let (_, file) = self.order[self.next_write];
            let out = self.out.as_mut().expect("archive file opened above");
            encode_record(out, file as u32, &payload)
                .map_err(|error| UnpackError::io("write record", error))?;
            self.stats[self.archive].unpacked += 1;
            self.next_write += 1;
        }
    }

    fn finish_archive(&mut self, staging: &Path) -> Result<(), UnpackError> {
        let mut out = self.out.take().expect("archive file is open");
        out.flush()
            .map_err(|error| UnpackError::io("flush archive file", error))?;
        out.sync_all()
            .map_err(|error| UnpackError::io("sync archive file", error))?;
        drop(out);
        let path = staging.join(ARCHIVES[self.archive].3);
        self.stats[self.archive].bytes =
            file_size(&path).map_err(|error| UnpackError::io("stat archive file", error))?;
        self.archive += 1;
        Ok(())
    }
}

#[cfg(test)]
#[path = "fill_tests.rs"]
mod tests;
