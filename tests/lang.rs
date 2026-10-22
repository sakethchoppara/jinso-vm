const FIB: &str = "fn fib(n) {\n  if n < 2 { return n; }\n  return fib(n - 1) + fib(n - 2);\n}\nprint fib(20);";

fn run_with(src: &str, gc_stress: bool) -> Result<(String, lumen::gc::Stats), String> {
    let chunk = lumen::compile(src)?;
    let mut out = Vec::new();
    let stats = lumen::vm::run(&chunk, &mut out, gc_stress)?;
    Ok((String::from_utf8(out).unwrap(), stats))
}

/// Runs every program twice, normally and with a collection before every allocation;
/// both must agree. A missing GC root shows up as a "dangling handle" panic under stress.
fn run(src: &str) -> Result<String, String> {
    let normal = run_with(src, false).map(|(out, _)| out);
    let stressed = run_with(src, true).map(|(out, _)| out);
    assert_eq!(normal, stressed, "--gc-stress changed the result of {src:?}");
    normal
}

fn ast(expr: &str) -> String {
    lumen::parse(&format!("print {expr};")).unwrap()[0].to_string()
}

#[test]
fn precedence_and_associativity() {
    for (src, want) in [
        ("1 + 2 * 3", "(+ 1 (* 2 3))"),
        ("(1 + 2) * 3", "(* (+ 1 2) 3)"),
        ("10 - 3 - 2", "(- (- 10 3) 2)"),
        ("8 / 4 % 3", "(% (/ 8 4) 3)"),
        ("-2 * 3", "(* (- 2) 3)"),
        ("- - 1", "(- (- 1))"),
        ("1 + 2 < 4 == true", "(== (< (+ 1 2) 4) true)"),
        ("not 1 < 2", "(not (< 1 2))"),
        ("a or b and c", "(or a (and b c))"),
        ("not a and b", "(and (not a) b)"),
        ("a == b or c < d", "(or (== a b) (< c d))"),
        ("-f(1) * 2", "(* (- (call f 1)) 2)"),
        ("f(a, g(b))(c)", "(call (call f a (call g b)) c)"),
    ] {
        assert_eq!(ast(src), format!("(print {want})"), "{src}");
    }
}

