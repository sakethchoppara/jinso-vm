# Profiling Jinso

## How

```sh
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release --target-dir /tmp/prof
perf record -o fib.data /tmp/prof/release/jinso run bench/fib.jn
perf report -i fib.data --no-children --sort symbol    # by function
perf report -i fib.data --no-children --sort srcline   # by source line
perf stat -e instructions ./target/release/jinso run bench/fib.jn   # noise-free count
```

Line tables are only for profiling builds; the shipped release profile is unchanged.
`perf` needs `kernel.perf_event_paranoid` at 1 or lower.

## Where fib spends its time

By function, almost everything is one symbol, because the whole interpreter loop is
inlined into `vm::run`:

| Self time | Function |
|---|---|
| 95.6% | `jinso::vm::run` |
| 3.7% | `jinso::upvalues::OpenUpvalues::close` (before it was inlined) |
| <0.2% each | kernel: scheduling, page faults, timers |

So the useful breakdown is by source line inside `vm::run`. This one is from after the
decode tables of [decision 0001](decisions/0001-call-path-regression-from-closures.md),
and it picked the next two optimizations:

| Self time | Line | What it is |
|---|---|---|
| 10.0% | `ip += 1 + OPERAND_LEN[byte]` | advancing past the instruction |
| 8.6% | slice indexing (`index.rs`) | reading bytecode and operands |
| 6.4% | `match op` | the jump to the instruction's code |
| 5.2% | `DECODE[byte]` | decoding the opcode |
| 4.5% | `Vec` push and pop (`mod.rs`) | the value stack |

Fetching, decoding and dispatching together took about a quarter of fib's time, and the
value stack about another tenth. In loop, decoding plus dispatch was about 45%. That chose
the two optimizations:

1. [0002](decisions/0002-predecode-bytecode.md): decode the bytecode once before running,
   so fetching an instruction is one load. fib −18.5%, loop −22%, strings −16%.
2. [0003](decisions/0003-compare-and-branch.md): one compare-and-branch instruction for
   `if a < b` and `while a < b` instead of `Lt` + `JumpIfFalse`. fib −8.6%, loop −7.3%,
   strings −5.5%.

## After both

| Self time | Line | What it is |
|---|---|---|
| 12.6% | `match op` | the jump to the instruction's code |
| 12.2% | `let Instr { op, arg, next } = code[ip]` | fetching the decoded instruction |
| 12.2% | slice indexing (`index.rs`) | the bounds check on that fetch, and stack slots |
| 6.1% | `Vec` push (`mod.rs`) | the value stack |
| 3.6% | `GetLocal` | reading a local onto the stack |

Fetch and dispatch are now a single load and a jump per instruction, so the remaining
costs are the number of instructions and the value stack. Next candidates: instructions
that combine common sequences (`GetLocal` + `Constant` + `Sub`), or a value stack that
doesn't check capacity on every push.
