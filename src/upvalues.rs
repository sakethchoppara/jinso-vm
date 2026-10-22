use crate::gc::{Heap, Upvalue};
use crate::value::Value;

/// The VM's open upvalues: captured variables that still live in a stack slot, sorted by
/// slot. Two closures capturing the same variable share one upvalue, which is how a write
/// through one closure is seen by the other and by the function that owns the variable.
#[derive(Default)]
pub struct OpenUpvalues(Vec<(usize, u32)>);

impl OpenUpvalues {
    /// The upvalue for `slot`, creating it if no closure has captured that slot yet.
    /// Allocates without collecting: the caller must collect first if one is due.
    pub fn capture(&mut self, heap: &mut Heap, slot: usize) -> u32 {
        match self.0.binary_search_by_key(&slot, |&(s, _)| s) {
            Ok(i) => self.0[i].1,
            Err(i) => {
                let h = heap.alloc_upvalue(slot);
                self.0.insert(i, (slot, h));
                h
            }
        }
    }

    /// Closes every upvalue at or above `from`, copying the slot's value into the upvalue,
    /// because those slots are about to be popped (block exit) or truncated (return).
    #[inline] // runs on every Return, where the list is usually empty
    pub fn close(&mut self, heap: &mut Heap, stack: &[Value], from: usize) {
        while let Some(&(slot, h)) = self.0.last() {
            if slot < from {
                break;
            }
            heap.set_upvalue(h, Upvalue::Closed(stack[slot]));
            self.0.pop();
        }
    }

    /// GC roots: an upvalue only the open list knows about must survive, so a later
    /// closure capturing the same slot finds it instead of a freed handle.
    pub fn handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().map(|&(_, h)| h)
    }
}