#[test]
fn programs() {
    for (src, want) in [
        ("print 1 + 2 * 3;", "7\n"),
        ("print 7 / 2; print -7 / 2; print 7 % 3; print -7 % 3;", "3\n-3\n1\n-1\n"),
        ("print 1 < 2; print 2 <= 2; print 3 > 4; print 3 >= 4;", "true\ntrue\nfalse\nfalse\n"),
        ("print 1 == 1; print 1 != 1; print true == 1; print nil == nil;", "true\nfalse\nfalse\ntrue\n"),
        ("print not true; print nil;", "false\nnil\n"),
        ("let x = 2; let y = x * 10; x = y + 1; print x; print y;", "21\n20\n"),
        ("let x = 1; let x = x + 1; print x;", "2\n"),
        ("// comment\nprint 1; // trailing\n", "1\n"),
        // control flow
        ("if 1 < 2 { print 1; } else { print 2; }", "1\n"),
        ("if false { print 1; } print 2;", "2\n"),
        ("let x = 5; if x < 3 { print 1; } else if x < 10 { print 2; } else { print 3; }", "2\n"),
        ("let x = 50; if x < 3 { print 1; } else if x < 10 { print 2; } else { print 3; }", "3\n"),
        ("let i = 0; let s = 0; while i < 10 { s = s + i; i = i + 1; } print s;", "45\n"),
        ("while false { print 1; } print 2;", "2\n"),
        // and/or short-circuit: the 1/0 would error if evaluated
        ("print true and false; print false or true;", "false\ntrue\n"),
        ("print false and 1 / 0 == 0; print true or 1 / 0 == 0;", "false\ntrue\n"),
        ("print true and 1 < 2; print false or 2 < 1;", "true\nfalse\n"),
        // block-scoped locals
        ("let x = 1; if true { let x = 2; print x; x = 3; print x; } print x;", "2\n3\n1\n"),
        ("if true { let a = 1; if true { let b = a + 1; print b; } print a; }", "2\n1\n"),
        ("if true { let a = 1; let a = a + 1; print a; }", "2\n"),
        // a local declared inside a loop body must be popped every iteration
        ("let i = 0; while i < 3 { let j = 0; while j < i { j = j + 1; } print j; i = i + 1; }", "0\n1\n2\n"),
        ("let n = 0; if true { let i = 0; while i < 1000 { let t = i * 2; n = n + t; i = i + 1; } } print n;", "999000\n"),
        // functions
        (FIB, "6765\n"),
        ("fn add(a, b) { return a + b; } print add(add(1, 2), add(3, 4));", "10\n"),
        ("fn fact(n) { if n <= 1 { return 1; } return n * fact(n - 1); } print fact(20);", "2432902008176640000\n"),
        ("fn f() { } print f(); fn g() { return; } print g();", "nil\nnil\n"),
        ("fn p(x) { print x; } p(1); p(2);", "1\n2\n"),
        ("fn f(n) { return n; } print f; let g = f; print g(7);", "<fn f>\n7\n"),
        // mutual recursion: is_odd is a global defined after is_even
        ("fn is_even(n) { if n == 0 { return true; } return is_odd(n - 1); }
          fn is_odd(n) { if n == 0 { return false; } return is_even(n - 1); }
          print is_even(10); print is_odd(7);", "true\ntrue\n"),
        // return from inside nested blocks and a loop; locals beyond params
        ("fn find(limit) { let i = 0; while true { if i * i > limit { let r = i; return r; } i = i + 1; } }
          print find(50);", "8\n"),
        ("fn f(x) { let x = x * 2; if true { let x = x + 1; return x; } } print f(5);", "11\n"),
        // calls from inside a top-level block: frame base must sit above the block's locals
        ("fn sq(n) { return n * n; } if true { let a = 3; let b = sq(a) + a; print b; }", "12\n"),
        ("fn sum(n) { if n == 0 { return 0; } return n + sum(n - 1); } print sum(3000);", "4501500\n"),
        // strings
        ("print \"hello\";", "hello\n"),
        ("let a = \"foo\"; let b = a + \"bar\"; print b; print a;", "foobar\nfoo\n"),
        ("print \"ab\" == \"a\" + \"b\"; print \"a\" != \"b\"; print \"1\" == 1;", "true\ntrue\nfalse\n"),
        ("let s = \"\"; let i = 0; while i < 5 { s = s + \"ab\"; i = i + 1; } print s;", "ababababab\n"),
        // GC roots: strings held in a global, in a caller's locals, in arguments and in
        // block locals must all survive collections triggered deeper in the call stack.
        ("fn join(a, b) { let sep = \"-\"; return a + sep + b; }
          fn twice(s) { let t = join(s, s); return join(t, t); }
          let g = \"x\" + \"y\";
          if true { let local = twice(g); print local; print g; }", "xy-xy-xy-xy\nxy\n"),
        // g is reachable only through the globals table while h is allocated
        ("let g = \"a\" + \"b\"; let h = \"c\" + \"d\"; print g + h;", "abcd\n"),
        // same, but a block local reachable only through the stack
        ("if true { let l = \"a\" + \"b\"; let m = \"c\" + \"d\"; print l + m; }", "abcd\n"),
        ("fn f(n) { if n == 0 { return \"\"; } let mine = \"<\" + \">\"; return mine + f(n - 1) + mine; } print f(3);",
         "<><><><><><>\n"),
    ] {
        assert_eq!(run(src).as_deref(), Ok(want), "{src}");
    }
}

#[test]
fn runtime_errors() {
    for (src, want) in [
        ("print 1 / 0;", "line 1: division by zero"),
        ("print 1 % 0;", "line 1: division by zero"),
        ("print 1 + true;", "line 1: operands must be two ints or two strs, got int and bool"),
        ("print 1 < nil;", "operands must be ints"),
        ("print -true;", "operand of '-' must be an int"),
        ("print not 1;", "operand of 'not' must be a bool"),
        ("\nprint x;", "line 2: undefined variable 'x'"),
        ("y = 1;", "undefined variable 'y'"),
        ("print 9223372036854775807 + 1;", "integer overflow"),
        ("print (-9223372036854775807 - 1) / -1;", "integer overflow"),
        ("if 1 { print 1; }", "line 1: condition must be a bool, got int"),
        ("let x = nil;\nwhile x { }", "line 2: condition must be a bool, got nil"),
        ("print 1 and true;", "condition must be a bool"),
        ("if true { let a = 1; } print a;", "undefined variable 'a'"),
        ("fn f(n) { return f(n + 1); }\nf(0);", "line 1: stack overflow"),
        ("fn f(a, b) { }\nf(1);", "line 2: f() takes 2 argument(s), got 1"),
        ("let x = 1; x();", "can only call functions, got int"),
        ("fn f() { return 1 / 0; }\n\nf();", "line 1: division by zero"),
        ("fn f() { return g(); }\nf();", "undefined variable 'g'"),
        ("print \"a\" + 1;", "operands must be two ints or two strs, got str and int"),
        ("print \"a\" - \"b\";", "operands must be ints, got str and str"),
        ("print \"a\" < \"b\";", "operands must be ints"),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: got {err:?}");
    }
}

#[test]
fn syntax_errors() {
    for (src, want) in [
        ("print 1 +;", "1:10: expected expression"),
        ("print 1", "expected ';'"),
        ("let 1 = 2;", "expected identifier"),
        ("print \"oops", "1:7: unterminated string"),
        ("print 99999999999999999999;", "too large"),
        ("print @;", "1:7: unexpected character '@'"),
        ("if true { print 1 }", "expected ';', got '}'"),
        ("while true print 1;", "expected '{'"),
        ("if true { print 1; ", "expected '}'"),
        ("return 1;", "line 1: return outside function"),
        ("if true { fn f() { } }", "functions can only be declared at top level"),
        ("fn f(a, a) { }", "duplicate parameter 'a'"),
        ("fn f(a b) { }", "expected ')'"),
        ("print f(1,);", "expected expression"),
        ("print 1 +;", "expected expression, got ';'"),
        ("let x = 1 let", "expected ';', got 'let'"),
        ("print", "expected expression, got end of file"),
        ("print x y;", "expected ';', got identifier 'y'"),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: got {err:?}");
    }
}

#[test]
fn reports_multiple_syntax_errors() {
    let err = run("let = 1;\nprint 2\nprint (;\nprint 3;").unwrap_err();
    assert_eq!(err.lines().count(), 3, "{err}");

    // recovery inside a block must not produce a spurious error at its `}`
    let err = run("if true {\n  let = 1;\n  print 2;\n}\nprint (;").unwrap_err();
    assert_eq!(err.lines().count(), 2, "{err}");
}

#[test]
fn disassembler_shows_jump_targets() {
    let chunk = lumen::compile("let i = 0;\nwhile i < 3 {\n  i = i + 1;\n}").unwrap();
    let want = "\
0000    1 Constant        0  (0)
0003    | DefineGlobal    0  (i)
0006    2 GetGlobal       0  (i)
0009    | Constant        1  (3)
0012    | Lt           
0013    | JumpIfFalse    13  (-> 0029)
0016    3 GetGlobal       0  (i)
0019    | Constant        2  (1)
0022    | Add          
0023    | SetGlobal       0  (i)
0026    2 Loop           23  (-> 0006)
";
    assert_eq!(lumen::disasm::disassemble(&chunk), want);
}

#[test]
fn disassembler_marks_functions() {
    let dis = lumen::disasm::disassemble(&lumen::compile(FIB).unwrap());
    assert!(dis.contains("== fn fib =="), "{dis}");
    assert!(dis.contains("(<fn fib>)"), "{dis}");
    assert!(dis.contains("Call            1"), "{dis}");
}

const GARBAGE_LOOP: &str = "let s = \"\"; let i = 0;
while i < 3000 { s = s + \"x\"; i = i + 1; }
print s == s;";

#[test]
fn gc_frees_unreachable_strings() {
    // ~4.5MB of intermediate strings, each unreachable once s moves on.
    let (out, stats) = run_with(GARBAGE_LOOP, false).unwrap();
    assert_eq!(out, "true\n");
    assert!(stats.collections >= 1, "{stats:?}");
    assert!(stats.freed > 1000, "{stats:?}");
    // Survivors: the literals, s, and at most the garbage made since the last collection.
    assert!(stats.live < 3000, "{stats:?}");
}

#[test]
fn gc_stress_collects_on_every_allocation() {
    let (_, stats) = run_with(GARBAGE_LOOP, true).unwrap();
    assert_eq!(stats.collections, 3000, "{stats:?}");
    // After the final collection only the 3 literals and s survive; then one more alloc.
    assert!(stats.live <= 5, "{stats:?}");
}

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    for src in [
        format!("print {}1{};", "(".repeat(100_000), ")".repeat(100_000)),
        format!("print {}1;", "-".repeat(100_000)),
        format!("print {}true;", "not ".repeat(100_000)),
        format!("print 1{};", " + 1".repeat(100_000)),
        format!("{}{}", "if true { ".repeat(100_000), "}".repeat(100_000)),
        format!("if false {{ }}{} {{ }}", " else if false { }".repeat(100_000)),
    ] {
        let err = run(&src).unwrap_err();
        assert!(err.contains("nesting too deep"), "{}", &err[..err.len().min(200)]);
    }
    // just under the limit still works
    assert_eq!(run(&format!("print {}1{};", "(".repeat(100), ")".repeat(100))).unwrap(), "1\n");
    assert_eq!(run(&format!("print 0{};", " + 1".repeat(100))).unwrap(), "100\n");
}
