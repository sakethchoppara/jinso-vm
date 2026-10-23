mod common;
use common::*;

#[test]
fn each_escape() {
    for (src, want) in [
        (r#"print "line one\nline two";"#, "line one\nline two\n"),
        (r#"print "say \"hi\"";"#, "say \"hi\"\n"),
        (r#"print "tab\there";"#, "tab\there\n"),
        (r#"print "cr\r";"#, "cr\r\n"),
        (r#"print "back\\slash";"#, "back\\slash\n"),
        (r#"print "nul\0byte";"#, "nul\0byte\n"),
        // \u{...} with 1, 2, 5 and 6 hex digits, either case
        (r#"print "\u{41}\u{a}\u{1F600}\u{0000e9}";"#, "A\n\u{1F600}\u{e9}\n"),
        (r#"print "\u{10FFFF}" == "\u{10ffff}";"#, "true\n"),
        // escapes are decoded in the lexer, so the value is the decoded one
        (r#"print len("a\nb"); print len("\u{1F600}"); print "\"" == "\u{22}";"#, "3\n1\ntrue\n"),
        (r#"let s = "x"; s = s + "\t"; s = s + "\\"; print s;"#, "x\t\\\n"),
        // inside arrays, strings print in escaped form
        (r#"print ["a\nb", "q\"", "\u{1F600}"];"#, "[\"a\\nb\", \"q\\\"\", \"\u{1F600}\"]\n"),
        // a real newline inside a string is still allowed, and line numbers keep counting
        ("print \"two\nlines\";\nprint 1 / 0;", "two\nlines\n"),
    ] {
        match run(src) {
            Ok(out) => assert_eq!(out, want, "{src}"),
            Err(e) => assert!(e.contains("line 3: division by zero") && want == "two\nlines\n", "{src}: {e}"),
        }
    }
}

#[test]
fn escape_errors_point_at_the_backslash() {
    for (src, want) in [
        (r#"print "ab\q";"#, "1:10: unknown escape sequence '\\q'"),
        ("print \"ok\";\nprint \"x\\q\";", "2:9: unknown escape sequence '\\q'"),
        (r#"print "\x41";"#, "1:8: unknown escape sequence '\\x'"),
        (r#"print "\u{D800}";"#, "1:8: \\u{D800} is not a valid Unicode scalar value"),
        (r#"print "\u{dfff}";"#, "\\u{dfff} is not a valid Unicode scalar value"),
        (r#"print "\u{110000}";"#, "1:8: \\u{110000} is not a valid Unicode scalar value"),
        (r#"print "\u{}";"#, "1:8: \\u{} needs 1 to 6 hex digits"),
        (r#"print "\u{1234567}";"#, "\\u{1234567} needs 1 to 6 hex digits"),
        (r#"print "a\u41";"#, "1:9: expected '{' after \\u"),
        (r#"print "\u{zz}";"#, "1:8: \\u{...} needs 1 to 6 hex digits and a closing '}'"),
        (r#"print "\u{41";"#, "\\u{...} needs 1 to 6 hex digits and a closing '}'"),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: got {err:?}");
    }
}

#[test]
fn unterminated_strings_point_at_the_opening_quote() {
    for (src, want) in [
        (r#"print "abc\"#, "1:7: unterminated string"),
        (r#"print "abc\";"#, "1:7: unterminated string"),
        (r#"print "\u{41"#, "1:7: unterminated string"),
        (r#"print "\u"#, "1:7: unterminated string"),
        ("print 1;\n  print \"never closed", "2:9: unterminated string"),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: got {err:?}");
    }
}

#[test]
fn escaped_constants_print_escaped_in_dis_and_ast() {
    let chunk = jinso::compile(r#"print "line one\nline two"; print "say \"hi\"\t\\";"#).unwrap();
    let dis = jinso::disasm::disassemble(&chunk);
    assert!(dis.contains(r#"("line one\nline two")"#), "{dis}");
    assert!(dis.contains(r#"("say \"hi\"\t\\")"#), "{dis}");
    assert_eq!(dis.lines().count(), 4, "a multi-line string must not break the listing:\n{dis}");
    assert_eq!(ast(r#""say \"hi\"\n""#), r#"(print "say \"hi\"\n")"#);
}
