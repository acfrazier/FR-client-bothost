// Port of `~/experiments/Server/webclient/src/datastruct/LruCache.ts`. The
// table and history share one arena so a cached node keeps its identity while
// being linked into both chains (TS `Linkable2` in a `HashTable` and a
// `LinkList2` at once).

use super::hash_table::HashTable;
use super::link_list::LinkList2;
use super::linkable::{Arena, LinkableTrait};

pub struct LruCache<T: LinkableTrait> {
    capacity: usize,
    available: usize,
    arena: Arena<T>,
    table: HashTable<T>,
    history: LinkList2<T>,
}

impl<T: LinkableTrait> LruCache<T> {
    pub fn new(size: usize) -> Self {
        let mut arena = Arena::new();
        let table: HashTable<T> = HashTable::new(&mut arena, 1024);
        let history: LinkList2<T> = LinkList2::new(&mut arena);
        LruCache {
            capacity: size,
            available: size,
            arena,
            table,
            history,
        }
    }

    /// Look up a key and move the node to the most-recently-used end.
    pub fn find(&mut self, key: i64) -> Option<&mut T> {
        let id = self.table.find(&self.arena, key);
        if let Some(id) = id {
            self.history.push(&mut self.arena, id);
        }
        id.map(|id| self.arena.get_mut(id))
    }

    pub fn put(&mut self, node: T, key: i64) {
        if self.available == 0 {
            if let Some(id) = self.history.pop_front(&mut self.arena) {
                self.arena.unlink(id);
                self.arena.take(id);
            }
        } else {
            self.available -= 1;
        }
        let id = self.arena.alloc(node);
        self.table.put(&mut self.arena, id, key);
        self.history.push(&mut self.arena, id);
    }

    /// Detach the node for `key` from the hash-table chain only (Java
    /// `Linkable.unlink()` on a table node). The node stays in the LRU
    /// history until evicted, exactly like Java; `ObjType.getSprite` uses
    /// this to drop an ohi-mismatched cache hit before re-putting its key,
    /// so the stale node does not shadow the replacement in `find`.
    pub fn unlink_key(&mut self, key: i64) {
        if let Some(id) = self.table.find(&self.arena, key) {
            self.arena.unlink(id);
        }
    }

    /// Remove a key entirely: unlink it from both the hash-table chain and
    /// the LRU history and free the node (unlike `unlink_key`, the node is
    /// not recoverable). The shared geometry store uses this on
    /// `Model::unload`, so a dropped model is re-requested instead of
    /// served from a stale decoded copy.
    pub fn remove(&mut self, key: i64) {
        if let Some(id) = self.table.find(&self.arena, key) {
            self.arena.unlink(id);
            self.arena.unlink2(id);
            self.arena.take(id);
            self.available += 1;
        }
    }

    pub fn clear(&mut self) {
        while let Some(id) = self.history.pop_front(&mut self.arena) {
            self.arena.unlink(id);
            self.arena.take(id);
        }
        self.available = self.capacity;
    }
}
