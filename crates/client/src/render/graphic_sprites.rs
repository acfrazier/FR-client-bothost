use std::fs::File;
use std::io::Read;
use std::mem::size_of;
use std::path::Path;

use crate::graphics::Pix32;
use crate::io::JagFile;

use super::budget::{
    reserve_vec, vec_capacity_bytes, BudgetCharge, ByteBudget, RenderBudget, RenderLimit,
    RenderResource,
};

struct Entry {
    name: String,
    index: i32,
    sprite: Option<Pix32>,
    _key_charge: Option<BudgetCharge>,
    _pixel_charge: Option<BudgetCharge>,
}

pub(crate) struct GraphicSpriteCache {
    entries: Vec<Entry>,
    table_charge: Option<BudgetCharge>,
}

impl GraphicSpriteCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            table_charge: None,
        }
    }

    pub(crate) fn get(
        &mut self,
        cache_dir: &str,
        name: &str,
        index: i32,
    ) -> Result<Option<&Pix32>, RenderLimit> {
        let retained_budget = RenderBudget::current_resource(RenderResource::HeadMisc);
        if let Some(found) = self
            .entries
            .iter()
            .position(|entry| entry.name == name && entry.index == index)
        {
            if let Some(budget) = retained_budget.as_ref() {
                self.sync_existing(budget)?;
            }
            return Ok(self.entries[found].sprite.as_ref());
        }

        let (sprite, pixel_charge) = if let Some(budget) = retained_budget.as_ref() {
            self.sync_existing(budget)?;
            let scratch_budget = RenderBudget::current_resource(RenderResource::TempModels)
                .ok_or_else(|| RenderLimit::new(RenderResource::TempModels, usize::MAX, 0))?;
            match Self::load_scoped(cache_dir, name, index, &scratch_budget, budget)? {
                Some((sprite, charge)) => (Some(sprite), Some(charge)),
                None => (None, None),
            }
        } else {
            (Self::load_legacy(cache_dir, name, index), None)
        };

        let (name, key_charge) = match retained_budget.as_ref() {
            Some(budget) => Self::copy_key(name, budget)?,
            None => (name.to_owned(), None),
        };
        self.reserve_slot(retained_budget.as_ref())?;
        self.entries.push(Entry {
            name,
            index,
            sprite,
            _key_charge: key_charge,
            _pixel_charge: pixel_charge,
        });
        Ok(self.entries.last().and_then(|entry| entry.sprite.as_ref()))
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        let mut bytes = self.entries.capacity().saturating_mul(size_of::<Entry>());
        for entry in &self.entries {
            bytes = bytes.saturating_add(entry.name.capacity());
            if let Some(sprite) = &entry.sprite {
                bytes =
                    bytes.saturating_add(sprite.data.capacity().saturating_mul(size_of::<i32>()));
            }
        }
        bytes
    }

    fn allocation_limit(budget: &ByteBudget) -> RenderLimit {
        RenderLimit::new(budget.resource(), usize::MAX, budget.maximum())
    }

    fn checked_bytes(
        count: usize,
        element_size: usize,
        budget: &ByteBudget,
    ) -> Result<usize, RenderLimit> {
        count
            .checked_mul(element_size)
            .ok_or_else(|| Self::allocation_limit(budget))
    }

    fn sync_charge(
        charge: &mut Option<BudgetCharge>,
        bytes: usize,
        budget: &ByteBudget,
    ) -> Result<(), RenderLimit> {
        match charge.as_mut() {
            Some(existing) if existing.belongs_to(budget) => existing.resize(bytes),
            _ => {
                *charge = Some(budget.reserve(bytes)?);
                Ok(())
            }
        }
    }

    fn sync_existing(&mut self, budget: &ByteBudget) -> Result<(), RenderLimit> {
        let table_bytes = Self::checked_bytes(self.entries.capacity(), size_of::<Entry>(), budget)?;
        Self::sync_charge(&mut self.table_charge, table_bytes, budget)?;
        for entry in &mut self.entries {
            Self::sync_charge(&mut entry._key_charge, entry.name.capacity(), budget)?;
            if let Some(sprite) = &entry.sprite {
                let bytes = Self::checked_bytes(sprite.data.capacity(), size_of::<i32>(), budget)?;
                Self::sync_charge(&mut entry._pixel_charge, bytes, budget)?;
            }
        }
        Ok(())
    }

    fn reserve_slot(&mut self, budget: Option<&ByteBudget>) -> Result<(), RenderLimit> {
        if let Some(budget) = budget {
            let bytes = vec_capacity_bytes(&self.entries);
            Self::sync_charge(&mut self.table_charge, bytes, budget)?;
            reserve_vec(&mut self.entries, 1, &mut self.table_charge, Some(budget))
        } else {
            self.entries.reserve(1);
            Ok(())
        }
    }

    fn copy_key(
        name: &str,
        budget: &ByteBudget,
    ) -> Result<(String, Option<BudgetCharge>), RenderLimit> {
        let mut charge = budget.reserve(name.len())?;
        let mut key = String::new();
        key.try_reserve_exact(name.len())
            .map_err(|_| Self::allocation_limit(budget))?;
        key.push_str(name);
        charge.resize(key.capacity())?;
        Ok((key, Some(charge)))
    }

    fn load_legacy(cache_dir: &str, name: &str, index: i32) -> Option<Pix32> {
        let source = std::fs::read(Path::new(cache_dir).join("media")).ok()?;
        let jag = JagFile::new(source);
        Pix32::depack(&jag, name, index)
    }

    fn load_scoped(
        cache_dir: &str,
        name: &str,
        index: i32,
        scratch_budget: &ByteBudget,
        retained_budget: &ByteBudget,
    ) -> Result<Option<(Pix32, BudgetCharge)>, RenderLimit> {
        let separator = usize::from(!cache_dir.is_empty() && !cache_dir.ends_with('/'));
        let path_len = cache_dir
            .len()
            .checked_add(separator)
            .and_then(|length| length.checked_add("media".len()))
            .ok_or_else(|| Self::allocation_limit(scratch_budget))?;
        let mut path_charge = scratch_budget.reserve(path_len)?;
        let mut path = String::new();
        path.try_reserve_exact(path_len)
            .map_err(|_| Self::allocation_limit(scratch_budget))?;
        path.push_str(cache_dir);
        if separator != 0 {
            path.push('/');
        }
        path.push_str("media");
        path_charge.resize(path.capacity())?;

        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(_) => return Ok(None),
        };
        let file_len = match file.metadata() {
            Ok(metadata) => usize::try_from(metadata.len())
                .map_err(|_| Self::allocation_limit(scratch_budget))?,
            Err(_) => return Ok(None),
        };
        let mut source_charge = scratch_budget.reserve(file_len)?;
        let mut source = Vec::new();
        source
            .try_reserve_exact(file_len)
            .map_err(|_| Self::allocation_limit(scratch_budget))?;
        source.resize(file_len, 0);
        if file.read_exact(&mut source).is_err() {
            return Ok(None);
        }
        source_charge.resize(source.capacity())?;

        let jag = JagFile::new_render(source, scratch_budget)?;
        Pix32::depack_render(&jag, name, index, scratch_budget, retained_budget)
    }
}

