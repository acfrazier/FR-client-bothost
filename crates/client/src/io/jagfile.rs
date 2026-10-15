// Port of `~/experiments/Server/webclient/src/io/JagFile.ts`. `read` keeps the
// TS signature (`&self`), so the per-file decompression cache lives behind a
// `RefCell` (mutation is only ever write-once per slot, as in the TS arrays).
use std::cell::RefCell;

use super::bzip2::bunzip2_sized;
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
            hash = hash
                .wrapping_mul(61)
                .wrapping_add(c as i32)
                .wrapping_sub(32);
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
            let data = bunzip2_sized(&src[6..], unpacked_size as usize);
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
            bunzip2_sized(src, self.file_unpacked_size[index] as usize)
        };
        self.file_unpacked.borrow_mut()[index] = Some(data.clone());
        Some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::super::bzip2::{bunzip2_sized, try_bunzip2};
    use super::JagFile;
    use bzip2::{write::BzEncoder, Compression};
    use std::io::Write;

    fn append_g3(dst: &mut Vec<u8>, value: usize) {
        dst.extend_from_slice(&[(value >> 16) as u8, (value >> 8) as u8, value as u8]);
    }

    fn headerless(data: &[u8]) -> Vec<u8> {
        let mut encoder = BzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()[4..].to_vec()
    }

    fn container(file_data: &[u8], unpacked_size: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(12 + file_data.len());
        data.extend_from_slice(&1_u16.to_be_bytes());
        data.extend_from_slice(&JagFile::gen_hash("test.dat").to_be_bytes());
        append_g3(&mut data, unpacked_size);
        append_g3(&mut data, file_data.len());
        data.extend_from_slice(file_data);
        data
    }

    fn archive(data: &[u8], compressed: bool) -> Vec<u8> {
        let packed = if compressed {
            headerless(data)
        } else {
            data.to_vec()
        };
        let mut archive = Vec::with_capacity(6 + packed.len());
        append_g3(&mut archive, data.len());
        append_g3(&mut archive, packed.len());
        archive.extend_from_slice(&packed);
        archive
    }

    #[test]
    fn normal_compressed_archive_and_member_preserve_bytes() {
        // Java JagFile.java:42-44 and :72-80 allocate the declared container
        // and member sizes; BZip2.java:41-65 writes only within those bounds.
        let file_data = vec![0x5a; 8_192];
        let member_stream = headerless(&file_data);
        assert_eq!(
            bunzip2_sized(&member_stream, file_data.len()),
            try_bunzip2(&member_stream).unwrap()
        );
        let member_archive = archive(&container(&member_stream, file_data.len()), false);
        assert_eq!(
            JagFile::new(member_archive).read("test.dat"),
            Some(file_data.clone())
        );

        let container_data = container(&file_data, file_data.len());
        let compressed_archive = archive(&container_data, true);
        assert_eq!(
            JagFile::new(compressed_archive).read("test.dat"),
            Some(file_data)
        );
    }

    #[test]
    fn oversized_member_stream_is_truncated_to_declared_size() {
        // Java JagFile.java:73-80 allocates the declared member size, and
        // BZip2.java:49-65 stops writing when its remaining output reaches 0.
        let file_data: Vec<u8> = (0..32_768).map(|i| (i * 37) as u8).collect();
        let declared_size = 12_345;
        let compressed = headerless(&file_data);
        let archive = archive(&container(&compressed, declared_size), false);

        assert_eq!(
            JagFile::new(archive).read("test.dat"),
            Some(file_data[..declared_size].to_vec())
        );
    }

    #[test]
    fn short_member_stream_leaves_java_zero_filled_suffix() {
        // Java's `new byte[fileUnpackedSize]` in JagFile.java:73 zero-fills
        // the destination; BZip2.java:49-65 only overwrites produced bytes.
        let file_data: Vec<u8> = (0..4_096).map(|i| (i * 19) as u8).collect();
        let declared_size = file_data.len() + 37;
        let compressed = headerless(&file_data);
        let archive = archive(&container(&compressed, declared_size), false);
        let mut expected = file_data;
        expected.resize(declared_size, 0);

        assert_eq!(JagFile::new(archive).read("test.dat"), Some(expected));
    }
}
