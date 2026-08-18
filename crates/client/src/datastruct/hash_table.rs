// Port of `~/experiments/Server/webclient/src/datastruct/HashTable.ts`. The
// bucket index is `key & (bucketCount - 1)` (TS uses the same mask on a
// `bigint`), so the table size must be a power of two.

use std::marker::PhantomData;

use super::linkable::{Arena, LinkableTrait};

pub struct HashTable<T: LinkableTrait> {
    bucket_count: usize,
    buckets: Vec<usize>,
    marker: PhantomData<fn() -> T>,
}

impl<T: LinkableTrait> HashTable<T> {
    pub fn new(arena: &mut Arena<T>, size: usize) -> Self {
        let mut buckets = Vec::with_capacity(size);
        for _ in 0..size {
            let s = arena.alloc(T::sentinel());
            arena.get_mut(s).links_mut().next = Some(s);
            arena.get_mut(s).links_mut().prev = Some(s);
            buckets.push(s);
        }
        HashTable {
            bucket_count: size,
            buckets,
            marker: PhantomData,
        }
    }

    pub fn find(&self, arena: &Arena<T>, key: i64) -> Option<usize> {
        let start = self.buckets[((key as u64) & (self.bucket_count as u64 - 1)) as usize];
        let mut node = arena.get(start).links().next.expect("linked list invariant");
        while node != start {
            if arena.get(node).links().key == key {
                return Some(node);
            }
            node = arena.get(node).links().next.expect("linked list invariant");
        }
        None
    }

    /// Link an existing arena node into the table and set its key (TS
    /// `HashTable.put`). Unlinks the node from any chain first, so re-putting a
    /// node moves it to the new bucket.
    pub fn put(&mut self, arena: &mut Arena<T>, id: usize, key: i64) {
        if arena.get(id).links().prev.is_some() {
            arena.unlink(id);
        }
        let s = self.buckets[((key as u64) & (self.bucket_count as u64 - 1)) as usize];
        let tail = arena.get(s).links().prev.expect("linked list invariant");
        {
            let n = arena.get_mut(id).links_mut();
            n.key = key;
            n.prev = Some(tail);
            n.next = Some(s);
        }
        arena.get_mut(tail).links_mut().next = Some(id);
        arena.get_mut(s).links_mut().prev = Some(id);
    }
}
