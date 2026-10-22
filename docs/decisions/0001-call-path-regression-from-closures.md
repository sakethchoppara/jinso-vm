# 0001: Call-path regression from closures

**Status:** accepted for now, to be revisited in the week 5 profiling pass.

## What changed

Adding closures put more work on every call and return, including calls to functions
that capture nothing:

- `Frame` grew from `(return_ip, base)` to `(return_ip, base, closure)`.
- `Call` matches the callee against `Fn`, `Closure` and `Native` instead of only `Fn`.
- `Return` calls `OpenUpvalues::close`, which checks for upvalues to close even when
  there are none.

## Measurements

`./scripts/bench.sh 10` (best of 10, AMD Ryzen 7 5800HS):

| Benchmark | v1 | after arrays | after closures | after merging the native call path |
|---|---|---|---|---|
| fib | 0.132s | 0.135s | 0.148s | 0.140s (+6%) |
| loop | 0.579s | 0.599s | 0.619s | 0.566s to 0.601s |
| strings | 0.075s | 0.076s | 0.079s | 0.075s to 0.078s |

The first closure build checked the callee twice on every call: once in a guard for
built-ins, then again for functions and closures. Merging those into one `match` brought
loop and strings back to v1.

## Noise

Rebuilding after an unrelated change moved `loop` between 0.566s and 0.601s. Code layout
alone moves these benchmarks by about ±5%, the same size as the PRD's regression budget.
Marking `OpenUpvalues::close` `#[inline]` made no measurable difference, so it was reverted.

## Decision

Accept fib at about +6% for now. Which of the three costs above dominates is a guess
without a profiler. The week 5 pass uses `perf` to find out, and should compare several
builds rather than trusting a single one.
