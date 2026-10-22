const FIB: &str = "fn fib(n) {\n  if n < 2 { return n; }\n  return fib(n - 1) + fib(n - 2);\n}\nprint fib(20);";

fn run_with(src: &str, gc_stress: bool) -> Result<(String, jinso::gc::Stats), String> {
    let chunk = jinso::compile(src)?;
    let mut out = Vec::new();
    let stats = jinso::vm::run(&chunk, &mut out, gc_stress)?;
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
    jinso::parse(&format!("print {expr};")).unwrap()[0].to_string()
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
        // in-place append must not be visible through other values sharing the buffer
        ("let s = \"ab\"; let t = s; s = s + \"c\"; t = t + \"d\"; print s; print t;", "abc\nabd\n"),
        ("let a = \"x\"; let b = a + \"y\"; let c = a + \"z\"; print b; print c; print a;", "xy\nxz\nx\n"),
        ("fn f() { let s = \"\"; s = s + \"a\"; return s; } print f(); print f() + f();", "a\naa\n"),
        ("let s = \"ab\"; s = s + s; s = s + s; print s;", "abababab\n"),
        ("let s = \"ab\"; let t = s + \"c\"; print s == \"ab\"; print t == \"abc\"; print s == t;", "true\ntrue\nfalse\n"),
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
    let chunk = jinso::compile("let i = 0;\nwhile i < 3 {\n  i = i + 1;\n}").unwrap();
    let want = "\
0000    1 Constant        0  (0)
0003    | DefineGlobal    0  (i)
0006    2 GetGlobal       0  (i)
0009    | Constant        1  (3)
0012    | JumpIfNotLt    13  (-> 0028)
0015    3 GetGlobal       0  (i)
0018    | Constant        2  (1)
0021    | Add          
0022    | SetGlobal       0  (i)
0025    2 Loop           22  (-> 0006)
";
    assert_eq!(jinso::disasm::disassemble(&chunk), want);
}

#[test]
fn disassembler_marks_functions() {
    let dis = jinso::disasm::disassemble(&jinso::compile(FIB).unwrap());
    assert!(dis.contains("== fn fib =="), "{dis}");
    assert!(dis.contains("(<fn fib>)"), "{dis}");
    assert!(dis.contains("Call            1"), "{dis}");
}

