//! Client-Java 377 `Wave.field1471/field1472`: 5000 sound and delay slots.
//! Wave unpacking and silent-lead-in trimming share the legacy wire format.
//! Separate process tables keep a cache path loaded for 274/289 from aliasing
//! the revision-377 table (including when low-memory mode is later disabled).
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use super::jagfx::{JagFX, SoundPair, SoundTables};

const SYNTH_CAPACITY_377: usize = 5000;

impl JagFX {
    /// Empty Java-377-sized tables; generation scratch remains lazy.
    pub fn new_377() -> Self {
        static EMPTY: LazyLock<SoundPair> = LazyLock::new(|| {
            (
                Arc::new((0..SYNTH_CAPACITY_377).map(|_| None).collect()),
                Arc::new(vec![0; SYNTH_CAPACITY_377]),
            )
        });
        Self::from_tables(EMPTY.clone())
    }

    /// Load `sounds.dat` using Java 377's table sizes and loader.
    pub fn load_shared_377(cache_dir: &str, lowmem: bool) -> Self {
        static TABLES: LazyLock<Mutex<SoundTables>> = LazyLock::new(|| Mutex::new(HashMap::new()));
        Self::load_shared_with(cache_dir, lowmem, Self::new_377(), &TABLES)
    }
}
