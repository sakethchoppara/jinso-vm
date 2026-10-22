# 0003: Compare-and-branch instructions for `if` and `while`

**Status:** accepted.

## Problem

`while i < n { ... }` compiled to `Lt` (pop two ints, push a bool) followed by
`JumpIfFalse` (pop the bool, check its type, maybe jump). Every loop iteration and every
`if n < 2` in fib paid for an extra dispatch, a push, a pop and a type check on a bool the
program never sees.

## Change

Four instructions, `JumpIfNotLt`, `JumpIfNotLe`, `JumpIfNotGt` and `JumpIfNotGe`, each with
a u16 forward offset: pop two ints and jump unless the comparison holds.
`Compiler::cond_jump` emits one when an `if` or `while` condition is a `<`, `<=`, `>` or
`>=` comparison; any other condition still compiles to the expression plus `JumpIfFalse`.

This is not a bytecode optimizer, which v2 lists as a non-goal: no pass rewrites
instructions after they're emitted. The compiler just picks a different instruction for one
syntactic shape, the same way it already compiles `<=` as `Gt` + `Not`.

Behavior is unchanged:

- Both operands are still evaluated, left first, before comparing. A test calls functions
  that print in both operands to check the order.
- Non-int operands give the same message as before, `operands must be ints, got X and Y`,
  with the condition's line.
- Comparisons used as values (`let b = a < c;`) still use `Lt`/`Gt`.

## Result

Instructions executed (`perf stat`, millions):

| Benchmark | before | after | change | vs v1 |
|---|---|---|---|---|
| fib | 1255 | 1147 | −8.6% | −22% |
| loop | 5473 | 5072 | −7.3% | −27% |
| strings | 750 | 709 | −5.5% | −17% |
