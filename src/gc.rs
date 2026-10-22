use crate::value::Value;

/// First collection happens once this many bytes are live; the threshold then doubles.
const INITIAL_THRESHOLD: usize = 1 << 20;

/// Mark-and-sweep heap of string buffers. Values refer to objects by index, so there are no
/// raw pointers: a GC bug shows up as a "dangling handle" panic instead of memory corruption.
///
/// A string value is a view `(handle, len)` of a prefix of a buffer. Buffers only ever grow
/// at the end, so a view never changes what it sees: that is what lets `try_append` extend a
/// buffer in place while other values still point into it.
pub struct Heap {
    objects: Vec<Option<Obj>>,
    free: Vec<u32>,
    bytes: usize,
    next_gc: usize,
    stress: bool,
    collections: usize,
    freed: usize,
}

struct Obj {
    marked: bool,
    s: String,
}

#[derive(Debug)]
pub struct Stats {
    pub collections: usize,
    pub freed: usize,
    pub live: usize,
}

/// Bytes charged to the GC for an object: its buffer capacity plus the slot.
fn size(s: &String) -> usize {
    s.capacity() + std::mem::size_of::<Obj>()
}

impl Heap {
    /// Seeds the heap with the program's string literals, so literal `i` is handle `i`
    /// and the compiler can emit `Value::Str` constants before the heap exists.
    /// `stress` collects before every allocation, to flush out missing roots.
    pub fn new(literals: &[String], stress: bool) -> Heap {
        let mut heap = Heap { objects: Vec::new(), free: Vec::new(), bytes: 0, next_gc: INITIAL_THRESHOLD, stress, collections: 0, freed: 0 };
        for s in literals {
            heap.alloc(s.clone());
        }
        heap
    }

    fn obj(&self, h: u32) -> &Obj {
        self.objects[h as usize].as_ref().expect("dangling handle: GC freed a live object")
    }

    pub fn get(&self, h: u32, len: u32) -> &str {
        &self.obj(h).s[..len as usize]
    }

    /// Never collects by itself: the caller must collect first (see `should_collect`)
    /// while every value it still needs is reachable from the roots.
    pub fn alloc(&mut self, s: String) -> Value {
        let len = s.len() as u32;
        self.bytes += size(&s);
        let obj = Some(Obj { marked: false, s });
        // In stress mode freed slots are never reused, so a stale handle hits None and
        // panics instead of silently reading whatever got allocated there next.
        let h = match self.free.pop().filter(|_| !self.stress) {
            Some(h) => {
                self.objects[h as usize] = obj;
                h
            }
            None => {
                self.objects.push(obj);
                self.objects.len() as u32 - 1
            }
        };
        Value::Str { h, len }
    }

    /// `a + b` without copying `a`, when `a` ends exactly where its buffer does (nothing has
    /// appended past it yet). This makes `s = s + x` in a loop amortized O(len x) rather than
    /// O(len s). Returns None if another value already extended the buffer; then the caller
    /// must copy. Allocates no object, so it never needs a collection.
    pub fn try_append(&mut self, (ah, alen): (u32, u32), (bh, blen): (u32, u32)) -> Option<Value> {
        if self.obj(ah).s.len() != alen as usize {
            return None;
        }
        let tail = self.get(bh, blen).to_owned(); // b may be the same buffer as a
        let obj = self.objects[ah as usize].as_mut().unwrap();
        let before = size(&obj.s);
        obj.s.push_str(&tail);
        self.bytes += size(&obj.s) - before;
        Some(Value::Str { h: ah, len: alen + blen })
    }

    pub fn should_collect(&self) -> bool {
        self.stress || self.bytes > self.next_gc
    }

    /// Mark everything reachable from `roots`, then sweep the rest.
    pub fn collect(&mut self, roots: impl Iterator<Item = Value>) {
        // ponytail: strings hold no references, so marking is one pass over the roots.
        // Objects that point to objects (arrays, closures) will need a gray worklist here.
        // A short view keeps its whole buffer alive; fine until substrings exist.
        for v in roots {
            if let Value::Str { h, .. } = v {
                self.objects[h as usize].as_mut().expect("dangling handle in roots").marked = true;
            }
        }
        for (h, slot) in self.objects.iter_mut().enumerate() {
            match slot {
                Some(obj) if obj.marked => obj.marked = false,
                Some(obj) => {
                    self.bytes -= size(&obj.s);
                    self.freed += 1;
                    *slot = None;
                    self.free.push(h as u32);
                }
                None => {}
            }
        }
        self.collections += 1;
        self.next_gc = (self.bytes * 2).max(INITIAL_THRESHOLD);
    }

    pub fn stats(&self) -> Stats {
        Stats { collections: self.collections, freed: self.freed, live: self.objects.iter().flatten().count() }
    }
}
