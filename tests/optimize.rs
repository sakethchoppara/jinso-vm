mod common;
use common::*;
use jinso::optimize::Passes;

/// Instruction names in the optimized listing, in order.
fn ops(src: &str) -> Vec<String> {
    dis(src).lines().filter(|l| !l.starts_with("==")).filter_map(|l| l.get(10..).map(|r| r.split_whitespace().next().unwrap_or("").to_string())).collect()
}

#[test]
fn folds_int_and_bool_literals() {
    for (src, want_out, want_ops) in [
        ("print 2 * 3 + 1;", "7\n", vec!["Constant", "Print"]),
        ("print (10 - 4) / 2 % 2;", "1\n", vec!["Constant", "Print"]),
        ("print -5 + 2;", "-3\n", vec!["Constant", "Print"]),
        ("print 1 < 2; print 2 > 3;", "true\nfalse\n", vec!["Constant", "Print", "Constant", "Print"]),
        ("print 1 <= 1; print 4 >= 5; print 1 != 2;", "true\nfalse\ntrue\n", vec!["Constant", "Print", "Constant", "Print", "Constant", "Print"]),
        ("print 1 == 1; print true == false; print nil == nil; print 1 == true;", "true\nfalse\ntrue\nfalse\n",
         vec!["Constant", "Print", "Constant", "Print", "Constant", "Print", "Constant", "Print"]),
        ("print not true; print not not false;", "false\nfalse\n", vec!["Constant", "Print", "Constant", "Print"]),
        ("print -(-(3));", "3\n", vec!["Constant", "Print"]),
    ] {
        assert_eq!(run(src).as_deref(), Ok(want_out), "{src}");
        assert_eq!(ops(src), want_ops, "{src}\n{}", dis(src));
    }
}

#[test]
fn folds_only_the_constant_part() {
    // x + 2 * 3: the 2 * 3 folds, the addition to a variable can't
    let src = "let x = 5; print x + 2 * 3;";
    assert_eq!(run(src).as_deref(), Ok("11\n"));
    assert_eq!(ops(src), ["Constant", "DefineGlobal", "GetGlobal", "Constant", "Add", "Print"]);
    // left to right: (x + 2) * 3 has no constant subexpression
    assert!(ops("let x = 1; print x + 2 + 3;").contains(&"Add".to_string()));
}

#[test]
fn never_folds_what_would_fail() {
    for (src, err, kept) in [
        ("print 1 / 0;", "line 1: division by zero", "Div"),
        ("print 1 % 0;", "line 1: division by zero", "Mod"),
        ("print 9223372036854775807 + 1;", "line 1: integer overflow", "Add"),
        ("print -9223372036854775807 - 2;", "line 1: integer overflow", "Sub"),
        ("print 4611686018427387904 * 2;", "line 1: integer overflow", "Mul"),
        ("print (-9223372036854775807 - 1) / -1;", "line 1: integer overflow", "Div"),
        ("print -(-9223372036854775807 - 1);", "line 1: integer overflow", "Neg"),
        ("print 1 + true;", "line 1: operands must be two ints or two strs, got int and bool", "Add"),
        ("print -true;", "line 1: operand of '-' must be an int, got bool", "Neg"),
        ("print not 1;", "line 1: operand of 'not' must be a bool, got int", "Not"),
        ("print 1 < nil;", "line 1: operands must be ints, got int and nil", "Lt"),
        ("print \"a\" - 1;", "line 1: operands must be ints, got str and int", "Sub"),
    ] {
        assert_eq!(run(src).unwrap_err(), err, "{src}");
        assert!(ops(src).contains(&kept.to_string()), "{src} should keep {kept}:\n{}", dis(src));
    }
}

#[test]
fn errors_keep_their_line_after_folding() {
    // (2 - 2) folds to 0 on line 3; the division on line 2 still fails at runtime there
    let src = "let a = 1;\nprint a /\n (2 - 2);";
    assert_eq!(run(src).unwrap_err(), "line 2: division by zero");
    // a folded constant takes the operator's line
    let listing = dis("print 1\n  +\n  2;");
    assert!(listing.lines().next().unwrap().starts_with("0000    2 Constant"), "{listing}");
}

#[test]
fn never_folds_calls() {
    let src = "fn f() { print \"called\"; return 1; } print f() + 1; print 2 * f();";
    assert_eq!(run(src).as_deref(), Ok("called\n2\ncalled\n2\n"));
    assert_eq!(ops(src).iter().filter(|o| *o == "Call").count(), 2);
}

#[test]
fn strings_are_not_folded() {
    assert!(ops("print \"a\" + \"b\";").contains(&"Add".to_string()));
    assert!(ops("print \"a\" == \"a\";").contains(&"Eq".to_string()));
}

#[test]
fn folds_inside_functions_and_closures() {
    let src = "fn f() { return 10 * 10; } fn mk() { let k = 2 + 3; fn g() { return k * (4 - 1); } return g; } print f(); print mk()();";
    assert_eq!(run(src).as_deref(), Ok("100\n15\n"));
    let listing = dis(src);
    assert!(listing.contains("(100)") && listing.contains("(5)") && listing.contains("(3)"), "{listing}");
    // 10 * 10 folded; k * 3 can't, because k is a captured variable
    assert_eq!(listing.matches(" Mul").count(), 1, "{listing}");
}

#[test]
fn constant_conditions() {
    for (src, want) in [
        ("if false { print 1; } print 2;", "2\n"),
        ("if true { print 1; } else { print 2; }", "1\n"),
        ("if false { print 1; } else { print 2; }", "2\n"),
        ("if 1 < 2 { print \"yes\"; } if 3 <= 2 { print \"no\"; }", "yes\n"),
        ("if 2 > 1 and 1 >= 1 { print 1; }", "1\n"),
        ("while false { print 1; } print 2;", "2\n"),
        ("fn f() { let i = 0; while true { i = i + 1; if i == 3 { return i; } } } print f();", "3\n"),
        // short-circuiting on literals: f() never runs at either level
        ("fn f() { print \"ran\"; return true; } print false and f(); print true or f();", "false\ntrue\n"),
        ("print true or 1 / 0 == 0;", "true\n"),
    ] {
        assert_eq!(run(src).as_deref(), Ok(want), "{src}");
    }
    // the condition and jump are gone from the listing
    assert!(!ops("if true { print 1; }").iter().any(|o| o.starts_with("Jump")));
    assert_eq!(ops("if 1 < 2 { print 1; }"), ["Constant", "Print"]);
    // a non-bool literal condition is left for the VM to reject
    assert_eq!(run("if 1 { }").unwrap_err(), "line 1: condition must be a bool, got int");
    assert!(ops("if 1 { }").contains(&"JumpIfFalse".to_string()));
}

#[test]
fn passes_can_be_turned_off() {
    let mut no_fold = Passes::ALL;
    assert!(no_fold.disable("fold"));
    assert!(!no_fold.disable("nonsense"));
    assert!(dis_at("print 2 * 3;", no_fold).contains("Mul"));
    assert!(!dis_at("print 2 * 3;", Passes::ALL).contains("Mul"));
    assert!(dis_at("print 2 * 3;", Passes::NONE).contains("Mul"));
}
