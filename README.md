# Lumen

[![CI](https://github.com/sakethchoppara/lumen-vm/actions/workflows/ci.yml/badge.svg)](https://github.com/sakethchoppara/lumen-vm/actions/workflows/ci.yml)

**[Try it in your browser →](https://sakethchoppara.github.io/lumen-vm/)**

A small scripting language implemented from scratch in Rust: a lexer, a recursive-descent
parser, a compiler that emits bytecode, a stack-based virtual machine, and a mark-and-sweep
garbage collector. No dependencies.

```
fn fib(n) {
  if n < 2 { return n; }
  return fib(n - 1) + fib(n - 2);
}
print fib(30);   // 832040, in about the same time as CPython 3.13
```

## Quick start

```sh
cargo build --release
./target/release/lumen run examples/fib.lm              # run a program
./target/release/lumen dis examples/fib.lm              # show its bytecode
./target/release/lumen ast examples/basics.lm           # show its syntax tree
./target/release/lumen run --gc-stress examples/strings.lm   # collect garbage on every allocation

cargo test                          # 100+ cases, each run normally and under --gc-stress
./scripts/bench.sh                  # benchmark against CPython
cargo run --example fuzz -- 600     # 10 minutes of random input

./scripts/build-web.sh && python3 -m http.server -d web   # playground at localhost:8000
```

## Playground

`web/` is a static page that runs the same compiler and VM, built for
`wasm32-unknown-unknown`, with Output, Bytecode and Syntax tree tabs. It uses no
wasm-bindgen. `src/wasm.rs` exports three plain functions (`alloc`, `run`, `result_ptr`),
and about 20 lines of JS copy the source in and the results out. The VM runs in a Web
Worker, so a program that never ends is stopped after 5 seconds instead of freezing the
tab. GitHub Actions rebuilds it and deploys it to Pages on every push to `main`.

## The language

| | |
|---|---|
| Types | `int` (64-bit, overflow is an error), `bool`, `str`, `nil`, functions |
| Operators | `+ - * / %`, `== != < <= > >=`, `and or not` (short-circuiting), `-x` |
| Statements | `let x = e;`, `x = e;`, `print e;`, `if c { } else if c { } else { }`, `while c { }` |
| Functions | `fn name(a, b) { return a + b; }`, recursion, functions as values |
| Scoping | top-level variables are globals; variables declared in a block are locals |
| Comments | `// to end of line` |

Lumen is dynamically typed with strict checks: `1 + true`, `if 1 { }` and `not 5` are
runtime errors, not silent conversions. Every error carries a source location and the VM
never panics on user input:

```
3:8: expected expression, got ';'
line 4: operands must be ints, got str and int
line 1: stack overflow (more than 4096 nested calls)
```

Syntax errors don't stop at the first one: the parser recovers at the next statement and
reports them all.

## Architecture

```
source ──▶ lexer ──▶ tokens ──▶ parser ──▶ AST ──▶ compiler ──▶ Chunk ──▶ VM
           lexer.rs             parser.rs  ast.rs  compiler.rs  chunk.rs  vm.rs
                                                                          │
                                                       gc.rs (heap) ◀─────┘
```

| File | Role |
|---|---|
| `lexer.rs` | Source to tokens with line and column |
| `parser.rs` | Recursive descent for statements, precedence climbing for expressions, error recovery |
| `ast.rs` | Syntax tree, printable as S-expressions (`lumen ast`) |
| `compiler.rs` | AST to bytecode: resolves variables to slots, backpatches jumps |
| `chunk.rs` | Opcodes, the `Vec<u8>` bytecode, constant pool, line table |
| `vm.rs` | Fetch-decode-execute loop, value stack, call frames |
| `gc.rs` | String heap, mark-and-sweep collector |
| `disasm.rs` | Bytecode listing (`lumen dis`) |
| `wasm.rs` | Exports for the browser playground |

### What `fib` compiles to

```
0000    1 Jump           46  (-> 0049)      skip over the function body
== fn fib ==
0003    2 GetLocal        0                 n is stack slot 0 of this frame
0006    | Constant        0  (2)
0009    | Lt
0010    | JumpIfFalse     4  (-> 0017)      backpatched once the `if` body was emitted
0013    | GetLocal        0
0016    | Return
0017    3 GetGlobal       0  (fib)
0020    | GetLocal        0
0023    | Constant        1  (1)
0026    | Sub
0027    | Call            1
0030    | GetGlobal       0  (fib)
0033    | GetLocal        0
0036    | Constant        2  (2)
0039    | Sub
0040    | Call            1
0043    | Add
0044    | Return
0045    1 Constant        3  (nil)          implicit `return nil;`
0048    | Return
0049    | Constant        4  (<fn fib>)
0052    | DefineGlobal    0  (fib)
...
```

### How the pieces work

**Precedence.** Each infix operator has a binding power. `expr(min)` only consumes operators
at least as strong as `min` and parses the right operand with `p + 1`, which makes
`10 - 3 - 2` group as `(10 - 3) - 2`. `not` binds looser than comparisons, as in Python, so
`not a < b` means `not (a < b)`.

**Jump patching.** When the compiler reaches `if`, it doesn't yet know how long the body
will be. It emits `JumpIfFalse 0xFFFF`, remembers the operand's position, compiles the body,
then writes the real offset back. `while` jumps backwards with `Loop`, whose offset is known
immediately. `and`/`or` compile to the same jumps, so `false and 1/0 == 0` never divides.

**Variables.** Nothing is looked up by name at runtime. Globals are resolved to indices at
compile time. A global's slot stays "undefined" until its `let` runs, so functions can call
functions defined below them, and reading a global too early is still a clean runtime error.
Locals live directly on the value stack. Declaring one emits no code at all, because the
initializer's value is already sitting in the right slot. Leaving a block pops its locals.

**Calls.** The caller pushes the function and its arguments, then `Call argc`. The new
frame's base points at the first argument, so parameters are just the callee's first local
slots. `Return` truncates the stack back to the callee and leaves the return value there.
A frame is only `(return_ip, base)`.

**Garbage collection.** Strings live in a heap of slots. Values hold a slot index, not a
pointer, so a collector bug shows up as a "dangling handle" panic, never memory corruption.
The roots are:

- the value stack, which holds every frame's arguments and locals,
- the globals,
- the constant pool, since string literals are loaded into the heap at startup.

A collection runs when live bytes pass a threshold, and the threshold then doubles. With
`--gc-stress` it runs before *every* allocation and freed slots are never reused, so a
missing root crashes immediately instead of reading a recycled object. The whole test
suite runs in both modes and must produce identical output. Each root was removed
deliberately to confirm the suite catches it.

## Benchmarks

`./scripts/bench.sh`: best of 5 runs, AMD Ryzen 7 5800HS, Linux, release build.

| Benchmark | lumen | CPython 3.13.5 | lumen vs CPython |
|---|---|---|---|
| fib | 0.127s | 0.112s | 1.13x slower |
| loop | 0.588s | 0.616s | 1.05x faster |
| strings | 0.246s | 0.018s | 13.67x slower |

| Program | What it measures |
|---|---|
| `fib` | `fib(30)`: about 2.7 million recursive calls |
| `loop` | 10 million iterations of arithmetic on locals |
| `strings` | 100,000 appends of one character to a growing string |

On calls and arithmetic, Lumen runs about level with CPython 3.13. CPython has years of
tuning, including the specializing interpreter added in 3.11. Lumen pays for checking every
integer operation for overflow and for 16-byte tagged values.

On strings Lumen is far behind, for a known reason: `s = s + "x"` copies the whole string
every time, which is O(n²). CPython spots that the old string has no other references and
appends in place. Lumen's collector doesn't track reference counts, so it can't use that trick.

## Testing

- **Unit tests** (`tests/lang.rs`): over 100 cases covering every opcode, precedence and
  associativity, scoping, recursion, every runtime and syntax error path, and garbage
  collection. Every program runs twice, normally and under `--gc-stress`.
- **Fuzzing** (`examples/fuzz.rs`): feeds random token soup and mutated example programs
  through the whole pipeline with debug assertions on. A 10-minute run (283,968 inputs)
  finds no panics, and
  CI runs one minute of it on every push.
- **Deep nesting**: the parser caps nesting at 128 levels, so input like 100,000 `(`s or a
  100,000-term `1 + 1 + …` is an error, not a Rust stack overflow. Tests cover each shape.

## Not in v1

Closures, arrays, structs, a REPL, string escapes, and a standard library.
