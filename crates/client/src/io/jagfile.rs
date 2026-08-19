// Port of `~/experiments/Server/webclient/src/io/JagFile.ts`. `read` keeps the
// TS signature (`&self`), so the per-file decompression cache lives behind a
// `RefCell` (mutation is only ever write-once per slot, as in the TS arrays).
use std::cell::RefCell;

use super::bzip2::bunzip2;
use super::packet::Packet;

pub struct JagFile {
    data: Vec<u8>,
    unpacked: bool,
    pub file_count: i32,
    file_hash: Vec<i32>,
    // public in TS; read by later tasks (ondemand/config) for size checks
    #[allow(dead_code)]
    file_unpacked_size: Vec<i32>,
    file_packed_size: Vec<i32>,
    file_offset: Vec<i32>,
    file_unpacked: RefCell<Vec<Option<Vec<u8>>>>,
}

impl JagFile {
    pub fn gen_hash(name: &str) -> i32 {
        let mut hash: i32 = 0;
        for c in name.chars() {
            let c = c.to_ascii_uppercase();
            hash = hash.wrapping_mul(61).wrapping_add(c as i32).wrapping_sub(32);
        }
        hash
    }

    pub fn new(src: Vec<u8>) -> Self {
        let mut packet = Packet::new(src.clone());
        let unpacked_size = packet.g3();
        let packed_size = packet.g3();

        let unpacked = unpacked_size != packed_size;
        let data = if unpacked {
            // decompressed container is re-parsed from offset 0
            let data = bunzip2(&src[6..]);
            packet = Packet::new(data.clone());
            data
        } else {
            src
        };

        // non-unpacked: packet still sits at pos 6 (after the two g3 reads)
        let file_count = packet.g2();
        let mut file_hash = Vec::with_capacity(file_count as usize);
        let mut file_unpacked_size = Vec::with_capacity(file_count as usize);
        let mut file_packed_size = Vec::with_capacity(file_count as usize);
        let mut file_offset = Vec::with_capacity(file_count as usize);

        let mut offset = packet.pos as i32 + file_count * 10;
        for _ in 0..file_count {
            file_hash.push(packet.g4());
            file_unpacked_size.push(packet.g3());
            file_packed_size.push(packet.g3());
            file_offset.push(offset);
            offset += *file_packed_size.last().unwrap();
        }

        JagFile {
            data,
            unpacked,
            file_count,
            file_hash,
            file_unpacked_size,
            file_packed_size,
            file_offset,
            file_unpacked: RefCell::new(vec![None; file_count as usize]),
        }
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let hash = Self::gen_hash(name);
        let index = self.file_hash.iter().position(|&h| h == hash)?;
        self.read_index(index as i32)
    }

    pub fn read_index(&self, index: i32) -> Option<Vec<u8>> {
        if index < 0 || index >= self.file_count {
            return None;
        }
        let index = index as usize;
        {
            let cache = self.file_unpacked.borrow();
            if let Some(data) = &cache[index] {
                return Some(data.clone());
            }
        }
        let offset = self.file_offset[index] as usize;
        let length = self.file_packed_size[index] as usize;
        let src = &self.data[offset..offset + length];
        let data = if self.unpacked {
            src.to_vec()
        } else {
            bunzip2(src)
        };
        self.file_unpacked.borrow_mut()[index] = Some(data.clone());
        Some(data)
    }
}
