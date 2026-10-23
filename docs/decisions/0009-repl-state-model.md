# 0009: What the REPL keeps between inputs

**Status:** accepted.

## The model

A REPL session is one growing program. Each input is compiled onto the end of the same
`Chunk` (`Compiler::add`) and run from where its code starts (`Vm::run(chunk, start, ...)`).
So nothing needs translating between inputs: a function defined in input 1 is a `Value::Fn`
whose bytecode is still at the same offset in input 50, and a global keeps its slot.

| State | Lives in | Between inputs | After a syntax or compile error | After a runtime error |
|---|---|---|---|---|
| Bytecode, line table | `Chunk` | kept, appended to | the failed input's part removed | kept |
| Constants, string literals, functions | `Chunk` | kept, appended to | the failed input's part removed | kept |
| Global name → slot table | `Compiler` | kept | the failed input's names removed | kept |
| Global values | `Vm` | kept | unchanged (nothing ran) | kept, including ones set before the error |
| Heap | `Vm` | kept | unchanged | **kept** |
| Open upvalues | `Vm` | none (every scope has ended) | none | **closed**, then cleared |
| Value stack, call frames | `Vm` | empty | empty | **reset** to empty |

## Why the heap is never reset

Everything a global refers to lives on the heap: strings, arrays, closures and their
upvalues. Resetting the heap after an error would leave every earlier global pointing at
freed objects. Garbage collection already removes what's unreachable, so after an error
the objects the failed input created and dropped are simply collected later.

## Why open upvalues are closed, not dropped, after an error

A runtime error can happen while a closure created in that same input still points into
the stack:

```
let g = nil;
fn mk() { let v = ["captured"]; fn get() { return v; } g = get; return 1 / 0; }
mk();       // line 1: division by zero
print g();  // ["captured"]
```

When `1 / 0` fails, `v` is a live open upvalue in `mk`'s frame, and `get` is already
stored in the global `g`. Clearing the stack without closing the upvalue would leave `g`
reading a slot that no longer exists. Recovery closes every open upvalue first, copying
the values out, then clears the stack. Removing that line is mutant
`8-recovery-closes-upvalues`, and `a_closure_survives_an_error_in_the_call_that_made_it`
catches it.

## Why a failed compile rolls back

`let zed = 1; fn f() { return 1; } return 2;` compiles `zed` and `f` before reaching the
`return` that's illegal at the top level. If those stayed in the global table and chunk,
`zed` would have a slot but no value (reading it would be "undefined variable", which is
fine), but `f`'s `FnInfo` and constants would sit in the chunk referring to code that was
never laid out. `Compiler::add` records the length of everything it appends to and
truncates back on any error, so a failed input leaves no trace.

## The VM catches up on what the chunk gained

The VM keeps its own copy of the constants (with string literals turned into heap
handles), its own decoded copy of the code, and the global values. At the start of each
`run` it loads only what was appended since the last one: new literals into the heap, new
constants, new global slots (built-ins like `len` bound by name), and the new code decoded.
Loading allocates without collecting, so nothing new is unrooted in between.

## Cost

Moving the VM's state into a struct that outlives one run made the hot loop read through
references. Rebinding the code, constants and globals as plain slices for the duration of
a run, and building with one codegen unit, kept every benchmark within a couple of percent
of the week 6 build, and faster on most.
