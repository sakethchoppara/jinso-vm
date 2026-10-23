# 0007: Differential testing: -O0 is the specification for -O1

**Status:** accepted.

## The argument

The unoptimized compiler is simple enough to trust: it emits one instruction sequence per
syntax node, and it's what every v1 and v2 test was written against. So the optimizer
needs no separate definition of "correct": for every program, `-O1` must print exactly what
`-O0` prints, and fail exactly how `-O0` fails, with the same message and line.

The reference is `-O0` on a VM with superinstructions turned off
([0012](0012-superinstructions.md)), so the VM's own rewrites are checked the same way as
the optimizer's.

## Where it's checked

- **Every test.** `tests/common::run` runs each program four ways (`-O0` and `-O1`, each
  normally and under `--gc-stress`) and asserts all four agree. That covers every test in
  `tests/lang.rs`, `tests/escapes.rs` and `tests/optimize.rs`, so the optimizer is tested by
  every test, not only its own.
- **The fuzzer.** Every random input is compiled at both levels (the compile errors must
  match) and, if it has no `while`, run at both levels under a step budget. A difference
  saves the input to `fuzz-mismatch.jo` and fails the run. A 3-minute run checked 1,658,642
  inputs.
- **`jinso run --verify`**, for checking one program by hand.

## What it can't catch

- **Bugs both levels share.** A VM bug, or a compiler bug upstream of the optimizer, makes
  both levels wrong the same way. The ordinary tests' expected outputs cover those.
- **Programs that loop forever.** The fuzzer doesn't execute programs containing `while`
  (a loop can double a string 30 times and allocate gigabytes first), and runs that hit the
  step budget are inconclusive. Threading can turn a `Jump` into a `Loop`, so which line a
  "step limit reached" error names can legitimately differ.
- **Performance.** Agreeing output says nothing about speed; see
  [0006](0006-which-passes-ship.md) for a pass that made the VM slower while every test
  passed.
- **Programs the inputs never produce.** Differential testing only covers what the tests
  and the fuzzer generate.
