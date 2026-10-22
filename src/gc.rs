use crate::value::Value;

/// First collection happens once this many bytes are live; the threshold then doubles.
const INITIAL_THRESHOLD: usize = 1 << 20;

/// Mark-and-sweep heap. Objects live in slots and values refer to them by index
/// (`Value::Str(handle)`), so there are no raw pointers: a GC bug shows up as a
/// "dangling handle" panic instead of memory corruption.
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
    s: Box<str>,
}

#[derive(Debug)]
pub struct Stats {
    pub collections: usize,
    pub freed: usize,
    pub live: usize,
}

fn size(s: &str) -> usize {
    s.len() + std::mem::size_of::<Obj>()
}

impl Heap {
    /// Seeds the heap with the program's string literals, so literal `i` is handle `i`
    /// and the compiler can emit `Value::Str(i)` constants before the heap exists.
    /// `stress` collects before every allocation, to flush out missing roots.
    pub fn new(literals: &[String], stress: bool) -> Heap {
        let mut heap = Heap { objects: Vec::new(), free: Vec::new(), bytes: 0, next_gc: INITIAL_THRESHOLD, stress, collections: 0, freed: 0 };
        for s in literals {
            heap.alloc(s.clone());
        }
        heap
    }

    pub fn get(&self, h: u32) -> &str {
        &self.objects[h as usize].as_ref().expect("dangling handle: GC freed a live object").s
    }

    /// Never collects by itself: the caller must collect first (see `should_collect`)
    /// while every value it still needs is reachable from the roots.
    pub fn alloc(&mut self, s: String) -> Value {
        self.bytes += size(&s);
        let obj = Some(Obj { marked: false, s: s.into_boxed_str() });
        // In stress mode freed slots are never reused, so a stale handle hits None and
        // panics instead of silently reading whatever got allocated there next.
        match self.free.pop().filter(|_| !self.stress) {
            Some(h) => {
                self.objects[h as usize] = obj;
                Value::Str(h)
            }
            None => {
                self.objects.push(obj);
                Value::Str(self.objects.len() as u32 - 1)
            }
        }
    }

    pub fn should_collect(&self) -> bool {
        self.stress || self.bytes > self.next_gc
    }

    /// Mark everything reachable from `roots`, then sweep the rest.
    pub fn collect(&mut self, roots: impl Iterator<Item = Value>) {
        // ponytail: strings hold no references, so marking is one pass over the roots.
        // Objects that point to objects (arrays, closures) will need a gray worklist here.
        for v in roots {
            if let Value::Str(h) = v {
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
