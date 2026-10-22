# 0002: Decode bytecode once, before running

**Status:** accepted.

## Problem

After the decode tables in [0001](0001-call-path-regression-from-closures.md), decoding was
still the largest cost in the profile. For every instruction the main loop:

- read the opcode byte and looked it up (`DECODE[byte]`),
- looked up how many operand bytes follow it (`OPERAND_LEN[byte]`),
- for most opcodes, read a two-byte big-endian operand with two more bounds checks.

In loop, those lines plus the dispatch `match` took about 45% of samples.

## Change

Before running, `vm::predecode` turns `Chunk::code` into a `Vec<Instr>`, one entry per
byte offset:

```rust
struct Instr { op: Op, arg: u16, next: u32 }
```

Only offsets where an instruction starts are filled; the others hold `Op::Bad` and are
never reached. Because the table is indexed by byte offset, `ip`, jump offsets and
function addresses mean the same thing as before, so neither the compiler nor the
disassembler changed, and `Chunk::code` stays the `Vec<u8>` bytecode from v1. Fetching an
instruction is now one 8-byte load.

The `DECODE` and `OPERAND_LEN` tables from 0001 only served the old loop, so they were
removed; `predecode` uses `Op::ALL` and `operand_len` once per instruction.

Cost: 8 bytes of memory per bytecode byte, built once per run. fib's bytecode is 70 bytes.

## Result

Instructions executed (`perf stat`, millions):

| Benchmark | before | after | change | vs v1 |
|---|---|---|---|---|
| fib | 1539 | 1255 | −18.5% | −15% |
| loop | 7033 | 5473 | −22.2% | −21% |
| strings | 893 | 750 | −16.0% | −12% |
