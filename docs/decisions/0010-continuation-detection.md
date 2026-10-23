# 0010: The parser decides when the REPL needs another line

**Status:** accepted.

## The problem

When the REPL reads `if x > 3 {` it has to wait for more lines before running anything.
The obvious way is to count braces: keep reading while there are more `{` than `}`. That's
wrong as soon as braces appear where they aren't syntax:

```
print "a { \" b";    // complete: the { is inside a string, and so is the \"
print "}{";          // complete
// a comment with a {
```

Counting correctly would mean re-implementing the lexer's rules for strings, escapes and
comments inside the REPL, and keeping the two in sync forever.

## The decision

The parser already knows. Each syntax error records whether it happened at the end of the
input (`at_end`), and `parse` reports `incomplete: true` when the *first* error did:

| Input | First error | Incomplete? |
|---|---|---|
| `if x > 3 {` | expected `}`, got end of file | yes: show `... ` and read another line |
| `print 1 +` | expected expression, got end of file | yes |
| `print "a { \" b";` | none | complete, runs |
| `if true { let = 1;` | expected identifier, got `=` | no: report it, even though `}` is also missing |

Only the first error counts: once something is wrong mid-input, more lines can't fix it.

## Escapes

A string with escapes is decoded entirely by the lexer before the parser runs, so a `\"`
or a `{` inside a string can't look like syntax to the parser either. A test runs
`print "a { \" b";` through the REPL and expects it to run immediately.

## Getting out of a continuation

A blank line at the `... ` prompt gives up and reports whatever error there is, so a
forgotten `;` can't trap you. End of input does the same.

## Cost

One `bool` on each parse error.
