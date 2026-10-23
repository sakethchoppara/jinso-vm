# 0012: Superinstructions, fused while decoding

**Status:** accepted.

## What was fused, and why those

A temporary build counted which pairs of instructions execute back to back on each
benchmark. Only pairs that sit next to each other in the bytecode can be fused (a `Loop`
followed by the loop's first instruction can't). The three fusions cover the hottest ones:

| Superinstruction | Replaces | Where it shows up |
|---|---|---|
| `GetLocal2 a b` | `GetLocal a; GetLocal b` | 18% of executed pairs in primes, 8% in loop |
| `AddConstToLocal x k` | `GetLocal x; AddConst k; SetLocal x` | `i = i + 1` in every loop; `s = s + "x"` (20% in strings) |
| `JumpIfNot<cmp>Const k` | `Constant k; JumpIfNot<cmp>` | `while i < N`, fib's `if n < 2` (about 10%) |

## Where: in the VM's decoder, not the optimizer

`vm::fuse` runs after the VM decodes the bytecode, and only rewrites the decoded copy:
the bytecode format, compiler, optimizer and disassembler are untouched, and the fused
opcodes never appear in a `Chunk`. Because decoded instructions are indexed by byte
offset, fusing only replaces the *first* instruction's entry; the entries for the
instructions it covers stay as they were. That gives two properties for free:

- A jump into the middle of a fused sequence still works: it lands on the plain entry.
  (A loop's back edge jumping to the second half of a `GetLocal2` happens in practice.)
- A superinstruction never needs its own slow path. Its fast path handles ints; for
  anything else (strings, overflow, wrong types) it does the first instruction's work and
  sets `ip` to the second one's plain entry, so the plain instructions run and report any
  error with the usual message and line.

## Testing it

Before this, differential testing compared `-O0` with `-O1`. Superinstructions are applied
at both levels, so a fusion bug that broke both the same way would have passed. The
reference is now `-O0` on a VM with fusion turned off (`vm::run_reference`): every test,
`--verify` and the fuzzer compare against it. `superinstructions_and_their_fallbacks`
reaches every fast path and every fallback, including the jump into a fused pair.

## Results

Instructions and minimum cycles over 5 runs, in millions:

| Benchmark | Before | All three | Cycles |
|---|---|---|---|
| primes | 1603 / 551 | 1380 / 527 | −4% |
| fib | 1069 / 356 | 1054 / 351 | −1% |
| loop | 4342 / 1328 | 3342 / 1207 | −9% |
| strings | 664 / 199 | 580 / 190 | −5% |
| branchy | 843 / 236 | 632 / 198 | −16% |

Each one pays its way: removing it alone costs, in millions of cycles,

| Removed | primes | fib | loop | strings | branchy |
|---|---|---|---|---|---|
| `AddConstToLocal` | +36 | 0 | +95 | +13 | +48 |
| `GetLocal2` | +26 | 0 | +66 | 0 | 0 |
| compare with a constant | 0 | +26 | +23 | +3 | +10 |
