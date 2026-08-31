// Port of `~/experiments/Server/webclient/src/datastruct/LinkList.ts` and
// `LinkList2.ts`. `LinkList` owns its nodes in a private arena and uses the
// `next`/`prev` chain; `LinkList2` borrows an arena and uses the `next2`/`prev2`
// chain, so its nodes can also be in a `HashTable` (as LruCache does).

use std::marker::PhantomData;

use super::linkable::{Arena, LinkableTrait};

/// Circular doubly-linked list with a sentinel, owning its nodes.
pub struct LinkList<T: LinkableTrait> {
    arena: Arena<T>,
    sentinel: usize,
    cursor: Option<usize>,
    /// Node most recently returned by `head`/`next`/`tail`/`prev`, so a
    /// walk can unlink or re-insert the current node (TS `unlink()`/`pushFront`
    /// of an in-list node) without holding a borrow across calls.
    last: Option<usize>,
}

impl<T: LinkableTrait> LinkList<T> {
    pub fn new() -> Self {
        let mut arena = Arena::new();
        let sentinel = arena.alloc(T::sentinel());
        arena.get_mut(sentinel).links_mut().next = Some(sentinel);
        arena.get_mut(sentinel).links_mut().prev = Some(sentinel);
        LinkList {
            arena,
            sentinel,
            cursor: None,
            last: None,
        }
    }

    pub fn clear(&mut self) {
        loop {
            let head = self
                .arena
                .get(self.sentinel)
                .links()
                .next
                .expect("linked list invariant");
            if head == self.sentinel {
                self.last = None;
                return;
            }
            self.arena.unlink(head);
            self.arena.take(head);
        }
    }

    pub fn push(&mut self, node: T) {
        let id = self.arena.alloc(node);
        let s = self.sentinel;
        let tail = self
            .arena
            .get(s)
            .links()
            .prev
            .expect("linked list invariant");
        {
            let n = self.arena.get_mut(id).links_mut();
            n.prev = Some(tail);
            n.next = Some(s);
        }
        self.arena.get_mut(tail).links_mut().next = Some(id);
        self.arena.get_mut(s).links_mut().prev = Some(id);
    }

    pub fn push_front(&mut self, node: T) {
        let id = self.arena.alloc(node);
        let s = self.sentinel;
        let head = self
            .arena
            .get(s)
            .links()
            .next
            .expect("linked list invariant");
        {
            let n = self.arena.get_mut(id).links_mut();
            n.prev = Some(s);
            n.next = Some(head);
        }
        self.arena.get_mut(s).links_mut().next = Some(id);
        self.arena.get_mut(head).links_mut().prev = Some(id);
    }

    pub fn pop_front(&mut self) -> Option<T> {
        let s = self.sentinel;
        let head = self
            .arena
            .get(s)
            .links()
            .next
            .expect("linked list invariant");
        if head == s {
            return None;
        }
        self.arena.unlink(head);
        Some(self.arena.take(head))
    }

    pub fn head(&mut self) -> Option<&mut T> {
        let s = self.sentinel;
        let head = self
            .arena
            .get(s)
            .links()
            .next
            .expect("linked list invariant");
        if head == s {
            self.cursor = None;
            self.last = None;
            return None;
        }
        self.cursor = self.arena.get(head).links().next;
        self.last = Some(head);
        Some(self.arena.get_mut(head))
    }

    pub fn tail(&mut self) -> Option<&mut T> {
        let s = self.sentinel;
        let tail = self
            .arena
            .get(s)
            .links()
            .prev
            .expect("linked list invariant");
        if tail == s {
            self.cursor = None;
            self.last = None;
            return None;
        }
        self.cursor = self.arena.get(tail).links().prev;
        self.last = Some(tail);
        Some(self.arena.get_mut(tail))
    }

    pub fn next(&mut self) -> Option<&mut T> {
        let node = self.cursor?;
        let s = self.sentinel;
        if node == s {
            self.cursor = None;
            self.last = None;
            return None;
        }
        self.cursor = self.arena.get(node).links().next;
        self.last = Some(node);
        Some(self.arena.get_mut(node))
    }

    pub fn prev(&mut self) -> Option<&mut T> {
        let node = self.cursor?;
        let s = self.sentinel;
        if node == s {
            self.cursor = None;
            self.last = None;
            return None;
        }
        self.cursor = self.arena.get(node).links().prev;
        self.last = Some(node);
        Some(self.arena.get_mut(node))
    }

