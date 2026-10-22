# Lumen

[![CI](https://github.com/sakethchoppara/lumen-vm/actions/workflows/ci.yml/badge.svg)](https://github.com/sakethchoppara/lumen-vm/actions/workflows/ci.yml)

**[Try it in your browser →](https://sakethchoppara.github.io/lumen-vm/)**

A small scripting language implemented from scratch in Rust: a lexer, a recursive-descent
parser, a compiler that emits bytecode, a stack-based virtual machine with closures and
arrays, and a mark-and-sweep garbage collector. No dependencies.

```
fn make_counter() {
  let count = 0;
  fn next() {
    count = count + 1;
    return count;
  }
  return next;
}

let c = make_counter();
print c();   // 1
print c();   // 2: count outlived the call that created it
```

## Quick start

```sh
cargo build --release
./target/release/lumen run examples/closures.lm          # run a program
./target/release/lumen dis examples/closures.lm          # show its bytecode
./target/release/lumen ast examples/arrays.lm            # show its syntax tree
./target/release/lumen run --gc-stress examples/strings.lm   # collect garbage on every allocation

cargo test                          # ~190 cases, each run normally and under --gc-stress
./scripts/bench.sh                  # benchmark against CPython
cargo run --example fuzz -- 600     # 10 minutes of random input

./scripts/build-web.sh && python3 -m http.server -d web   # playground at localhost:8000
```

## Playground

`web/` is a static page that runs the same compiler and VM, built for
`wasm32-unknown-unknown`, with Output, Bytecode and Syntax tree tabs. It uses no
wasm-bindgen. `src/wasm.rs` exports two plain functions (`alloc`, `run`) and imports one,
`emit(kind, ptr, len)`, which it calls with the syntax tree, the bytecode, each batch of
printed lines as they happen, and finally any error. The VM runs in a Web Worker, so a
program that never ends is stopped after 5 seconds instead of freezing the tab, and the
output it printed before then stays on screen. Output is capped at 10,000 lines. GitHub
Actions rebuilds it and deploys it to Pages on every push to `main`.

## The language

| | |
|---|---|
| Types | `int` (64-bit, overflow is an error), `bool`, `str`, `nil`, functions, arrays |
| Operators | `+ - * / %`, `== != < <= > >=`, `and or not` (short-circuiting), `-x` |
| Statements | `let x = e;`, `x = e;`, `a[i] = e;`, `print e;`, `if c { } else if c { } else { }`, `while c { }` |
| Functions | `fn name(a, b) { return a + b; }`, recursion, functions as values, nested functions |
| Closures | a nested function can read and assign its enclosing functions' variables, even after they return |
| Arrays | `[1, 2, 3]`, `a[i]`, `len(a)`, `push(a, v)`; shared by reference, may nest or contain themselves |
| Scoping | top-level variables are globals; variables declared in a block or function are locals |
| Comments | `// to end of line` |

Lumen is dynamically typed with strict checks: `1 + true`, `if 1 { }` and `not 5` are
runtime errors, not silent conversions. Every error carries a source location and the VM
never panics on user input:

```
3:8: expected expression, got ';'
line 4: operands must be ints, got str and int
line 2: index 5 out of bounds for array of length 3
line 1: stack overflow (more than 4096 nested calls)
```

Syntax errors don't stop at the first one: the parser recovers at the next statement and
reports them all.

## Architecture

```
source ──▶ lexer ──▶ tokens ──▶ parser ──▶ AST ──▶ compiler ──▶ Chunk ──▶ VM
           lexer.rs             parser.rs  ast.rs  compiler.rs  chunk.rs  vm.rs
                                                                          │
                                gc.rs: strings, arrays, closures ◀────────┤
                                upvalues.rs: open upvalue list   ◀────────┘
```

| File | Role |
|---|---|
| `lexer.rs` | Source to tokens with line and column |
| `parser.rs` | Recursive descent for statements, precedence climbing for expressions, error recovery |
| `ast.rs` | Syntax tree, printable as S-expressions (`lumen ast`) |
| `compiler.rs` | AST to bytecode: resolves every name to a local slot, an upvalue or a global; backpatches jumps |
| `chunk.rs` | Opcodes, the `Vec<u8>` bytecode, constant pool, line table, function and upvalue descriptors |
| `vm.rs` | Decodes the bytecode once, then runs it: value stack, call frames, built-ins |
| `gc.rs` | Heap of strings, arrays, closures and upvalues; mark-and-sweep with an explicit work stack |
| `upvalues.rs` | The open upvalues, sorted by stack slot, and closing them |
| `disasm.rs` | Bytecode listing (`lumen dis`) |
| `wasm.rs` | Exports for the browser playground |

### What `fib` compiles to

```
0000    1 Jump           45  (-> 0048)      skip over the function body
== fn fib ==
0003    2 GetLocal        0                 n is stack slot 0 of this frame
0006    | Constant        0  (2)
0009    | JumpIfNotLt     4  (-> 0016)      `if n < 2`: compare and branch in one instruction
0012    | GetLocal        0
0015    | Return
0016    3 GetGlobal       0  (fib)
0019    | GetLocal        0
0022    | Constant        1  (1)
0025    | Sub
0026    | Call            1
0029    | GetGlobal       0  (fib)
0032    | GetLocal        0
0035    | Constant        2  (2)
0038    | Sub
0039    | Call            1
0042    | Add
0043    | Return
0044    1 Constant        3  (nil)          implicit `return nil;`
0047    | Return
0048    | Constant        4  (<fn fib>)
0051    | DefineGlobal    0  (fib)
...
```

### How the pieces work

**Precedence.** Each infix operator has a binding power. `expr(min)` only consumes operators
at least as strong as `min` and parses the right operand with `p + 1`, which makes
`10 - 3 - 2` group as `(10 - 3) - 2`. `not` binds looser than comparisons, as in Python, so
`not a < b` means `not (a < b)`.

**Jump patching.** When the compiler reaches `if`, it doesn't yet know how long the body
will be. It emits the jump with offset `0xFFFF`, remembers the operand's position, compiles
the body, then writes the real offset back. `while` jumps backwards with `Loop`, whose
offset is known immediately. `and`/`or` compile to the same jumps, so `false and 1/0 == 0`
never divides.

**Variables.** Nothing is looked up by name at runtime. Globals are resolved to indices at
compile time. A global's slot stays "undefined" until its `let` runs, so functions can call
functions defined below them, and reading a global too early is still a clean runtime error.
Locals live directly on the value stack. Declaring one emits no code at all, because the
initializer's value is already sitting in the right slot. Leaving a block pops its locals.

**Calls.** The caller pushes the function and its arguments, then `Call argc`. The new
frame's base points at the first argument, so parameters are just the callee's first local
slots. `Return` truncates the stack back to the callee and leaves the return value there.
A frame is `(return_ip, base, closure)`. `len` and `push` are built-ins: a small table in
the VM, bound to ordinary globals, so a program can still define its own `len`.

### Closures

A variable a nested function uses lives on the stack like any other local, until the
function that owns it returns. A closure can't hold a pointer to that slot forever, and
copying the value would break sharing:

```
fn pair() { let v = 0; fn get() { return v; } fn set(x) { v = x; } return [get, set]; }
```

`get` and `set` must see the *same* `v`, during the call and after it. Lumen does what Lua
does. Each captured variable gets one **upvalue** object, which every closure capturing that
variable shares:

```
 while pair() runs                          after pair() returns

 stack                                      stack
 ┌───────────┐
 │ v = 0     │◀───────┐                    (pair's slots are gone)
 ├───────────┤        │ Open(slot)
 │ ...       │   ┌────┴─────┐                ┌──────────────────┐
 └───────────┘   │ upvalue  │                │ upvalue          │
                 └──────────┘                │ Closed(v = 42)   │
                   ▲      ▲                  └──────────────────┘
          ┌────────┘      └───────┐              ▲          ▲
     closure get          closure set       closure get   closure set
```

- **Compiler.** When a name isn't a local of the current function, the compiler looks for it
  in each enclosing function in turn. If it finds it, the variable is marked *captured*, and
  every function in between gets an upvalue descriptor: "slot `i` of my enclosing frame" for
  the innermost one, "upvalue `j` of my enclosing closure" for the others. That's what makes
  three levels of nesting work.
- **Creating a closure.** `Closure f` reads `f`'s descriptors. For a slot, it asks the open
  list for that slot's upvalue, creating one only if no closure has captured the slot yet.
  For an upvalue, it copies the handle from the running closure.
- **Open and closed.** An open upvalue reads and writes the stack slot, so the owning
  function and every closure see each other's writes. When the slot goes away, at the end
  of its block (`CloseUpvalue`) or on `Return`, the VM copies the value into the upvalue and
  drops it from the open list. Closures keep working without noticing.
- **Fresh per call.** Each call to `make_counter()` has new stack slots, so new upvalues: two
  counters are independent. A `let` inside a loop body is closed at the end of each
  iteration, so closures made in different iterations capture different variables.
- **No cost when unused.** A function that captures nothing compiles to a plain function
  value and allocates nothing.

### Arrays

Arrays are heap objects, and a value holds only a handle to one, so `let b = a;` gives two
names for one array and `push(b, 1)` shows up in `a`. `==` compares identity. Arrays can
contain anything, including other arrays, functions and themselves. Printing tracks the
arrays it's inside, so a cycle prints as `[...]`:

```
let me = ["self"];
push(me, me);
print me;        // ["self", [...]]
```

### Garbage collection

Every heap object (strings, arrays, closures, upvalues) lives in a slot. Values hold a slot
index, not a pointer, so a collector bug shows up as a "dangling handle" panic, never memory
corruption. The roots are:

- the value stack, which holds every frame's arguments and locals, and each frame's callee,
  so every running closure,
- the globals,
- the constant pool, since string literals are loaded into the heap at startup,
- the open upvalues.

From the roots, marking follows array elements, closures' upvalues, and closed upvalues'
values. It uses an explicit work stack, not recursion, so an array nested 200,000 deep marks
without overflowing the Rust stack; in that test the work stack never held more than two
entries. Each object's mark bit is set the first time it's reached, so cycles (an array
containing itself, a closure whose upvalue holds the closure) are visited once and collected
when unreachable.

A collection runs when live bytes pass a threshold, and the threshold then doubles. With
`--gc-stress` it runs before *every* allocation and freed slots are never reused, so a
missing root crashes immediately instead of reading a recycled object. The whole test suite
runs in both modes and must produce identical output.

**Root-removal experiment.** Each root and each edge marking follows was removed on purpose,
to check that some test fails. `scripts/root_removal.sh` automates this: it applies each
deliberate bug in `scripts/mutants/` to a temporary copy of the code, runs the tests, and
fails if any bug goes unnoticed or if a patch no longer applies to the current code. CI
runs it on every push (it takes about 12 seconds locally). Current results:

| Removed | Tests that fail |
|---|---|
| value stack as a root | `programs`, `arrays`, `closures`, `unreachable_cycles_are_collected` |
| globals as a root | `arrays`, `closures` |
| constant pool as a root | `programs`, `arrays`, `closures`, both `gc_*` tests |
| array elements | `arrays`, `closures` |
| open upvalues as a root | `closures` |
| a closure's upvalues | `closures` |
| a closed upvalue's value | `closures` |

The first round missed three of these. In v1, the globals case passed because every global
string was also on the stack whenever anything was allocated. In v2, the open-upvalue case
passed because the test's "dropped" closure was still bound to a local. The closed-upvalue
case passed because `"a" + "b"` appends into the literal `"a"`'s buffer, which is always
rooted through the constant pool. Each now has a test where the object is reachable *only*
through the thing being tested.

The same effect quietly disabled a v1 test: the `programs` case written to catch a missing
globals root builds its string with `"a" + "b"`, so since the append optimization it no
longer can. `arrays` and `closures` still catch it, which is why the experiment is rerun
after every change to the heap rather than trusted from the first time.

## Benchmarks

`./scripts/bench.sh`: best of 5 runs, AMD Ryzen 7 5800HS, Linux, release build.

| Benchmark | lumen | CPython 3.13.5 | lumen vs CPython | lumen instructions |
|---|---|---|---|---|
| fib | 0.116s | 0.114s | 1.02x slower | 1147M |
| loop | 0.476s | 0.609s | 1.28x faster | 5072M |
| strings | 0.063s | 0.072s | 1.14x faster | 710M |

| Program | What it measures |
|---|---|
| `fib` | `fib(30)`: about 2.7 million recursive calls |
| `loop` | 10 million iterations of arithmetic on locals |
| `strings` | 1,000,000 appends of one character to a growing string |

Wall-clock time moves by about ±5% between builds from code layout alone, so the last
column, instructions executed as counted by `perf stat`, is what to compare when judging a
change to the VM. It barely changes between runs. That's how a real 7–12% slowdown from
adding arrays and closures was found after wall-clock numbers had hidden part of it: see
[`docs/decisions/0001`](docs/decisions/0001-call-path-regression-from-closures.md).

### Optimizations in v2, chosen by profiling

`perf` showed the VM spending most of its time decoding and dispatching instructions, not
executing them ([`docs/profiling.md`](docs/profiling.md)). Two changes, measured in
instructions executed (millions):

| Benchmark | v1 | v2 before | decode once ([0002](docs/decisions/0002-predecode-bytecode.md)) | + compare-and-branch ([0003](docs/decisions/0003-compare-and-branch.md)) | vs v1 |
|---|---|---|---|---|---|
| fib | 1477 | 1539 | 1255 | 1147 | −22% |
| loop | 6933 | 7033 | 5473 | 5072 | −27% |
| strings | 856 | 893 | 750 | 709 | −17% |

"v2 before" is v2 with arrays and closures, after fixing the regression they caused.
In wall-clock time against v1: fib 0.132s → 0.116s, loop 0.579s → 0.476s, strings
0.075s → 0.063s.

### Strings

Strings used to be the weak spot. `s = s + "x"` copied the whole string every time, which is
O(n²): at 100,000 appends Lumen took 0.246 s to CPython's 0.018 s, 13.7x slower. CPython
avoids the copy by checking that the old string has no other references and resizing it in
place. Lumen has no reference counts to check, so it does this instead:

- A string value is a *view*, a buffer handle plus a length. Buffers only ever grow at the end.
- `a + b` appends `b` to `a`'s buffer in place when `a` ends exactly where the buffer ends,
  meaning nothing has extended that buffer yet. Every other view still sees only its own
  prefix, so strings stay immutable: after `let t = s; s = s + "x";`, `t` is unchanged.
- If another value already grew the buffer (`let b = a + "y"; let c = a + "z";`), there's no
  room to append, so it falls back to copying.

Appends are now amortized O(1): the 100,000-append run takes 0.009 s. The trade-off is that a
short view keeps its whole buffer alive.

## Testing

- **Tests** (`tests/lang.rs`): about 190 cases in 21 tests, covering every opcode, precedence
  and associativity, scoping, closures at one, two and three levels of nesting, arrays and
  aliasing, cycles, and every runtime and syntax error path. Every program runs twice,
  normally and under `--gc-stress`, and both must agree.
- **Fuzzing** (`examples/fuzz.rs`): feeds random token soup, including array and closure
  fragments, and mutated example programs through the whole pipeline with debug assertions
  on. Programs run under a step budget (`vm::run_limited`, which counts loop iterations and
  calls), because a mutated `fib(9223372036854775807)` is a valid program that would run for
  longer than the universe has existed. An earlier 10-minute run sat on one such input for
  minutes, which looked like a hang. A 10-minute run (1,388,256 inputs) finds no panics,
  and CI runs one minute of it on every push.
- **Deep nesting**: the parser caps nesting at 128 levels, so input like 100,000 `(`s, a
  100,000-term `1 + 1 + …` or a 100,000-long `a[0][0]…` is an error, not a Rust stack
  overflow.

## Design decisions

- [0001](docs/decisions/0001-call-path-regression-from-closures.md): the regression from
  arrays and closures, and measuring with instruction counts instead of wall-clock time
- [0002](docs/decisions/0002-predecode-bytecode.md): decode bytecode once, before running
- [0003](docs/decisions/0003-compare-and-branch.md): compare-and-branch for `if` and `while`

## Not yet

Structs, a REPL, string escapes, and a standard library beyond `len` and `push`.
