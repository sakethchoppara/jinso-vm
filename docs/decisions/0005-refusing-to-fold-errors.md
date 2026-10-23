# 0005: Refusing to fold anything that fails

**Status:** accepted.

## The rule

The optimizer may only change how fast a program runs, never what it does. For constant
folding that means: if evaluating an operation at runtime would fail, don't evaluate it at
compile time either. Leave the instruction in place, so the VM fails exactly as it would
have, with the same message and the same line.

`fold` in `src/optimize.rs` uses the same checked operations as the VM (`checked_add`,
`checked_div`, ...) and treats `None` as "don't fold". So these all reach the VM unchanged:

| Expression | Why it isn't folded | Runtime error |
|---|---|---|
| `1 / 0`, `1 % 0` | division by zero | `division by zero` |
| `9223372036854775807 + 1` | overflow | `integer overflow` |
| `(-9223372036854775807 - 1) / -1` | overflow (i64::MIN / -1) | `integer overflow` |
| `-(-9223372036854775807 - 1)` | overflow | `integer overflow` |
| `1 + true`, `-true`, `not 1`, `1 < nil` | type error | the VM's type error |
| `if 1 { }` | not a bool | `condition must be a bool, got int` |

Only int, bool and nil literals fold. Strings don't (a v3 non-goal), and nothing involving a
variable or a call does: `f() + 1` keeps its `Call`, so side effects and their order are
untouched. `false and f()` drops `f()` only because short-circuiting already guarantees it
never runs, and the constant-condition pass sees exactly that jump structure.

## Lines

A folded constant takes the operator's line, the line the VM would have reported if the
operation had failed. Folding a subexpression doesn't move the line of what's left:
in `print a /\n (2 - 2);` the `(2 - 2)` becomes `0`, and the division on line 2 still fails
at runtime with `line 2: division by zero`.

## Why not report these as compile errors instead?

Because the program might never reach them. `if false { print 1 / 0; }` runs fine; an
error raised at compile time would reject a valid program, and an error raised only when
the optimizer is on would make `-O0` and `-O1` disagree.

## How it's checked

Every test program runs at `-O0` and `-O1`, and the output and error text, including the
line, must match. `tests/optimize.rs` has one case per row of the table above, each also
checking that the instruction is still in the optimized listing.
