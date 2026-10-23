# 0004: Symbolic jumps and relocation

**Status:** accepted.

## Before

From v1 to v2, the compiler wrote bytes straight into `Chunk::code`. A forward jump was
emitted with a placeholder offset (`0xFFFF`) and backpatched once its target was known;
`while` computed its `Loop` offset from a saved byte position; each function's address was
the byte position where its body started.

That's the simplest thing that works for a compiler that never changes its output after
writing it. An optimizer has to: removing or replacing even one instruction shifts every
byte after it, so every jump that crosses it would need its offset recomputed, and every
function address after it would move. Backpatched offsets record *where* a jump goes as a
number that's only valid for the exact byte layout at the time it was written.

## Now

The compiler emits a `Vec<Item>` (`src/optimize.rs`):

- `Ins { op, arg, line }`: an ordinary instruction.
- `Goto { target, line }`: an unconditional jump to a label.
- `Branch { op, target, line }`: a conditional jump (`JumpIfFalse`, `JumpIfNot*`) to a label.
- `Label(l)`: marks a position, emits nothing.

Function bodies are bracketed by start and end labels. `relocate` lays the list out in two
passes: the first computes every label's address, the second writes bytes and offsets.
A `Goto` becomes `Jump` if its target is ahead and `Loop` if it's behind, so a pass that
retargets a jump never has to think about direction. Conditional jumps stay forward-only
(the VM has no backward conditional jump), and relocation asserts it.

## Checks

- The bytecode for every program in `examples/` and `bench/` is byte-for-byte identical to
  v2's, compared with `jinso dis` before and after.
- Instruction counts are unchanged: fib 1135M, loop 5012M, strings 706M.

## Cost

One more pass over the program at compile time, and a `Vec<Item>` that's discarded after
relocation. `relocate` appends to whatever code the chunk already has, which is what the
REPL needs to add each input to a growing program.
