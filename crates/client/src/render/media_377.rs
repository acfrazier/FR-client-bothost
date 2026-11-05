//! Sprite tables loaded only by the revision-377 client.
//!
//! Java anchors: `Client.java:1914-1951` loads mapscene/mapfunction, PK and
//! prayer icons, multiway, and mapdots; `Client.java:2023-2035` applies the
//! shared minimap-icon color jitter.

use crate::core::build::random_float;
use crate::graphics::{Pix32, Pix8};
use crate::io::JagFile;

/// The 377 media sprite tables whose capacities and indices differ from the
/// legacy renderer. All sprites are immutable after the shared media load.
pub struct Media377 {
    /// Java `imageMapscene[100]`; 72 entries are loaded.
    pub mapscene: [Option<Pix8>; 100],
    /// Java `imageMapfunction[100]`; 70 entries are loaded.
    pub mapfunction: [Option<Pix32>; 100],
    /// Java `imageHeadiconsPk[32]`; six entries are loaded.
    pub headicons_pk: [Option<Pix32>; 32],
    /// Java `imageHeadiconsPrayer[32]`; nine entries are loaded.
    pub headicons_prayer: [Option<Pix32>; 32],
    /// Java `imageMapdot4`, the fifth `mapdots` sprite (index 4).
    pub mapdots5: Option<Pix32>,
    /// Java `imageOverlayMultiway`.
    pub overlay_multiway: Option<Pix32>,
}

impl Media377 {
    /// Empty 377 tables for renderers without a loaded media archive.
    pub fn empty() -> Self {
        Self {
            mapscene: [const { None }; 100],
            mapfunction: [const { None }; 100],
            headicons_pk: [const { None }; 32],
            headicons_prayer: [const { None }; 32],
            mapdots5: None,
            overlay_multiway: None,
        }
    }

    pub(crate) fn depack(jag: &JagFile) -> Self {
        let mut media = Self::empty();
        for i in 0..72 {
            media.mapscene[i] = Pix8::depack(jag, "mapscene", i as i32);
        }
        for i in 0..70 {
            media.mapfunction[i] = Pix32::depack(jag, "mapfunction", i as i32);
        }
        for i in 0..6 {
            media.headicons_pk[i] = Pix32::depack(jag, "headicons_pk", i as i32);
        }
        for i in 0..9 {
            media.headicons_prayer[i] = Pix32::depack(jag, "headicons_prayer", i as i32);
        }
        media.mapdots5 = Pix32::depack(jag, "mapdots", 4);
        media.overlay_multiway = Pix32::depack(jag, "overlay_multiway", 0);

        let rand_r = (random_float() * 21.0) as i32 - 10;
        let rand_g = (random_float() * 21.0) as i32 - 10;
        let rand_b = (random_float() * 21.0) as i32 - 10;
        let rand = (random_float() * 41.0) as i32 - 20;
        media.adjust_map_icons(rand_r, rand_g, rand_b, rand);
        media
    }

    fn adjust_map_icons(&mut self, rand_r: i32, rand_g: i32, rand_b: i32, rand: i32) {
        for sprite in self.mapfunction.iter_mut().flatten() {
            sprite.rgb_adjust(rand_r + rand, rand_g + rand, rand_b + rand);
        }
        for sprite in self.mapscene.iter_mut().flatten() {
            sprite.rgb_adjust(rand_r + rand, rand_g + rand, rand_b + rand);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Media377;
    use crate::graphics::{Pix32, Pix8};
    use crate::io::{synthetic_jag, JagFile};

    #[test]
    fn map_icon_jitter_matches_java_rgb_channels() {
        let mut media = Media377::empty();
        let mut function = Pix32::new(1, 1);
        function.data[0] = 0x123456;
        media.mapfunction[69] = Some(function);
        let mut scene = Pix8::new(1, 1, vec![0, 0x123456]);
        scene.data[0] = 1;
        media.mapscene[71] = Some(scene);

        // Java draws the same rand values into each sprite: R=+6, G=+3, B=+8.
        media.adjust_map_icons(1, -2, 3, 5);

        assert_eq!(media.mapfunction[69].as_ref().unwrap().data, [0x18375e]);
        assert_eq!(media.mapscene[71].as_ref().unwrap().bpal[1], 0x18375e);
    }

    #[test]
    fn depack_loads_java_377_high_map_icon_indices() {
        let mut index = vec![0, 1, 0, 1, 2, 0x12, 0x34, 0x56];
        let scene_index = index.len();
        append_sprite_metadata(&mut index, 72);
        let function_index = index.len();
        append_sprite_metadata(&mut index, 70);

        let mut scene_data = (scene_index as u16).to_be_bytes().to_vec();
        scene_data.extend([1; 72]);
        let mut function_data = (function_index as u16).to_be_bytes().to_vec();
        function_data.extend([1; 70]);
        let bytes = synthetic_jag(&[
            ("index.dat", index.as_slice()),
            ("mapscene.dat", scene_data.as_slice()),
            ("mapfunction.dat", function_data.as_slice()),
        ]);

        let media = Media377::depack(&JagFile::new(bytes));

        let scene = media.mapscene[71].as_ref().expect("Java loads 72 scenes");
        assert_eq!(scene.data, [1]);
        assert_ne!(scene.bpal[1], 0);
        let function = media.mapfunction[69]
            .as_ref()
            .expect("Java loads 70 map functions");
        assert_ne!(function.data[0], 0);
        assert!(media.mapscene[72].is_none());
        assert!(media.mapfunction[70].is_none());
    }

    #[test]
    fn hint_icons_keep_java_377_array_capacity() {
        let media = crate::render::media::Media::empty();
        assert_eq!(media.headicons_hint.len(), 32);
    }

    fn append_sprite_metadata(index: &mut Vec<u8>, count: usize) {
        for _ in 0..count {
            index.extend_from_slice(&[0, 0, 0, 1, 0, 1, 0]);
        }
    }
}
