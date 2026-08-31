use client::datastruct::{Arena, HashTable, LinkList, LinkList2, Linkable, LruCache};

#[test]
fn link_list_push_pop_front() {
    let mut list = LinkList::new();
    list.push(Linkable::new(1));
    list.push(Linkable::new(2));
    assert_eq!(list.pop_front().unwrap().key, 1);
    assert_eq!(list.pop_front().unwrap().key, 2);
    assert!(list.pop_front().is_none());
}

#[test]
fn link_list_push_front_reverses_order() {
    let mut list = LinkList::new();
    list.push(Linkable::new(1));
    list.push(Linkable::new(2));
    list.push_front(Linkable::new(3));
    assert_eq!(list.pop_front().unwrap().key, 3);
    assert_eq!(list.pop_front().unwrap().key, 1);
    assert_eq!(list.pop_front().unwrap().key, 2);
    assert!(list.pop_front().is_none());
}

#[test]
fn link_list_clear_drops_all_and_is_reusable() {
    let mut list = LinkList::new();
    list.push(Linkable::new(1));
    list.push(Linkable::new(2));
    list.clear();
    assert!(list.pop_front().is_none());
    list.push(Linkable::new(3));
    assert_eq!(list.pop_front().unwrap().key, 3);
}

#[test]
fn link_list_for_each_walks_in_order() {
    let mut list = LinkList::new();
    list.push(Linkable::new(10));
    list.push(Linkable::new(20));
    list.push(Linkable::new(30));
    let mut keys = Vec::new();
    list.for_each(|n| keys.push(n.key));
    assert_eq!(keys, [10, 20, 30]);
}

#[test]
fn link_list_for_each_empty_is_noop() {
    let list: LinkList<Linkable> = LinkList::new();
    let mut count = 0;
    list.for_each(|_| count += 1);
    assert_eq!(count, 0);
}

#[test]
fn link_list_for_each_leaves_mutable_cursor_untouched() {
    let mut list = LinkList::new();
    list.push(Linkable::new(10));
    list.push(Linkable::new(20));
    let mut seen = Vec::new();
    list.for_each(|n| seen.push(n.key));
    assert_eq!(seen, [10, 20]);
    // the immutable walk must not disturb head/next cursor state
    let mut keys = Vec::new();
    keys.push(list.head().unwrap().key);
    while let Some(n) = list.next_node() {
        keys.push(n.key);
    }
    assert_eq!(keys, [10, 20]);
}

#[test]
fn link_list_head_next_iterates() {
    let mut list = LinkList::new();
    list.push(Linkable::new(10));
    list.push(Linkable::new(20));
    list.push(Linkable::new(30));
    let mut keys = Vec::new();
    let first = list.head().unwrap();
    keys.push(first.key);
    while let Some(n) = list.next_node() {
        keys.push(n.key);
    }
    assert_eq!(keys, [10, 20, 30]);
}

#[test]
fn link_list_head_empty_and_cursor_exhaustion() {
    let mut list = LinkList::new();
    assert!(list.head().is_none());
    list.push(Linkable::new(7));
    assert_eq!(list.head().unwrap().key, 7);
    assert!(list.next_node().is_none()); // cursor hit sentinel
    assert!(list.next_node().is_none());
}

#[test]
fn link_list_tail_and_prev() {
    let mut list = LinkList::new();
    list.push(Linkable::new(10));
    list.push(Linkable::new(20));
    assert_eq!(list.tail().unwrap().key, 20);
    assert_eq!(list.prev().unwrap().key, 10);
    assert!(list.prev().is_none());
}

#[test]
fn arena_unlink_splices_and_is_idempotent() {
    let mut arena = Arena::new();
    let mut list = LinkList2::new(&mut arena);
    let a = arena.alloc(Linkable::new(1));
    let b = arena.alloc(Linkable::new(2));
    let c = arena.alloc(Linkable::new(3));
    list.push(&mut arena, a);
    list.push(&mut arena, b);
    list.push(&mut arena, c);
    assert_eq!(list.size(&arena), 3);
    arena.unlink2(b);
    assert_eq!(list.size(&arena), 2);
    arena.unlink2(b); // unlinked node: no-op
    assert_eq!(list.size(&arena), 2);
    let id = list.pop_front(&mut arena).unwrap();
    assert_eq!(arena.get(id).key, 1);
    let id = list.pop_front(&mut arena).unwrap();
    assert_eq!(arena.get(id).key, 3);
    assert!(list.pop_front(&mut arena).is_none());
}

