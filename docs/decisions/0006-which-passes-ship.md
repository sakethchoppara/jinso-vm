# 0006: Which optimizer passes ship

**Status:** all six ship. One record for all of them, with a section per pass, so their
numbers sit side by side.

## Method

Each pass is measured by turning just that pass off (`--no-PASS`) and comparing with
`-O1`. Numbers are millions of instructions executed (`perf stat`). `branchy` was added
for this decision: a loop with a constant subexpression and an if/else chain at the end of
its body, the shape the PRD's passes are written for. The original three benchmarks
(fib, loop, strings) have no constants to fold and no jump chains to thread.

| Configuration | fib | loop | strings | branchy |
|---|---|---|---|---|
| `-O0` | 1128 | 4992 | 703 | 1217 |
| `-O1` | 1077 | 4542 | 672 | 858 |
| `--no-fold` | 1077 | 4542 | 673 | 1063 |
| `--no-cond` | 1077 | 4542 | 673 | 857 |
| `--no-thread` | 1077 | 4542 | 673 | 868 |
| `--no-deadjump` | 1077 | 4542 | 673 | 857 |
| `--no-unreachable` | 1077 | 4542 | 672 | 857 |
| `--no-fuse` | 1128 | 4992 | 704 | 961 |

## fold: ships

−205M on branchy (`60 * 60 * 24 % 1000` evaluated once at compile time instead of every
iteration). Nothing on the others. See [0005](0005-refusing-to-fold-errors.md) for what it
refuses to fold.

## cond: ships

Nothing on the four benchmarks, which have no literal conditions. On a `while true` loop
it removes the condition's test from every iteration:

| Program | `-O0` | `--no-cond` | `-O1` |
|---|---|---|---|
| `while true { i = i + 1; if i == 10000000 { return i; } }` | 3832M | 3612M | 2992M (−17%) |

## thread: ships

−10M on branchy, where each branch of the final if/else jumped to the loop's back edge and
now is a back edge. Zero elsewhere.

## deadjump and unreachable: ship, as a pipeline with cond

Neither changes a benchmark. They exist for what `cond` leaves behind: `if false { ... }`
becomes `Goto L; <dead block>; L:`. `unreachable` deletes the block, which turns the
`Goto` into a jump to the next instruction, which `deadjump` deletes. Measured on a loop
with an `if false { print "debug"; }` in its body:

| `-O0` | `--no-cond` | `--no-unreachable` | `--no-deadjump` | `-O1` |
|---|---|---|---|---|
| 3302M | 3082M | 2622M | 2622M | 2462M |

Turning off either one costs 6.1%, because the jump only becomes dead once the block is
gone. `unreachable` also removes 10 of the 386 instructions across the example listings
(code after `return`, and the implicit `return nil` after a function's last `return`).

## fuse: ships, and is the pass that meets the 5% target

`Constant c; Add` becomes `AddConst c` (also Sub, Mul, Div, Mod). This is the pass aimed at
the hot loops, where `i + 1`, `n - 1` and `i % 7` are everywhere: −51M fib, −450M loop,
−32M strings, −103M branchy.

On the three original benchmarks, `-O1` versus the v3 baseline (1135M, 5012M, 706M) is
fib −5.1%, loop −9.4%, strings −4.8%: **−8.2% in total**, against a target of 5%. Without
`fuse`, `-O1` executes exactly as many instructions as `-O0` on those three: the PRD's
five passes find nothing to do there.

## The lesson: fewer instructions isn't always faster

Instruction counts were adopted in [0001](0001-call-path-regression-from-closures.md)
because they're stable between runs and builds. While adding `fuse`, they were also
misleading twice:

1. The first version gave each fused opcode its own arm in the VM's `match`, with its own
   copy of the error formatting and the string path. `-O0` (identical bytecode to v2)
   then ran 5% more instructions.
2. Moving the error formatting into one `#[cold]` function brought the instruction count
   back to v2's, but cycles rose by about a third: on loop, 1931M cycles against v2's
   1466M, 2.4 instructions per cycle instead of 3.4. Keeping the operands available for an
   out-of-line call apparently cost more on the hot path than the code it saved.

Bisecting with `perf stat -e instructions,cycles` on builds that differed by one change
found both. The fix was a single VM arm for all five fused opcodes: one type check, a small
`match` for the operation, and one copy each of the string path and the error message.

Cycles (minimum of 5 runs, millions), `-O1` against the v3 baseline:

| Benchmark | v3 baseline | `-O1` | Change |
|---|---|---|---|
| fib | 346 | 356 | +3% |
| loop | 1465 | 1333 | −9% |
| strings | 186 | 198 | +6% |
| total | 1997 | 1887 | −5.5% |

So the optimizer removes 8% of the work on these benchmarks, and the time saved is smaller
and uneven: loop gets faster, fib and strings stay within a few percent, either way. The
rule since: a change to the VM is judged on both instructions and cycles.
