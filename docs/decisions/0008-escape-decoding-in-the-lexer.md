# 0008: Escapes are decoded in the lexer, and unknown escapes are errors

**Status:** accepted.

## Where

The lexer decodes `\n \t \r \\ \" \0 \u{...}` while scanning a string literal, so a
`Tok::Str` carries the string's real value. Nothing after the lexer knows escapes exist:
the parser, compiler, constant pool, heap and VM see plain strings.

Decoding later (in the parser or compiler) would have meant carrying the raw text and the
literal's position through the AST just to report a bad escape, and every consumer of a
string constant would have to remember whether it was decoded yet. The lexer already knows
the line and column of every character, so it's the one place that can point at the
backslash for free.

## Unknown escapes are errors

`"\q"` could mean the two characters `\q` (as in some shells) or be an error. It's an
error, because silently keeping it hides typos: `"C:\new"` becomes a newline either way,
and `"\x41"` would print literally instead of an `A`. An error at the backslash
(`1:10: unknown escape sequence '\q'`) is cheap to fix and never surprising, and turning it
into a feature later is backward compatible, while the reverse isn't.

The same reasoning covers `\u{...}`: surrogates (`\u{D800}`), values above `\u{10FFFF}`,
an empty `\u{}` and more than 6 digits are all errors, since none of them is a Unicode
scalar value and Rust strings can't hold them anyway.

## Positions

- A bad escape is reported at its backslash.
- Running out of input inside a string, including inside an escape (`"abc\`, `"\u{41`),
  is an unterminated string, reported at the opening quote, because that's where the
  problem starts.

## Display

The disassembler and syntax tree print string constants with Rust's `{:?}`, which escapes
`\n`, `\t`, `\"` and the rest in exactly Jinso's syntax. A multi-line string stays on one
line of the listing.
