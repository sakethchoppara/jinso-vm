# 0001: Regression from arrays and closures, and how it was found

**Status:** resolved. Every benchmark is within the 5% budget of v1, measured by
instruction count. The decode tables were later replaced by decoding the whole program
once before running: see [0002](0002-predecode-bytecode.md).

## Symptom

After arrays and closures landed, wall-clock times looked like fib +12%, loop +7% and
strings +5%. One change, merging a duplicated callee check in `Call`, appeared to bring
loop and strings back to v1, and an `#[inline]` appeared to do nothing.

Both conclusions were wrong. Rebuilding after an unrelated change moved `loop` between
0.566s and 0.601s: code layout alone moves wall-clock time by about ±5%, as much as the
budget itself.

## Measuring without the noise

`perf stat -e instructions` counts instructions executed. Three runs of fib differed by
under 0.01%, while cycles varied by 6%. Building v1 (`202c231`) and the arrays commit
(`5a4e9db`) in worktrees gave the real picture, in millions of instructions:

| Benchmark | v1 | + arrays | + closures |
|---|---|---|---|
| fib | 1477 | 1542 | 1655 (+12%) |
| loop | 6933 | 7233 | 7413 (+7%) |
| strings | 856 | 898 | 925 (+8%) |

The regression was real on all three, including loop, which never calls a function.

## Cause

`perf record` on fib with line tables (`CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`):

- Most samples were in decoding, not in executing: `Op::ALL[code[ip]]` (two bounds
  checks) and `op.operand_len()`, a `match` that grew with each opcode arrays and closures
  added. That explains why loop slowed down too: every instruction pays for decoding.
- `OpenUpvalues::close` took 3.7% of fib as a real function call on every `Return`,
  although fib never captures anything.

## Changes, one at a time, by instructions (millions)

| Change | fib | loop | strings |
|---|---|---|---|
| Before | 1654 | 7412 | 925 |
| `#[inline]` on `OpenUpvalues::close` | 1625 | 7443 | 928 |
| `OPERAND_LEN`: a 256-entry table replaces the `operand_len` match | 1572 | 7123 | 899 |
| `DECODE`: a 256-entry opcode table, padded with `Op::Bad` | 1539 | 7033 | 893 |
| **vs v1** | **+4.2%** | **+1.4%** | **+4.3%** |

Both tables are indexed by a `u8`, so with exactly 256 entries the compiler drops the
bounds check, and decoding becomes two loads. `Op::Bad` is never emitted by the compiler;
the VM treats it as unreachable.

The remaining fib cost compared with v1 is the bigger call frame (it also saves the
caller's closure) and the extra callee kinds `Call` checks for. Removing it would mean
splitting the call path for plain functions and closures; it's within budget, so it's left.

## Decision

- Keep all three changes.
- `scripts/bench.sh` now reports instruction counts alongside wall-clock time when `perf`
  can count events. Judge a code change by the instruction column; wall-clock time is for
  comparing with CPython.
