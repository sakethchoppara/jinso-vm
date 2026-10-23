# 0011: Fast array access

**Status:** accepted.

## Problem

`bench/primes` (an odd-only Sieve of Eratosthenes counting the primes below 2 million) was
the first benchmark CPython won: 0.197s against 0.177s. It's the only one built on arrays,
and the profile showed why: every `composite[j] = true` called out-of-line functions for its
bounds and type checks (`element`, `Heap::array_mut`, about 10% of the run), and building
the million-entry array called the `push` built-in a million times (about 5%).

## Changes, measured one at a time

Instructions and minimum cycles over 5 runs, in millions:

| Change | primes instr | primes cycles | other benchmarks |
|---|---|---|---|
| Before | 1767 | 620 | |
| 1. `#[inline]` on `element`, `Heap::obj`, `array`, `array_mut`, `push` | 1722 | 566 (−8.7%) | unchanged |
| 2. `Index`/`SetIndex` check the common case inline; errors explained by a cold `index_error` | 1603 | 551 (−2.6%) | within 1% |
| 3. Call `push` directly instead of through the built-in table, so it inlines | 1608 | 560 (+1.6%) | within 1% |

Changes 1 and 2 ship. Change 3 doesn't: both ways of writing it (comparing function
pointers, then dispatching on the built-in's index) made primes slower. Inlining `push`
into the loop grew the loop more than it saved; the call through the table is cheap
compared with what `push` does.

The fast path in change 2 does no formatting: it matches an array and an int, then uses
`get`/`get_mut`, and only on failure calls `index_error`, which builds the same messages as
before (`index 5 out of bounds for array of length 3`, `array index must be an int, got
str`, `cannot index a value of type int`). That keeps error text out of the main loop, the
lesson from [0006](0006-which-passes-ship.md).

## Result

primes: 620M → 551M cycles (−11%), 1767M → 1603M instructions (−9%). Against CPython, best
of 10: 0.176s against 0.178s, from 1.11x slower to level.

What's left is the same as everywhere else: fetching and dispatching instructions. The
sieve's inner loop is 12 instructions per crossed-off entry; making it cheaper would mean
fusing common pairs (`GetLocal; GetLocal`), a broader change than array access.