#[test]
fn arena_unlink_on_unlinked_node_is_noop() {
    let mut arena = Arena::new();
    let a = arena.alloc(Linkable::new(5));
    arena.unlink(a);
    arena.unlink2(a);
    assert_eq!(arena.get(a).key, 5);
}

#[test]
fn hash_table_put_find_and_repin() {
    let mut arena = Arena::new();
    let mut table = HashTable::new(&mut arena, 1024);
    let a = arena.alloc(Linkable::new(0));
    let b = arena.alloc(Linkable::new(0));
    table.put(&mut arena, a, 42);
    table.put(&mut arena, b, 17);
    assert_eq!(table.find(&arena, 42), Some(a));
    assert_eq!(table.find(&arena, 17), Some(b));
    assert_eq!(table.find(&arena, 99), None);
    // re-put the same node: unlinks it from its bucket and updates its key
    table.put(&mut arena, a, 99);
    assert_eq!(table.find(&arena, 99), Some(a));
    assert_eq!(table.find(&arena, 42), None);
    assert_eq!(arena.get(a).key, 99);
}

#[test]
fn lru_cache_evicts_least_recently_used() {
    let mut cache = LruCache::new(3);
    cache.put(Linkable::new(0), 1);
    cache.put(Linkable::new(0), 2);
    cache.put(Linkable::new(0), 3);
    assert_eq!(cache.find(2).unwrap().key, 2); // 2 becomes MRU
    cache.put(Linkable::new(0), 4); // evicts 1
    assert!(cache.find(1).is_none());
    assert_eq!(cache.find(3).unwrap().key, 3);
    assert_eq!(cache.find(4).unwrap().key, 4);
    assert_eq!(cache.find(2).unwrap().key, 2);
    cache.put(Linkable::new(0), 5); // evicts 3
    assert!(cache.find(3).is_none());
    assert_eq!(cache.find(5).unwrap().key, 5);
}

#[test]
fn lru_cache_clear_resets_capacity() {
    let mut cache = LruCache::new(2);
    cache.put(Linkable::new(0), 1);
    cache.put(Linkable::new(0), 2);
    cache.clear();
    assert!(cache.find(1).is_none());
    assert!(cache.find(2).is_none());
    cache.put(Linkable::new(0), 3);
    cache.put(Linkable::new(0), 4);
    cache.put(Linkable::new(0), 5); // would evict early if capacity not reset
    assert_eq!(cache.find(5).unwrap().key, 5);
    assert_eq!(cache.find(4).unwrap().key, 4);
    assert!(cache.find(3).is_none());
}

#[test]
fn link_list_unlink_last_during_iterate() {
    let mut list = LinkList::new();
    list.push(Linkable::new(10));
    list.push(Linkable::new(20));
    list.push(Linkable::new(30));
    let first = list.head().unwrap();
    assert_eq!(first.key, 10);
    let second = list.next_node().unwrap();
    assert_eq!(second.key, 20);
    list.unlink_last();
    let mut keys = Vec::new();
    keys.push(list.head().unwrap().key);
    while let Some(n) = list.next_node() {
        keys.push(n.key);
    }
    assert_eq!(keys, [10, 30]);
}

#[test]
fn link_list_move_last_to_front() {
    let mut list = LinkList::new();
    list.push(Linkable::new(10));
    list.push(Linkable::new(20));
    list.push(Linkable::new(30));
    let _ = list.head();
    let _ = list.next_node(); // last = 20
    list.move_last_to_front();
    let mut keys = Vec::new();
    keys.push(list.head().unwrap().key);
    while let Some(n) = list.next_node() {
        keys.push(n.key);
    }
    assert_eq!(keys, [20, 10, 30]);
}
