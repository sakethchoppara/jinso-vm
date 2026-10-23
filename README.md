# Jinso

[![CI](https://github.com/sakethchoppara/jinso-vm/actions/workflows/ci.yml/badge.svg)](https://github.com/sakethchoppara/jinso-vm/actions/workflows/ci.yml)

**[Try it in your browser →](https://sakethchoppara.github.io/jinso-vm/)**

A small scripting language implemented from scratch in Rust: a lexer, a recursive-descent
parser, a compiler with a bytecode optimizer, a stack-based virtual machine with closures
and arrays, and a mark-and-sweep garbage collector. No dependencies.

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
./target/release/jinso run examples/closures.jo          # run a program
./target/release/jinso dis examples/closures.jo          # show its bytecode
./target/release/jinso ast examples/arrays.jo            # show its syntax tree
./target/release/jinso run --gc-stress examples/strings.jo   # collect garbage on every allocation
./target/release/jinso run -O0 examples/fib.jo           # without the optimizer (-O1 is the default)
./target/release/jinso run --verify examples/fib.jo      # run at -O0 and -O1, fail if they differ
./target/release/jinso dis --diff bench/branchy.jo       # unoptimized and optimized side by side
./target/release/jinso repl                              # interactive session

cargo test                          # ~277 cases, each run at -O0 and -O1, normally and under --gc-stress
./scripts/root_removal.sh           # break the GC on purpose, 8 ways; each must fail a test
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
| Strings | `"..."` with escapes `\n \t \r \\ \" \0` and `\u{1F600}` (1 to 6 hex digits) |
| Comments | `// to end of line` |

Jinso is dynamically typed with strict checks: `1 + true`, `if 1 { }` and `not 5` are
runtime errors, not silent conversions. Every error carries a source location and the VM
never panics on user input:

```
3:8: expected expression, got ';'
line 4: operands must be ints, got str and int
line 2: index 5 out of bounds for array of length 3
line 1: stack overflow (more than 4096 nested calls)
```

Syntax errors don't stop at the first one: the parser recovers at the next statement and
reports them all. A bad escape is reported at its backslash (`1:10: unknown escape sequence
'\q'`), and `\u{D800}`, values above `\u{10FFFF}` and an empty `\u{}` are errors too.
Escapes are decoded by the lexer, so the disassembler and syntax tree show string constants
in escaped form and a multi-line string doesn't break the listing.

## Architecture

```
source ──▶ lexer ──▶ parser ──▶ AST ──▶ compiler ──▶ instructions ──▶ optimizer ──▶ Chunk ──▶ VM
         lexer.rs   parser.rs  ast.rs  compiler.rs   with labels    optimize.rs   chunk.rs  vm.rs
                                                                                              │
                                                     gc.rs: strings, arrays, closures ◀───────┤
                                                     upvalues.rs: open upvalue list   ◀───────┘
```

| File | Role |
|---|---|
| `lexer.rs` | Source to tokens with line and column; decodes string escapes |
| `parser.rs` | Recursive descent for statements, precedence climbing for expressions, error recovery |
| `ast.rs` | Syntax tree, printable as S-expressions (`jinso ast`) |
| `compiler.rs` | AST to an instruction list whose jumps name labels; resolves every name to a local slot, an upvalue or a global |
| `optimize.rs` | The optimizer's passes, and `relocate`, which lays the instruction list out as bytecode |
| `chunk.rs` | Opcodes, the `Vec<u8>` bytecode, constant pool, line table, function and upvalue descriptors |
| `vm.rs` | Decodes the bytecode once, then runs it: value stack, call frames, built-ins; a `Vm` can outlive one run |
| `repl.rs` | `jinso repl`: the line loop, continuation prompts and `:` commands |
| `gc.rs` | Heap of strings, arrays, closures and upvalues; mark-and-sweep with an explicit work stack |
| `upvalues.rs` | The open upvalues, sorted by stack slot, and closing them |
| `disasm.rs` | Bytecode listing (`jinso dis`) |
| `wasm.rs` | Exports for the browser playground |

### What `fib` compiles to

```
0000    1 Jump           39  (-> 0042)      skip over the function body
== fn fib ==
0003    2 GetLocal        0                 n is stack slot 0 of this frame
0006    | Constant        0  (2)
0009    | JumpIfNotLt     4  (-> 0016)      `if n < 2`: compare and branch in one instruction
0012    | GetLocal        0
0015    | Return
0016    3 GetGlobal       0  (fib)
0019    | GetLocal        0
0022    | SubConst        1  (1)            `n - 1`: Constant 1 and Sub, fused by the optimizer
0025    | Call            1
0028    | GetGlobal       0  (fib)
0031    | GetLocal        0
0034    | SubConst        2  (2)
0037    | Call            1
0040    | Add
0041    | Return                            (the unreachable `return nil` after it is gone)
0042    1 Constant        4  (<fn fib>)
0045    | DefineGlobal    0  (fib)
...
```

### How the pieces work

**Precedence.** Each infix operator has a binding power. `expr(min)` only consumes operators
at least as strong as `min` and parses the right operand with `p + 1`, which makes
`10 - 3 - 2` group as `(10 - 3) - 2`. `not` binds looser than comparisons, as in Python, so
`not a < b` means `not (a < b)`.

**Jumps and labels.** When the compiler reaches `if`, it doesn't yet know how long the body
will be. It emits a jump to a fresh label, compiles the body, then places the label. The
compiler's output is a list of instructions and labels, not bytes: `relocate` lays it out
at the end, computing every jump offset and function address in one pass. An unconditional
jump becomes `Jump` if its label is ahead and `Loop` if it's behind. (Through v2, jumps were
emitted with a placeholder offset and backpatched; that broke as soon as an optimizer needed
to remove an instruction: [0004](docs/decisions/0004-symbolic-jumps-and-relocation.md).)
`and`/`or` compile to the same jumps, so `false and 1/0 == 0` never divides.

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

`get` and `set` must see the *same* `v`, during the call and after it. Jinso does what Lua
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
fails if any bug goes unnoticed, if a mutant doesn't compile, or if a patch no longer
applies to the current code (so a refactor can't silently turn the experiment off). CI runs
it on every push. Current results:

| Removed | Tests that fail |
|---|---|
| value stack as a root | `programs`, `arrays`, `closures`, `unreachable_cycles_are_collected`, a REPL test |
| globals as a root | `arrays`, `closures`, `closures_and_arrays_live_across_inputs` (REPL) |
| constant pool as a root | `programs`, `arrays`, `closures`, both `gc_*` tests, an optimizer test |
| array elements | `arrays`, `closures` |
| open upvalues as a root | `closures` |
| a closure's upvalues | `closures`, `closures_and_arrays_live_across_inputs` |
| a closed upvalue's value | `closures` |
| closing open upvalues when the REPL recovers from an error | `a_closure_survives_an_error_in_the_call_that_made_it` |

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

## The optimizer

`-O1`, the default, runs six passes over the instruction list until none of them changes
anything (at most 10 rounds), then `fuse` once at the end. Each can be turned off with
`--no-PASS`, and `-O0` turns them all off.

| Pass | What it does | Example |
|---|---|---|
| `fold` | evaluates operations on int, bool and nil literals | `2 * 3 + 1` → `Constant 7` |
| `cond` | resolves branches on literal conditions | `if false { A }` → jump over A; `while true` loses its test |
| `thread` | a jump to a jump goes straight to the final target | an if/else ending a loop body jumps to the loop start |
| `unreachable` | drops code after `return` or a jump, up to a label something jumps to | the implicit `return nil` after a last `return` |
| `deadjump` | drops a jump to the very next instruction | what `cond` and `unreachable` leave of `if false { ... }` |
| `fuse` | `Constant c` followed by `Add`/`Sub`/`Mul`/`Div`/`Mod` becomes one `AddConst c` | `i + 1`, `n - 1`, `i % 7` in every hot loop |

**It never changes what a program does.** Anything that would fail at runtime (overflow,
division by zero, a type error) is left for the VM, so it still fails with the same message
on the same line; calls are never folded, so side effects and their order are untouched;
and every surviving instruction keeps its source line
([0005](docs/decisions/0005-refusing-to-fold-errors.md)). This is checked by differential
testing: `-O0` is the specification for `-O1`
([0007](docs/decisions/0007-differential-testing.md)). Every test runs at both levels and
must agree on output and errors, and so does every fuzzer input.

**What it's worth**, in millions of instructions executed (`perf stat`), measured by
turning one pass off at a time ([0006](docs/decisions/0006-which-passes-ship.md)):

| | fib | loop | strings | branchy |
|---|---|---|---|---|
| `-O0` | 1119 | 4772 | 695 | 1193 |
| `-O1` | **1070** | **4352** | **663** | **842** |
| without `fold` | 1070 | 4352 | 664 | 1048 |
| without `thread` | 1070 | 4352 | 664 | 852 |
| without `fuse` | 1119 | 4772 | 695 | 940 |

`cond`, `unreachable` and `deadjump` don't change these four, which have no literal
conditions. Where they apply they matter: `cond` cuts a `while true` loop by 18%, and in a
loop containing an `if false { ... }` debug block, `cond` saves 21% and `unreachable` and
`deadjump` another 6% (each is needed: the jump over the block only becomes dead once the
block is gone).

Against the v3 baseline (fib 1135M, loop 5012M, strings 706M), `-O1` runs **11.2% fewer
instructions** on the three original benchmarks (fib −5.7%, loop −13.2%, strings −6.1%).
Most of the optimizer's share is `fuse`: the PRD's other passes find nothing to fold or
thread there, so a fourth benchmark, `branchy`, was added to measure them; on it `-O1` runs
29% fewer instructions than `-O0`.

**Fewer instructions isn't always faster.** The first version of `fuse` cut instructions
but made the VM's main loop slower in cycles for every program, optimized or not: five new
`match` arms, each with its own copy of the error formatting, cost about 20%, and moving the
formatting into one cold function cost even more. Merging them into one arm fixed it, and
building with one codegen unit stopped unrelated code from changing how the main loop is
inlined. In cycles (minimum of 5 runs), `-O1` against the v3 baseline is now: fib −1%,
loop −10%, strings +6%, **6.7% fewer in total**; branchy runs 38% fewer cycles than at
`-O0`. So the optimizer clearly removes work, its effect on time is smaller and uneven, and
VM changes are now judged on cycles as well as instructions.

Optimizing costs about 0.3 ms on a 1,330-line program (every example and benchmark
concatenated ten times): 0.45 ms to compile at `-O1` against 0.15 ms at `-O0`.

## The REPL

```
$ jinso repl
> let x = 5;
> print x * 2;
10
> fn double(n) { return n * 2; }
> print double(x);
10
> print 1 / 0;
line 1: division by zero
> print x;
5
> while x > 3 {
...   x = x - 1;
... }
> :dis
0050    1 GetGlobal       0  (x)
0053    | Constant        7  (3)
0056    | JumpIfNotGt    12  (-> 0071)
0059    2 GetGlobal       0  (x)
0062    | SubConst        8  (1)
0065    | SetGlobal       0  (x)
0068    1 Loop           21  (-> 0050)
```

Every input is compiled onto the end of the same program and run in the same VM, so
globals, functions, closures and arrays from earlier inputs stay alive. A runtime error
resets the stack and closes any open upvalues, but keeps the heap and the globals; a syntax
or compile error adds nothing at all ([0009](docs/decisions/0009-repl-state-model.md)).
Whether to show `... ` is the parser's call: it reports when its first error was hitting the
end of the input, so `{` or `\"` inside a string never confuses it
([0010](docs/decisions/0010-continuation-detection.md)). A blank line at `... ` gives up and
reports the error. `:dis` shows the last input's bytecode, `:reset` starts over, `:quit`
exits. It reads plain lines, so it works piped (`printf 'print 1;' | jinso repl`), which is
how its tests drive it.

## Benchmarks

`./scripts/bench.sh`: best of 10 runs, AMD Ryzen 7 5800HS, Linux, release build, `-O1`.

| Benchmark | jinso | CPython 3.13.5 | jinso vs CPython | jinso instructions |
|---|---|---|---|---|
| branchy | 0.075s | 0.121s | 1.61x faster | 842M |
| fib | 0.112s | 0.114s | 1.02x faster | 1070M |
| loop | 0.418s | 0.611s | 1.46x faster | 4352M |
| primes | 0.197s | 0.177s | 1.11x slower | 1767M |
| strings | 0.063s | 0.070s | 1.11x faster | 664M |

| Program | What it measures |
|---|---|
| `branchy` | 1 million iterations with a constant subexpression and an if/else chain |
| `fib` | `fib(30)`: about 2.7 million recursive calls |
| `loop` | 10 million iterations of arithmetic on locals |
| `primes` | counting the 148,933 primes below 2 million with an odd-only Sieve of Eratosthenes |
| `strings` | 1,000,000 appends of one character to a growing string |

`primes` is the one CPython wins, and the profile says why: it's the only benchmark built on
arrays, and array access is Jinso's least optimized path. Each `composite[j] = true` goes
through bounds and type checks in functions the compiler doesn't inline (`element`,
`Heap::array_mut`: about 10% of the run), and building the million-entry array calls the
`push` built-in a million times (about 5%). CPython's list indexing is a tight C path. The
Python version is a line-for-line match with `while` loops; idiomatic Python would hand the
sieve to C with slice assignment (`composite[p*p//2::p] = ...`), which is a different
implementation rather than the same algorithm.

Wall-clock time moves by about ±5% between builds from code layout alone. Instructions
executed, the last column, barely changes between runs, which is how a real 7–12% slowdown
from adding arrays and closures was found after wall-clock numbers had hidden part of it
([0001](docs/decisions/0001-call-path-regression-from-closures.md)). But instructions alone
can also mislead (see the optimizer section above), so changes to the VM are checked with
`perf stat -e instructions,cycles`.

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
O(n²): at 100,000 appends Jinso took 0.246 s to CPython's 0.018 s, 13.7x slower. CPython
avoids the copy by checking that the old string has no other references and resizing it in
place. Jinso has no reference counts to check, so it does this instead:

- A string value is a *view*, a buffer handle plus a length. Buffers only ever grow at the end.
- `a + b` appends `b` to `a`'s buffer in place when `a` ends exactly where the buffer ends,
  meaning nothing has extended that buffer yet. Every other view still sees only its own
  prefix, so strings stay immutable: after `let t = s; s = s + "x";`, `t` is unchanged.
- If another value already grew the buffer (`let b = a + "y"; let c = a + "z";`), there's no
  room to append, so it falls back to copying.

Appends are now amortized O(1): the 100,000-append run takes 0.009 s. The trade-off is that a
short view keeps its whole buffer alive.

## Testing

- **Tests**: about 277 cases in 60 tests. `tests/lang.rs` covers every opcode, precedence
  and associativity, scoping, closures at one, two and three levels of nesting, arrays and
  aliasing, cycles, and every runtime and syntax error path; `tests/escapes.rs` every escape
  and escape error with its position; `tests/optimize.rs` what each pass must and must not
  do, line numbers after folding, jump threading in nested loops, and the optimizer's cost;
  `tests/repl.rs` piped sessions: multi-line blocks, recovery from runtime, syntax and
  compile errors, closures across inputs, and 100 inputs of globals under `--gc-stress`.
  Every program and session runs four ways, at `-O0` and `-O1`, each normally and under
  `--gc-stress`, and all four must agree.
- **Fuzzing** (`examples/fuzz.rs`): feeds random token soup, including array, closure and
  escape fragments, and mutated example programs through the whole pipeline with debug
  assertions on, at both `-O0` and `-O1`; any difference between them fails the run. Programs run under a step budget (`vm::run_limited`, which counts loop iterations and
  calls), because a mutated `fib(9223372036854775807)` is a valid program that would run for
  longer than the universe has existed. An earlier 10-minute run sat on one such input for
  minutes, which looked like a hang. The latest 10-minute run checked 6,014,556 inputs at
  both levels: no panics, and no difference between `-O0` and `-O1`. CI runs one minute of
  it on every push.
- **Deep nesting**: the parser caps nesting at 128 levels, so input like 100,000 `(`s, a
  100,000-term `1 + 1 + …` or a 100,000-long `a[0][0]…` is an error, not a Rust stack
  overflow.

## Design decisions

- [0001](docs/decisions/0001-call-path-regression-from-closures.md): the regression from
  arrays and closures, and measuring with instruction counts instead of wall-clock time
- [0002](docs/decisions/0002-predecode-bytecode.md): decode bytecode once, before running
- [0003](docs/decisions/0003-compare-and-branch.md): compare-and-branch for `if` and `while`
- [0004](docs/decisions/0004-symbolic-jumps-and-relocation.md): labels and relocation instead of backpatching
- [0005](docs/decisions/0005-refusing-to-fold-errors.md): the optimizer never folds anything that fails
- [0006](docs/decisions/0006-which-passes-ship.md): which passes ship, and fewer instructions vs fewer cycles
- [0007](docs/decisions/0007-differential-testing.md): differential testing, and what it can't catch
- [0008](docs/decisions/0008-escape-decoding-in-the-lexer.md): escapes are decoded in the lexer
- [0009](docs/decisions/0009-repl-state-model.md): what the REPL keeps between inputs, and what an error resets
- [0010](docs/decisions/0010-continuation-detection.md): the parser, not a brace count, decides when to read another line

## Not yet

Structs, modules, line editing and history in the REPL, and a standard library beyond `len`
and `push`.
