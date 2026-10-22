use crate::value::Value;

/// First collection happens once this many bytes are live; the threshold then doubles.
const INITIAL_THRESHOLD: usize = 1 << 20;

/// Mark-and-sweep heap of strings and arrays. Values refer to objects by index, so there
/// are no raw pointers: a GC bug shows up as a "dangling handle" panic instead of memory
/// corruption.
///
/// A string value is a view `(handle, len)` of a prefix of a buffer. Buffers only ever grow
/// at the end, so a view never changes what it sees: that is what lets `try_append` extend a
/// buffer in place while other values still point into it. Arrays are plain shared objects:
/// every value holding the handle sees every change.
pub struct Heap {
    objects: Vec<Option<Obj>>,
    free: Vec<u32>,
    /// Gray objects during marking. Kept between collections to reuse its allocation.
    work: Vec<u32>,
    bytes: usize,
    next_gc: usize,
    stress: bool,
    collections: usize,
    freed: usize,
    peak_work: usize,
}

struct Obj {
    marked: bool,
    kind: Kind,
}

enum Kind {
    Str(String),
    Array(Vec<Value>),
}

#[derive(Debug)]
pub struct Stats {
    pub collections: usize,
    pub freed: usize,
    pub live: usize,
    /// Most objects ever waiting on the mark work stack at once.
    pub peak_work: usize,
}

/// Bytes charged to the GC for an object: its buffer capacity plus the slot.
fn size(kind: &Kind) -> usize {
    std::mem::size_of::<Obj>()
        + match kind {
            Kind::Str(s) => s.capacity(),
            Kind::Array(items) => items.capacity() * std::mem::size_of::<Value>(),
        }
}

impl Heap {
    /// Seeds the heap with the program's string literals, so literal `i` is handle `i`
    /// and the compiler can emit `Value::Str` constants before the heap exists.
    /// `stress` collects before every allocation, to flush out missing roots.
    pub fn new(literals: &[String], stress: bool) -> Heap {
        let mut heap = Heap {
            objects: Vec::new(),
            free: Vec::new(),
            work: Vec::new(),
            bytes: 0,
            next_gc: INITIAL_THRESHOLD,
            stress,
            collections: 0,
            freed: 0,
            peak_work: 0,
        };
        for s in literals {
            heap.alloc_str(s.clone());
        }
        heap
    }

    fn obj(&self, h: u32) -> &Obj {
        self.objects[h as usize].as_ref().expect("dangling handle: GC freed a live object")
    }

    fn obj_mut(&mut self, h: u32) -> &mut Obj {
        self.objects[h as usize].as_mut().expect("dangling handle: GC freed a live object")
    }

    fn buf(&self, h: u32) -> &String {
        match &self.obj(h).kind {
            Kind::Str(s) => s,
            Kind::Array(_) => unreachable!("string handle points at an array"),
        }
    }

    pub fn get(&self, h: u32, len: u32) -> &str {
        &self.buf(h)[..len as usize]
    }

    pub fn array(&self, h: u32) -> &[Value] {
        match &self.obj(h).kind {
            Kind::Array(items) => items,
            Kind::Str(_) => unreachable!("array handle points at a string"),
        }
    }

    pub fn array_mut(&mut self, h: u32) -> &mut Vec<Value> {
        match &mut self.obj_mut(h).kind {
            Kind::Array(items) => items,
            Kind::Str(_) => unreachable!("array handle points at a string"),
        }
    }

    /// Appends to an array, charging any buffer growth to the GC. Allocates no object,
    /// so it never needs a collection.
    pub fn push(&mut self, h: u32, v: Value) {
        let items = self.array_mut(h);
        let before = items.capacity();
        items.push(v);
        let grown = items.capacity() - before;
        self.bytes += grown * std::mem::size_of::<Value>();
    }

    /// Never collects by itself: the caller must collect first (see `should_collect`)
    /// while every value it still needs is reachable from the roots.
    fn alloc(&mut self, kind: Kind) -> u32 {
        self.bytes += size(&kind);
        let obj = Some(Obj { marked: false, kind });
        // In stress mode freed slots are never reused, so a stale handle hits None and
        // panics instead of silently reading whatever got allocated there next.
        match self.free.pop().filter(|_| !self.stress) {
            Some(h) => {
                self.objects[h as usize] = obj;
                h
            }
            None => {
                self.objects.push(obj);
                self.objects.len() as u32 - 1
            }
        }
    }

    pub fn alloc_str(&mut self, s: String) -> Value {
        let len = s.len() as u32;
        Value::Str { h: self.alloc(Kind::Str(s)), len }
    }

    pub fn alloc_array(&mut self, items: Vec<Value>) -> Value {
        Value::Array(self.alloc(Kind::Array(items)))
    }

    /// `a + b` without copying `a`, when `a` ends exactly where its buffer does (nothing has
    /// appended past it yet). This makes `s = s + x` in a loop amortized O(len x) rather than
    /// O(len s). Returns None if another value already extended the buffer; then the caller
    /// must copy. Allocates no object, so it never needs a collection.
    // ponytail: a short view keeps its whole buffer alive; fine until substrings exist.
    pub fn try_append(&mut self, (ah, alen): (u32, u32), (bh, blen): (u32, u32)) -> Option<Value> {
        if self.buf(ah).len() != alen as usize {
            return None;
        }
        let tail = self.get(bh, blen).to_owned(); // b may be the same buffer as a
        let Kind::Str(s) = &mut self.obj_mut(ah).kind else { unreachable!() };
        let before = s.capacity();
        s.push_str(&tail);
        let grown = s.capacity() - before;
        self.bytes += grown;
        Some(Value::Str { h: ah, len: alen + blen })
    }

    pub fn should_collect(&self) -> bool {
        self.stress || self.bytes > self.next_gc
    }

    /// Mark everything reachable from `roots`, then sweep the rest.
    ///
    /// Marking uses an explicit work stack instead of recursion, so an array nested a
    /// million deep can't overflow the Rust stack. The mark bit is set when an object is
    /// first reached, so a cycle (an array containing itself) is traversed once.
    pub fn collect(&mut self, roots: impl Iterator<Item = Value>) {
        let mut work = std::mem::take(&mut self.work);
        work.extend(roots.filter_map(Value::handle));
        while let Some(h) = work.pop() {
            let obj = self.objects[h as usize].as_mut().expect("dangling handle reached while marking");
            if obj.marked {
                continue;
            }
            obj.marked = true;
            if let Kind::Array(items) = &obj.kind {
                work.extend(items.iter().filter_map(|v| v.handle()));
                self.peak_work = self.peak_work.max(work.len());
            }
        }
        self.work = work;

        for (h, slot) in self.objects.iter_mut().enumerate() {
            match slot {
                Some(obj) if obj.marked => obj.marked = false,
                Some(obj) => {
                    self.bytes -= size(&obj.kind);
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
        Stats {
            collections: self.collections,
            freed: self.freed,
            live: self.objects.iter().flatten().count(),
            peak_work: self.peak_work,
        }
    }
}