// Prepending can't reuse a buffer, so every iteration makes a new string and the old
// one becomes garbage (~4.5MB in total).
const GARBAGE_LOOP: &str = "let s = \"\"; let i = 0;
while i < 3000 { s = \"y\" + s; i = i + 1; }
print s == s;";

#[test]
fn gc_frees_unreachable_strings() {
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
    assert!(stats.collections > 2900, "{stats:?}");
    // After the final collection only the 2 literals and s survive; then one more alloc.
    assert!(stats.live <= 4, "{stats:?}");
}

#[test]
fn appending_grows_one_buffer_in_place() {
    // s = s + "x" extends s's buffer: no new objects, so nothing to collect even under stress.
    let src = "let s = \"\"; let i = 0; while i < 3000 { s = s + \"x\"; i = i + 1; } print s == s;";
    let (out, stats) = run_with(src, true).unwrap();
    assert_eq!(out, "true\n");
    assert_eq!((stats.collections, stats.live), (0, 2), "{stats:?}");
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

// ---- v2: arrays ----

#[test]
fn arrays() {
    for (src, want) in [
        // the PRD's example
        ("let a = [1, 2, 3]; push(a, 4); print a[0] + a[3]; a[1] = 20; print len(a); print a;", "5\n4\n[1, 20, 3, 4]\n"),
        ("print []; print len([]); print [nil, true, -1];", "[]\n0\n[nil, true, -1]\n"),
        ("print len(\"héllo\"); print len(\"\");", "5\n0\n"),
        // reference semantics: two names, one array
        ("let a = [1]; let b = a; push(b, 2); b[0] = 9; print a;", "[9, 2]\n"),
        ("fn add(a, v) { push(a, v); } let a = []; add(a, 1); add(a, 2); print a;", "[1, 2]\n"),
        ("let a = [1]; let b = a; print a == b; print a == [1]; print [] == [];", "true\nfalse\nfalse\n"),
        // nested arrays, including assignment through two indexes
        ("let m = [[1, 2], [3, 4]]; m[1][0] = 30; print m; print m[1][0] + m[0][1];", "[[1, 2], [30, 4]]\n32\n"),
        ("let g = [[0, 0], [0, 0]]; let r = g[1]; r[1] = 5; print g;", "[[0, 0], [0, 5]]\n"),
        // strings print raw at the top level, quoted inside arrays
        ("let a = [\"x\", \"y\" + \"z\"]; print a; print a[1];", "[\"x\", \"yz\"]\nyz\n"),
        // functions in arrays, including the built-ins
        ("fn sq(n) { return n * n; } let fs = [sq, len]; print fs[0](4); print fs[1]([1, 2]); print fs;",
         "16\n2\n[<fn sq>, <native len>]\n"),
        // index expressions and evaluation order
        ("let a = [10, 20, 30]; let i = 1; print a[i + 1]; a[i] = a[i] + 1; print a;", "30\n[10, 21, 30]\n"),
        ("let a = []; let i = 0; while i < 5 { push(a, i * i); i = i + 1; } print a;", "[0, 1, 4, 9, 16]\n"),
        ("print push([], 1);", "nil\n"),
        ("fn f() { return [1, 2]; } print f()[1]; print len(f());", "2\n2\n"),
        // a cycle prints as [...] instead of recursing forever
        ("let a = [1]; push(a, a); print a; print len(a); print a[1][1][0];", "[1, [...]]\n2\n1\n"),
        ("let a = []; let b = [a]; push(a, b); print a; print b;", "[[[...]]]\n[[[...]]]\n"),
        // the built-ins are ordinary globals, so a program can shadow them
        ("let len = 3; print len;", "3\n"),
        ("fn push(a, b) { return a + b; } print push(1, 2);", "3\n"),
        // locals holding arrays inside blocks and functions
        ("if true { let a = [1]; let b = [a, a]; a[0] = 2; print b; }", "[[2], [2]]\n"),
        ("fn sum(a) { let t = 0; let i = 0; while i < len(a) { t = t + a[i]; i = i + 1; } return t; } print sum([1, 2, 3, 4]);", "10\n"),
        // GC: objects reachable only through array elements must survive collections
        ("let a = [\"a\" + \"b\"]; let x = \"c\" + \"d\"; print a[0] + x;", "abcd\n"),
        ("let a = [[[\"a\" + \"b\"]]]; let x = \"c\" + \"d\"; let y = [x]; print a[0][0][0];", "ab\n"),
        ("fn mk(n) { return [n, \"s\" + \"t\"]; } let a = mk(1); let b = mk(2); print a; print b;", "[1, \"st\"]\n[2, \"st\"]\n"),
        ("let a = []; let i = 0; while i < 50 { push(a, \"n\" + \"x\"); push(a, [i]); i = i + 1; } print len(a); print a[98]; print a[99];", "100\nnx\n[49]\n"),
        ("let keep = []; let i = 0; while i < 30 { let t = [i, [i]]; if i % 10 == 0 { push(keep, t); } i = i + 1; } print keep;",
         "[[0, [0]], [10, [10]], [20, [20]]]\n"),
    ] {
        assert_eq!(run(src).as_deref(), Ok(want), "{src}");
    }
}

#[test]
fn array_errors() {
    for (src, want) in [
        ("let a = [1, 2, 3];\nprint a[5];", "line 2: index 5 out of bounds for array of length 3"),
        ("print [1][-1];", "line 1: index -1 out of bounds for array of length 1"),
        ("let a = [1]; a[1] = 2;", "index 1 out of bounds for array of length 1"),
        ("print [][0];", "index 0 out of bounds for array of length 0"),
        ("print [1][\"0\"];", "line 1: array index must be an int, got str"),
        ("let a = [1]; a[true] = 2;", "array index must be an int, got bool"),
        ("let x = 5; print x[0];", "line 1: cannot index a value of type int"),
        ("let s = \"abc\"; s[0] = 1;", "cannot index a value of type str"),
        ("print nil[0];", "cannot index a value of type nil"),
        ("print len(5);", "line 1: len() expects an array or str, got int"),
        ("print len(nil);", "len() expects an array or str, got nil"),
        ("push(1, 2);", "push() expects an array, got int"),
        ("print len();", "len() takes 1 argument(s), got 0"),
        ("push([]);", "push() takes 2 argument(s), got 1"),
        ("print [1] + [2];", "operands must be two ints or two strs, got array and array"),
        ("print [1] < [2];", "operands must be ints, got array and array"),
        ("if [] { }", "condition must be a bool, got array"),
        ("let a = [1]; a();", "can only call functions, got array"),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: got {err:?}");
    }
}

#[test]
fn array_syntax() {
    for (src, want) in [
        ("[1, 2, [3]]", "(array 1 2 (array 3))"),
        ("a[0][1]", "(index (index a 0) 1)"),
        ("f(1)[0](2)", "(call (index (call f 1) 0) 2)"),
        ("-a[0] * 2", "(* (- (index a 0)) 2)"),
    ] {
        assert_eq!(ast(src), format!("(print {want})"), "{src}");
    }
    assert_eq!(jinso::parse("a[i + 1] = v;").unwrap()[0].to_string(), "(set-index a (+ i 1) v)");
    for (src, want) in [
        ("print [1, 2;", "expected ']'"),
        ("print a[];", "expected expression, got ']'"),
        ("print a[1;", "expected ']'"),
        ("[1] = 2;", "1:5: can only assign to a variable or an array element"),
        ("f() = 1;", "can only assign to a variable or an array element"),
        ("1 + x = 2;", "can only assign to a variable or an array element"),
        ("print @[;", "unexpected character '@'"),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: got {err:?}");
    }
    // long index chains count toward the nesting limit
    let err = run(&format!("let a = [1]; print a{};", "[0]".repeat(100_000))).unwrap_err();
    assert!(err.contains("nesting too deep"), "{err}");
}

#[test]
fn unreachable_cycles_are_collected() {
    // Each iteration makes a self-cycle and a two-array cycle, then drops them.
    let src = "let i = 0; while i < 2000 { let a = [i]; push(a, a); let b = []; let c = [b]; push(b, c); i = i + 1; } print i;";
    let (out, stats) = run_with(src, true).unwrap();
    assert_eq!(out, "2000\n");
    assert!(stats.freed > 5900, "{stats:?}");
    assert!(stats.live <= 4, "{stats:?}");
}

#[test]
fn marking_deep_nesting_uses_no_recursion() {
    // A 200,000-deep chain [[[[...]]]]: recursive marking would overflow the Rust stack
    // (tests run on 2 MB threads). The work stack stays tiny: the chain's next link plus
    // whatever other roots are waiting, independent of the depth.
    let src = "let a = []; let i = 0; while i < 200000 { a = [a]; i = i + 1; } print len(a); print a[0][0][0];";
    let (out, stats) = run_with(src, false).unwrap();
    assert_eq!(out, "1\n[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[...]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]\n");
    assert!(stats.collections >= 1, "{stats:?}");
    assert!(stats.peak_work <= 4, "{stats:?}");
}

// ---- v2: closures ----

const COUNTER: &str = "fn make_counter() {
  let count = 0;
  fn next() {
    count = count + 1;
    return count;
  }
  return next;
}
";

#[test]
fn closures() {
    let counter = |rest: &str| format!("{COUNTER}{rest}");
    for (src, want) in [
        // the PRD's examples: a closure outlives its call, and each call gets fresh variables
        (counter("let c = make_counter(); print c(); print c();"), "1\n2\n"),
        (counter("let a = make_counter(); let b = make_counter(); print a(); print a(); print b(); print a();"), "1\n2\n1\n3\n"),
        (counter("let a = make_counter(); let b = make_counter(); print a == b; print a == a; print a;"), "false\ntrue\n<fn next>\n"),
        // capturing a parameter
        ("fn adder(n) { fn add(x) { return x + n; } return add; } let add5 = adder(5); print add5(10); print adder(1)(1);".into(), "15\n2\n"),
        // two and three levels of nesting
        ("fn outer() { let x = \"x\"; fn middle() { fn inner() { return x + \"!\"; } return inner; } return middle; } print outer()()();".into(), "x!\n"),
        ("fn a() {
            let n = 0;
            fn b() { fn c() { fn d() { n = n + 1; return n; } return d; } return c; }
            let d = b()();
            d(); d();
            return [n, d()];
          }
          print a();".into(), "[2, 3]\n"),
        // one variable seen at three levels, written at two
        ("fn a() { let x = 1; fn b() { x = x + 10; fn c() { x = x + 100; return x; } return c; } let c = b(); return [c(), x]; } print a();".into(), "[111, 111]\n"),
        // two closures share one variable
        ("fn pair() { let v = 0; fn get() { return v; } fn set(x) { v = x; } return [get, set]; } let p = pair(); p[1](42); print p[0]();".into(), "42\n"),
        // an open upvalue sees later writes to the variable, and the enclosing function sees the closure's writes
        ("fn f() { let x = 1; fn g() { return x; } x = 2; return g; } print f()();".into(), "2\n"),
        ("fn f() { let x = 1; fn inc() { x = x + 1; } inc(); inc(); return x; } print f();".into(), "3\n"),
        ("fn f() { let x = 1; fn g() { return x + x + x; } return g(); } print f();".into(), "3\n"),
        // a block local declared inside a loop is a fresh variable each iteration...
        ("let fs = []; let i = 0; while i < 3 { let j = i; fn f() { return j; } push(fs, f); i = i + 1; } print [fs[0](), fs[1](), fs[2]()];".into(), "[0, 1, 2]\n"),
        // ...but a variable declared outside the loop is shared by every closure
        ("fn f() { let fs = []; let i = 0; while i < 3 { fn g() { return i; } push(fs, g); i = i + 1; } return [fs[0](), fs[2]()]; } print f();".into(), "[3, 3]\n"),
        // a closure capturing a block local outlives the block
        ("fn f() { let g = nil; if true { let x = \"block\"; fn h() { return x; } g = h; } let y = \"after\"; return g; } print f()();".into(), "block\n"),
        // nested functions without captures, and recursion through a nested function's own name
        ("fn outer() { fn sq(x) { return x * x; } return sq(4); } print outer();".into(), "16\n"),
        ("fn outer() { fn fact(n) { if n <= 1 { return 1; } return n * fact(n - 1); } return fact(5); } print outer();".into(), "120\n"),
        // a closure whose upvalue holds the closure itself (a cycle through an upvalue)
        ("fn f() { fn me() { return me; } return me; } let s = f(); print s() == s; print s()()() == s;".into(), "true\ntrue\n"),
        // heap values in closed upvalues survive collections
        ("fn make() { let log = []; fn add(s) { push(log, s + \"!\"); return log; } return add; } let add = make(); add(\"a\"); print add(\"b\");".into(),
         "[\"a!\", \"b!\"]\n"),
        // GC root: open upvalues. Once g = nil, nothing but the open list refers to x's
        // upvalue; a collection must keep it, because h captures the same slot later.
        ("fn f() { let x = \"x\" + \"1\"; fn g() { return x; } g = nil; let junk = \"a\" + \"b\"; fn h() { return x; } return h(); } print f();".into(), "x1\n"),
        // GC edge: a closed upvalue's value. s is reachable only through get's closed
        // upvalue when junk is allocated, so marking has to follow it. (An array, not a
        // string: "a" + "b" appends into the literal "a"'s buffer, which is always rooted
        // through the constant pool, so it would survive a broken marker anyway.)
        ("fn make() { let s = [1, 2]; fn get() { let junk = [3]; return s; } return get; } let g = make(); print g();".into(), "[1, 2]\n"),
        // functions nested in a top-level block are locals of the script, not globals
        ("if true { let n = 5; fn get() { return n; } print get(); } print 1;".into(), "5\n1\n"),
    ] {
        assert_eq!(run(&src).as_deref(), Ok(want), "{src}");
    }
}

#[test]
fn closure_errors() {
    for (src, want) in [
        // y is declared after f, so f's `y` is a global and reading it is a clean runtime error
        ("fn outer() { fn f() { return y; } let y = 1; return f(); }\nouter();", "line 1: undefined variable 'y'"),
        (&format!("{COUNTER}make_counter()(1);"), "next() takes 0 argument(s), got 1"),
        ("fn outer() { let k = 1; fn r(n) { return r(n + k); } return r(0); }\nouter();", "line 1: stack overflow"),
        ("fn f() { return 1; }\nif true { return 2; }", "line 2: return outside function"),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: got {err:?}");
    }
}

#[test]
fn disassembler_shows_upvalues() {
    let dis = jinso::disasm::disassemble(&jinso::compile(&format!("{COUNTER}print make_counter()();")).unwrap());
    for want in ["== fn next ==", "GetUpvalue      0  (count)", "SetUpvalue      0  (count)", "Closure         1  (<fn next>)", "local 0 (count)"] {
        assert!(dis.contains(want), "missing {want:?} in:\n{dis}");
    }
}

#[test]
fn compare_and_branch() {
    // if/while conditions that are comparisons compile to one JumpIfNot* instruction;
    // results must match the general path (a comparison used as a value).
    for (src, want) in [
        ("let r = []; let a = 1; let b = 2;
          if a < b { push(r, 1); } if b < a { push(r, 2); } if a < a { push(r, 3); }
          if a <= b { push(r, 4); } if a <= a { push(r, 5); } if b <= a { push(r, 6); }
          if b > a { push(r, 7); } if a > b { push(r, 8); } if a > a { push(r, 9); }
          if b >= a { push(r, 10); } if a >= a { push(r, 11); } if a >= b { push(r, 12); }
          print r;", "[1, 4, 5, 7, 10, 11]\n"),
        ("let i = 0; while i <= 3 { i = i + 1; } print i; while i >= 1 { i = i - 2; } print i;", "4\n0\n"),
        ("let i = 10; while i > 7 { i = i - 1; } print i; if i < 8 { print \"else not taken\"; } else { print 0; }", "7\nelse not taken\n"),
        // both operands are evaluated, left first, before the comparison
        ("fn t(x) { print x; return x; } if t(1) < t(2) { print \"yes\"; } while t(5) >= t(9) { }", "1\n2\nyes\n5\n9\n"),
        // other conditions still take the general path
        ("if 1 == 1 { print 1; } if not (2 < 1) { print 2; } if 1 < 2 and 2 < 3 { print 3; }", "1\n2\n3\n"),
    ] {
        assert_eq!(run(src).as_deref(), Ok(want), "{src}");
    }
    for (src, want) in [
        ("if 1 < \"a\" { }", "line 1: operands must be ints, got int and str"),
        ("let x = nil;\nwhile x >= 0 { }", "line 2: operands must be ints, got nil and int"),
    ] {
        let err = run(src).unwrap_err();
        assert!(err.contains(want), "{src}: got {err:?}");
    }
}

#[test]
fn step_budget_stops_programs_that_never_end() {
    let limited = |src: &str| {
        let chunk = jinso::compile(src).unwrap();
        jinso::vm::run_limited(&chunk, &mut Vec::new(), false, 10_000).unwrap_err()
    };
    // an endless loop, and recursion whose call count is effectively unbounded (2^4096)
    assert!(limited("let i = 0;\nwhile true { i = i + 1; }").contains("line 2: step limit reached"));
    assert!(limited("fn f(n) { if n == 0 { return 0; } return f(n - 1) + f(n - 1); }\nprint f(4000);").contains("step limit reached"));
    // a program within budget runs normally
    let chunk = jinso::compile("let i = 0; while i < 100 { i = i + 1; } print i;").unwrap();
    let mut out = Vec::new();
    jinso::vm::run_limited(&chunk, &mut out, false, 10_000).unwrap();
    assert_eq!(out, b"100\n");
}
