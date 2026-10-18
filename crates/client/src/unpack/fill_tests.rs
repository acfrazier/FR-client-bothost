use super::*;
use std::time::{Duration, Instant};

fn tables(versions: Vec<i32>) -> ArchiveTables {
    let crcs = vec![0; versions.len()];
    ArchiveTables {
        version_table: "model_version",
        crc_table: "model_crc",
        versions,
        crcs,
    }
}

fn payload(archive: usize, file: i32) -> Vec<u8> {
    format!("archive {archive} entry {file}").into_bytes()
}

/// Serves every outstanding request newest first, `per_take` at a time, and
/// records the most requests it held unanswered.
struct ReversingSource {
    outstanding: Vec<(i32, i32)>,
    per_take: usize,
    max_outstanding: usize,
}

impl EntrySource for ReversingSource {
    fn request_entry(&mut self, archive: i32, file: i32) -> Result<(), String> {
        self.outstanding.push((archive, file));
        self.max_outstanding = self.max_outstanding.max(self.outstanding.len());
        Ok(())
    }

    fn take_entries(&mut self, _until: Instant, out: &mut Vec<FetchedEntry>) -> Result<(), String> {
        for _ in 0..self.per_take {
            let Some((archive, file)) = self.outstanding.pop() else {
                break;
            };
            out.push(FetchedEntry {
                archive,
                file,
                payload: payload(archive as usize, file),
            });
        }
        Ok(())
    }
}

fn staging(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cold-fill-window-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn expected_records(archive: usize, versions: &[i32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (file, version) in versions.iter().enumerate() {
        if *version != 0 {
            encode_record(&mut bytes, file as u32, &payload(archive, file as i32)).unwrap();
        }
    }
    bytes
}

/// The fill keeps `FILL_WINDOW` entries requested ahead of the oldest
/// unwritten one, across archive boundaries, and never more. A 16-entry
/// batch barrier could never hold more than 16. Records land in index order
/// although every entry arrives out of order.
#[test]
fn window_requests_ahead_without_a_batch_barrier_and_writes_in_order() {
    let models: Vec<i32> = (0..400).map(|file| i32::from(file % 7 != 3)).collect();
    let anims: Vec<i32> = vec![1; 90];
    let midi: Vec<i32> = vec![0; 5];
    let maps: Vec<i32> = (0..120).map(|file| i32::from(file % 2 == 0)).collect();
    let all = [
        tables(models.clone()),
        tables(anims.clone()),
        tables(midi.clone()),
        tables(maps.clone()),
    ];
    let dir = staging("order");
    let mut window = EntryWindow::new(&all);
    let mut source = ReversingSource {
        outstanding: Vec::new(),
        per_take: 7,
        max_outstanding: 0,
    };
    let mut steps = 0;
    while !window
        .step(&dir, &mut source, Some(Instant::now()))
        .unwrap()
    {
        steps += 1;
        assert!(steps < 10_000, "the fill must finish");
    }
    assert_eq!(source.max_outstanding, FILL_WINDOW);
    assert!(source.outstanding.is_empty());
    for (slot, versions) in [&models, &anims, &midi, &maps].into_iter().enumerate() {
        let written = std::fs::read(dir.join(ARCHIVES[slot].3)).unwrap();
        assert_eq!(
            written,
            expected_records(slot, versions),
            "{}",
            ARCHIVES[slot].3
        );
        let stats = &window.stats[slot];
        assert_eq!(stats.total, versions.len() as u32);
        assert_eq!(
            stats.unpacked + stats.skipped,
            stats.total,
            "{}",
            ARCHIVES[slot].3
        );
        assert_eq!(stats.bytes, written.len() as u64);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A step returns at its deadline while no entry arrives, and the idle limit
/// is measured across steps from the last arrival.
#[test]
fn quiet_steps_return_at_their_deadline() {
    struct Silent;
    impl EntrySource for Silent {
        fn request_entry(&mut self, _archive: i32, _file: i32) -> Result<(), String> {
            Ok(())
        }
        fn take_entries(
            &mut self,
            until: Instant,
            _out: &mut Vec<FetchedEntry>,
        ) -> Result<(), String> {
            std::thread::sleep(until.saturating_duration_since(Instant::now()));
            Ok(())
        }
    }
    let all = [
        tables(vec![1; 3]),
        tables(Vec::new()),
        tables(Vec::new()),
        tables(Vec::new()),
    ];
    let dir = staging("quiet");
    let mut window = EntryWindow::new(&all);
    for _ in 0..5 {
        let started = Instant::now();
        let done = window
            .step(&dir, &mut Silent, Some(started + Duration::from_millis(5)))
            .unwrap();
        assert!(!done);
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "{:?}",
            started.elapsed()
        );
    }
    window.last_progress = Some(Instant::now() - FETCH_IDLE_TIMEOUT);
    let error = window
        .step(
            &dir,
            &mut Silent,
            Some(Instant::now() + Duration::from_millis(5)),
        )
        .unwrap_err();
    assert!(error.to_string().contains("no entry arrived"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}
