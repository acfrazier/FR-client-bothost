// Port of `~/experiments/Server/webclient/src/datastruct/Linkable.ts` and
// `Linkable2.ts`. The two TS classes collapse into one node: an arena stores
// each node's key plus two independent link chains (`next`/`prev` and
// `next2`/`prev2`), so a single node can sit in a `LinkList` and a
// `HashTable`/`LinkList2` at the same time, exactly like the TS subclass
// `Linkable2 extends Linkable`. All links are arena indices, which keeps every
// container `Send` (no `Rc<RefCell>`).

/// Link state carried by every arena node (TS `Linkable` + `Linkable2`).
#[derive(Clone)]
pub struct Links {
    pub key: i64,
    pub(crate) next: Option<usize>,
    pub(crate) prev: Option<usize>,
    pub(crate) next2: Option<usize>,
    pub(crate) prev2: Option<usize>,
}

impl Links {
    pub fn new(key: i64) -> Self {
        Links {
            key,
            next: None,
            prev: None,
            next2: None,
            prev2: None,
        }
    }
}

/// Node payloads usable by the datastruct containers.
pub trait LinkableTrait: Sized {
    fn links(&self) -> &Links;
    fn links_mut(&mut self) -> &mut Links;
    /// Build an inert node to serve as a list/table sentinel.
    fn sentinel() -> Self;
}

/// Base node: a key plus two link chains (see module docs).
pub struct Linkable {
    links: Links,
}

impl Linkable {
    pub fn new(key: i64) -> Self {
        Linkable {
            links: Links {
                key,
                next: None,
                prev: None,
                next2: None,
                prev2: None,
            },
        }
    }
}

impl std::ops::Deref for Linkable {
    type Target = Links;

    fn deref(&self) -> &Links {
        &self.links
    }
}

impl std::ops::DerefMut for Linkable {
    fn deref_mut(&mut self) -> &mut Links {
        &mut self.links
    }
}

impl LinkableTrait for Linkable {
    fn links(&self) -> &Links {
        &self.links
    }

    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }

    fn sentinel() -> Self {
        Linkable::new(0)
    }
}

/// Index arena owning every node of a container.
pub struct Arena<T: LinkableTrait> {
    nodes: Vec<Option<T>>,
    free: Vec<usize>,
}

impl<T: LinkableTrait> Arena<T> {
    pub fn new() -> Self {
        Arena {
            nodes: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn alloc(&mut self, node: T) -> usize {
        if let Some(id) = self.free.pop() {
            self.nodes[id] = Some(node);
            id
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    pub fn get(&self, id: usize) -> &T {
        self.nodes[id].as_ref().expect("arena slot is occupied")
    }

    pub fn get_mut(&mut self, id: usize) -> &mut T {
        self.nodes[id].as_mut().expect("arena slot is occupied")
    }

    /// Remove the node from its `next`/`prev` chain (TS `Linkable.unlink`).
    /// No-op when the node is not linked.
    pub fn unlink(&mut self, id: usize) {
        let (prev, next) = {
            let n = self.get(id).links();
            (n.prev, n.next)
        };
        if let Some(p) = prev {
            self.get_mut(p).links_mut().next = next;
            if let Some(nx) = next {
                self.get_mut(nx).links_mut().prev = Some(p);
            }
            let n = self.get_mut(id).links_mut();
            n.next = None;
            n.prev = None;
        }
    }

    /// Remove the node from its `next2`/`prev2` chain (TS `Linkable2.unlink2`).
    /// No-op when the node is not linked.
    pub fn unlink2(&mut self, id: usize) {
        let (prev2, next2) = {
            let n = self.get(id).links();
            (n.prev2, n.next2)
        };
        if let Some(p) = prev2 {
            self.get_mut(p).links_mut().next2 = next2;
            if let Some(nx) = next2 {
                self.get_mut(nx).links_mut().prev2 = Some(p);
            }
            let n = self.get_mut(id).links_mut();
            n.next2 = None;
            n.prev2 = None;
        }
    }

    /// Extract the node and free its slot.
    pub fn take(&mut self, id: usize) -> T {
        let node = self.nodes[id].take().expect("arena slot is occupied");
        self.free.push(id);
        node
    }
}

impl<T: LinkableTrait> Default for Arena<T> {
    fn default() -> Self {
        Arena::new()
    }
}