#[cfg(feature = "render-test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTestGraphicSpriteLayout {
    pub unique_keys: usize,
    pub entry_bytes: usize,
    pub key_bytes: usize,
    pub sprite_pixel_bytes: usize,
    pub retained_bytes: usize,
    pub temp_models_peak_bytes: usize,
}

#[cfg(feature = "render-test-hooks")]
pub fn render_test_scoped_content_layout(
    cache_dir: &str,
    keys: &[(String, i32)],
) -> Option<RenderTestGraphicSpriteLayout> {
    let unlimited = usize::MAX;
    let budget = RenderBudget::fixture(crate::render::budget::RenderBudgetLimits::new(
        unlimited, unlimited, unlimited, unlimited, unlimited, unlimited, unlimited, unlimited,
        unlimited, unlimited, unlimited, unlimited, unlimited,
    ));
    let _scope = budget.scope();
    let mut cache = GraphicSpriteCache::new();
    for (name, index) in keys {
        cache.get(cache_dir, name, *index).ok()?;
    }
    let entry_bytes = cache.entries.capacity().checked_mul(size_of::<Entry>())?;
    let key_bytes = cache.entries.iter().try_fold(0usize, |total, entry| {
        total.checked_add(entry.name.capacity())
    })?;
    let sprite_pixel_bytes = cache.entries.iter().try_fold(0usize, |total, entry| {
        total.checked_add(
            entry
                .sprite
                .as_ref()
                .map_or(0, |sprite| vec_capacity_bytes(&sprite.data)),
        )
    })?;
    Some(RenderTestGraphicSpriteLayout {
        unique_keys: cache.entries.len(),
        entry_bytes,
        key_bytes,
        sprite_pixel_bytes,
        retained_bytes: cache.retained_bytes(),
        temp_models_peak_bytes: budget.for_resource(RenderResource::TempModels).peak(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bzip2::write::BzEncoder;
    use bzip2::Compression;

    use super::*;

    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

    struct MediaFixture(PathBuf);

    impl MediaFixture {
        fn new(sprite_width: u16, sprite_height: u16) -> Self {
            let path = std::env::temp_dir().join(format!(
                "graphic-sprite-cache-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            std::fs::write(
                path.join("media"),
                media_archive(sprite_width, sprite_height),
            )
            .unwrap();
            Self(path)
        }

        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }

    impl Drop for MediaFixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn append_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn append_u24(bytes: &mut Vec<u8>, value: usize) {
        bytes.extend_from_slice(&[(value >> 16) as u8, (value >> 8) as u8, value as u8]);
    }

    fn media_archive(sprite_width: u16, sprite_height: u16) -> Vec<u8> {
        let mut dat = vec![0, 0, 1];
        dat.resize(3 + sprite_width as usize * sprite_height as usize, 1);

        let mut index = Vec::new();
        append_u16(&mut index, sprite_width);
        append_u16(&mut index, sprite_height);
        index.push(2);
        index.extend_from_slice(&[0x12, 0x34, 0x56]);
        index.extend_from_slice(&[0, 0]);
        append_u16(&mut index, 1);
        append_u16(&mut index, 1);
        index.push(0);
        index.extend_from_slice(&[0, 0]);
        append_u16(&mut index, sprite_width);
        append_u16(&mut index, sprite_height);
        index.push(0);

        let members: [(&str, &[u8]); 2] = [("sprite.dat", &dat), ("index.dat", &index)];
        let mut contents = Vec::new();
        append_u16(&mut contents, members.len() as u16);
        for (name, bytes) in members {
            contents.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
            append_u24(&mut contents, bytes.len());
            append_u24(&mut contents, bytes.len());
        }
        for (_, bytes) in members {
            contents.extend_from_slice(bytes);
        }

        let mut encoder = BzEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&contents).unwrap();
        let compressed = encoder.finish().unwrap();
        let packed = &compressed[4..];

        let mut archive = Vec::new();
        append_u24(&mut archive, contents.len());
        append_u24(&mut archive, packed.len());
        archive.extend_from_slice(packed);
        archive
    }

    fn budget(head_misc: usize, temp_models: usize) -> RenderBudget {
        RenderBudget::fixture(super::super::budget::RenderBudgetLimits::new(
            0,
            0,
            0,
            0,
            0,
            0,
            head_misc,
            temp_models,
            0,
            0,
            0,
            0,
            0,
        ))
    }

    #[test]
    fn retained_census_counts_entry_keys_pixels_and_cached_misses() {
        let fixture = MediaFixture::new(32, 32);
        let mut cache = GraphicSpriteCache::new();
        let budget = budget(1_000_000, 1_000_000);
        let _scope = budget.scope();

        assert_eq!(
            cache
                .get(fixture.path(), "sprite", 0)
                .unwrap()
                .unwrap()
                .data[0],
            0x12_3456
        );
        assert!(cache.get(fixture.path(), "missing", 0).unwrap().is_none());
        let retained = cache.retained_bytes();
        assert!(cache.get(fixture.path(), "missing", 0).unwrap().is_none());
        assert_eq!(cache.retained_bytes(), retained);
        assert_eq!(
            cache.retained_bytes(),
            budget.for_resource(RenderResource::HeadMisc).live()
        );
        assert_eq!(budget.for_resource(RenderResource::TempModels).live(), 0);
    }

    #[test]
    fn refused_growth_preserves_cached_entries_and_charges() {
        let fixture = MediaFixture::new(256, 256);
        let mut cache = GraphicSpriteCache::new();
        let head_misc_limit = size_of::<Entry>() * 8 + 512;
        let budget = budget(head_misc_limit, 1_000_000);
        let _scope = budget.scope();

        assert!(cache.get(fixture.path(), "sprite", 0).unwrap().is_some());
        let entries_before = cache.entries.len();
        let retained_before = cache.retained_bytes();
        let live_before = budget.for_resource(RenderResource::HeadMisc).live();
        let error = match cache.get(fixture.path(), "sprite", 1) {
            Ok(_) => panic!("large sprite unexpectedly fit the retained budget"),
            Err(error) => error,
        };

        assert_eq!(error.resource(), RenderResource::HeadMisc);
        assert_eq!(cache.entries.len(), entries_before);
        assert_eq!(cache.retained_bytes(), retained_before);
        assert_eq!(
            budget.for_resource(RenderResource::HeadMisc).live(),
            live_before
        );
        assert_eq!(budget.for_resource(RenderResource::TempModels).live(), 0);
    }
}
