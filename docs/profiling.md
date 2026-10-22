# Profiling Lumen

## How

```sh
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release --target-dir /tmp/prof
perf record -o fib.data /tmp/prof/release/lumen run bench/fib.lm
perf report -i fib.data --no-children --sort symbol    # by function
perf report -i fib.data --no-children --sort srcline   # by source line
perf stat -e instructions ./target/release/lumen run bench/fib.lm   # noise-free count
```

Line tables are only for profiling builds; the shipped release profile is unchanged.
`perf` needs `kernel.perf_event_paranoid` at 1 or lower.

## Where fib spends its time

By function, almost everything is one symbol, because the whole interpreter loop is
inlined into `vm::run`:

| Self time | Function |
|---|---|
| 95.6% | `lumen::vm::run` |
| 3.7% | `lumen::upvalues::OpenUpvalues::close` (before it was inlined) |
| <0.2% each | kernel: scheduling, page faults, timers |

So the useful breakdown is by source line inside `vm::run`, after the decode tables
(see `docs/decisions/0001-call-path-regression-from-closures.md`):

| Self time | Line | What it is |
|---|---|---|
| 10.0% | `ip += 1 + OPERAND_LEN[byte]` | advancing past the instruction |
| 8.6% | slice indexing (`index.rs`) | reading bytecode and operands |
| 6.4% | `match op` | the jump to the instruction's code |
| 5.2% | `DECODE[byte]` | decoding the opcode |
| 4.5% | `Vec` push and pop (`mod.rs`) | the value stack |

Fetching, decoding and dispatching together take about a quarter of fib's time, and the
value stack about another tenth. Further gains would come from changing that design, for
example pre-decoding bytecode into a `Vec<Op>` with operands inline, or adding fused
instructions for common pairs like `GetLocal` + `Constant` + `Sub`.