    /// Immutable walk of the `next` chain from head to tail, the same order
    /// `head`/`next` visits. Takes `&self` (does not touch `cursor`/`last`),
    /// so hosts can read ground items through a `&Client`; use `head`/`next`
    /// when the walk needs to unlink or re-insert nodes.
    pub fn for_each(&self, mut f: impl FnMut(&T)) {
        let s = self.sentinel;
        let mut node = self
            .arena
            .get(s)
            .links()
            .next
            .expect("linked list invariant");
        while node != s {
            f(self.arena.get(node));
            node = self
                .arena
                .get(node)
                .links()
                .next
                .expect("linked list invariant");
        }
    }

    /// Remove and free the node most recently returned by `head`/`next`/
    /// `tail`/`prev` (TS `unlink()` during an iteration). No-op when there is
    /// no such node.
    pub fn unlink_last(&mut self) {
        let id = match self.last {
            Some(id) if id != self.sentinel => id,
            _ => return,
        };
        self.arena.unlink(id);
        self.arena.take(id);
        self.last = None;
    }

    /// Unlink the node most recently returned by `head`/`next`/`tail`/`prev`
    /// and re-insert it as the new head (TS `pushFront` of an in-list node).
    /// No-op when there is no such node.
    pub fn move_last_to_front(&mut self) {
        let id = match self.last {
            Some(id) if id != self.sentinel => id,
            _ => return,
        };
        self.arena.unlink(id);
        let s = self.sentinel;
        let head = self
            .arena
            .get(s)
            .links()
            .next
            .expect("linked list invariant");
        {
            let n = self.arena.get_mut(id).links_mut();
            n.prev = Some(s);
            n.next = Some(head);
        }
        self.arena.get_mut(s).links_mut().next = Some(id);
        self.arena.get_mut(head).links_mut().prev = Some(id);
    }
}

impl<T: LinkableTrait> Default for LinkList<T> {
    fn default() -> Self {
        LinkList::new()
    }
}

/// Circular doubly-linked list on the `next2`/`prev2` chain, borrowing an
/// arena (TS `LinkList2.ts`).
pub struct LinkList2<T: LinkableTrait> {
    sentinel: usize,
    cursor: Option<usize>,
    marker: PhantomData<fn() -> T>,
}

impl<T: LinkableTrait> LinkList2<T> {
    pub fn new(arena: &mut Arena<T>) -> Self {
        let sentinel = arena.alloc(T::sentinel());
        arena.get_mut(sentinel).links_mut().next2 = Some(sentinel);
        arena.get_mut(sentinel).links_mut().prev2 = Some(sentinel);
        LinkList2 {
            sentinel,
            cursor: None,
            marker: PhantomData,
        }
    }

    pub fn push(&mut self, arena: &mut Arena<T>, id: usize) {
        if arena.get(id).links().prev2.is_some() {
            arena.unlink2(id);
        }
        let s = self.sentinel;
        let tail = arena.get(s).links().prev2.expect("linked list invariant");
        {
            let n = arena.get_mut(id).links_mut();
            n.prev2 = Some(tail);
            n.next2 = Some(s);
        }
        arena.get_mut(tail).links_mut().next2 = Some(id);
        arena.get_mut(s).links_mut().prev2 = Some(id);
    }

    pub fn pop_front(&mut self, arena: &mut Arena<T>) -> Option<usize> {
        let s = self.sentinel;
        let head = arena.get(s).links().next2.expect("linked list invariant");
        if head == s {
            return None;
        }
        arena.unlink2(head);
        Some(head)
    }

    pub fn head(&mut self, arena: &Arena<T>) -> Option<usize> {
        let s = self.sentinel;
        let head = arena.get(s).links().next2.expect("linked list invariant");
        if head == s {
            self.cursor = None;
            return None;
        }
        self.cursor = arena.get(head).links().next2;
        Some(head)
    }

    pub fn next(&mut self, arena: &Arena<T>) -> Option<usize> {
        let node = self.cursor?;
        let s = self.sentinel;
        if node == s {
            self.cursor = None;
            return None;
        }
        self.cursor = arena.get(node).links().next2;
        Some(node)
    }

    pub fn size(&self, arena: &Arena<T>) -> usize {
        let mut count = 0;
        let s = self.sentinel;
        let mut node = arena.get(s).links().next2.expect("linked list invariant");
        while node != s {
            count += 1;
            node = arena
                .get(node)
                .links()
                .next2
                .expect("linked list invariant");
        }
        count
    }
}
