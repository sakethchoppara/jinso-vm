mod common;
use common::*;
use jinso::optimize::Passes;

/// Instruction names in the optimized listing, in order.
fn ops(src: &str) -> Vec<String> {
    names(&dis(src))
}

/// The same with every pass except `fuse`, for tests about folding: fusing would rename
/// the `Add` they look for to `AddConst`.
fn unfused(src: &str) -> Vec<String> {
    let mut passes = Passes::ALL;
    passes.disable("fuse");
    names(&dis_at(src, passes))
}

fn names(listing: &str) -> Vec<String> {
    listing.lines().filter(|l| !l.starts_with("==")).filter_map(|l| l.get(10..).map(|r| r.split_whitespace().next().unwrap_or("").to_string())).collect()
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
    assert_eq!(unfused(src), ["Constant", "DefineGlobal", "GetGlobal", "Constant", "Add", "Print"]);
    // left to right: (x + 2) + 3 has no constant subexpression
    assert_eq!(unfused("let x = 1; print x + 2 + 3;").iter().filter(|o| *o == "Add").count(), 2);
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
        assert!(unfused(src).contains(&kept.to_string()), "{src} should keep {kept}:\n{}", dis(src));
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
    assert!(unfused("print \"a\" + \"b\";").contains(&"Add".to_string()));
    assert!(unfused("print \"a\" == \"a\";").contains(&"Eq".to_string()));
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

/// Every jump in `chunk` as (at, op, target).
fn jumps(chunk: &jinso::chunk::Chunk) -> Vec<(usize, jinso::chunk::Op, usize)> {
    use jinso::chunk::Op;
    let mut out = Vec::new();
    let mut ip = 0;
    while ip < chunk.code.len() {
        let op = Op::ALL[chunk.code[ip] as usize];
        if op.operand_len() == 2 {
            let off = chunk.read_u16(ip + 1) as usize;
            match op {
                Op::Loop => out.push((ip, op, ip + 3 - off)),
                Op::Jump | Op::JumpIfFalse | Op::JumpIfNotLt | Op::JumpIfNotLe | Op::JumpIfNotGt | Op::JumpIfNotGe => {
                    out.push((ip, op, ip + 3 + off))
                }
                _ => {}
            }
        }
        ip += 1 + op.operand_len();
    }
    out
}

/// After threading, no jump lands on an unconditional jump, except where following it
/// would send a conditional jump backward (which the VM can't encode).
fn assert_threaded(src: &str) {
    use jinso::chunk::Op;
    let chunk = jinso::compile(src).unwrap();
    for (at, op, target) in jumps(&chunk) {
        // a jump to the end of the program (a final `while`'s exit) lands on nothing
        let Some(&byte) = chunk.code.get(target) else { continue };
        let landed = Op::ALL[byte as usize];
        let conditional = !matches!(op, Op::Jump | Op::Loop);
        let ok = match landed {
            Op::Jump => false,
            Op::Loop => conditional || target == at,
            _ => true,
        };
        assert!(ok, "{op:?} at {at} lands on {landed:?} at {target}:\n{}", jinso::disasm::disassemble(&chunk));
    }
}

const NESTED: &str = "let out = [];
let i = 0;
while i < 4 {
  let j = 0;
  while j < 3 {
    if j == 1 {
      if i % 2 == 0 { push(out, \"a\"); } else { push(out, \"b\"); }
    } else {
      push(out, j);
    }
    j = j + 1;
  }
  i = i + 1;
  if i == 3 { push(out, \"x\"); } else { push(out, \"y\"); }
}
print out;";

#[test]
fn jump_threading_across_nested_if_and_while() {
    assert_eq!(
        run(NESTED).as_deref(),
        Ok("[0, \"a\", 2, \"y\", 0, \"b\", 2, \"y\", 0, \"a\", 2, \"x\", 0, \"b\", 2, \"y\"]\n")
    );
    assert_threaded(NESTED);
    // An if-else ending a loop body: the then-branch jumped to the loop's back edge, and
    // now is a back edge. (NESTED's outer body ends by popping its local j, so there the
    // branch lands on a Pop, not a jump, and correctly isn't threaded.)
    let src = "let i = 0; while i < 3 { i = i + 1; if i == 2 { print \"two\"; } else { print i; } }";
    assert_eq!(run(src).as_deref(), Ok("1\ntwo\n3\n"));
    assert_threaded(src);
    let o0 = jumps(&jinso::compile_with(src, Passes::NONE).unwrap());
    let o1 = jumps(&jinso::compile(src).unwrap());
    let loops = |j: &[(usize, jinso::chunk::Op, usize)]| j.iter().filter(|x| x.1 == jinso::chunk::Op::Loop).count();
    assert!(loops(&o1) > loops(&o0), "threading should turn a Jump to a Loop into a Loop");
}

#[test]
fn every_example_is_threaded_and_has_no_dead_jumps() {
    for path in std::fs::read_dir("examples").unwrap().chain(std::fs::read_dir("bench").unwrap()) {
        let path = path.unwrap().path();
        if path.extension().is_none_or(|e| e != "jo") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        assert_threaded(&src);
        let chunk = jinso::compile(&src).unwrap();
        for (at, op, target) in jumps(&chunk) {
            assert!(!(op == jinso::chunk::Op::Jump && target == at + 3), "{path:?}: dead jump at {at}");
        }
    }
}

#[test]
fn removes_code_after_return() {
    // the print after return, and the implicit `return nil` after the last return
    let src = "fn f(x) { if x { return 1; } else { return 2; } print \"never\"; } print f(true); print f(false);";
    assert_eq!(run(src).as_deref(), Ok("1\n2\n"));
    let listing = dis(src);
    assert!(!listing.contains("never") && !listing.contains("(nil)"), "{listing}");
    assert_eq!(listing.matches("Return").count(), 2, "{listing}");
    // a function that falls off the end still returns nil
    assert_eq!(run("fn g() { let x = 1; } print g();").as_deref(), Ok("nil\n"));
    assert!(dis("fn g() { let x = 1; } print g();").contains("(nil)"));
}

#[test]
fn removes_code_after_an_endless_loop_but_keeps_function_bodies() {
    let src = "fn f() { while true { return 5; } print \"after\"; } print f();";
    assert_eq!(run(src).as_deref(), Ok("5\n"));
    assert!(!dis(src).contains("after"));
    // g's definition can never run, but its body stays: it's an entry point
    let listing = dis("fn spin() { while true { } fn g() { return 1; } return g; }");
    assert!(listing.contains("== fn g =="), "{listing}");
}

#[test]
fn optimizes_inside_closures() {
    let src = "fn make() { let n = 0; fn next() { if true { n = n + 1 * 2; } else { n = 0; } return n; } return next; }
               let c = make(); c(); print c();";
    assert_eq!(run(src).as_deref(), Ok("4\n"));
    let listing = dis(src);
    assert!(listing.contains("GetUpvalue") && listing.contains("(2)") && !listing.contains("JumpIfFalse"), "{listing}");
    assert_threaded(src);
}

#[test]
fn short_circuit_on_literals_drops_the_dead_operand() {
    let src = "fn f() { print \"ran\"; return true; } print false and f(); print true or f(); print true and f();";
    assert_eq!(run(src).as_deref(), Ok("false\ntrue\nran\ntrue\n"));
    assert_eq!(dis(src).matches("Call").count(), 1, "{}", dis(src));
}

#[test]
fn verify_agrees_on_examples_and_errors() {
    for src in [NESTED, "print 1 / 0;", "let a = [1];\nprint a[3];", "fn f() { return f(); }\nf();", "print \"\\q\";"] {
        let o0 = jinso::execute(src, Passes::NONE, false, u64::MAX);
        let verified = jinso::verify(src, false).unwrap();
        assert_eq!(verified, o0, "{src}");
    }
}

#[test]
fn diff_lines_up_both_listings() {
    let chunk = |p| jinso::compile_with("print 2 * 3;\nprint 4;", p).unwrap();
    let d = jinso::disasm::diff(&chunk(Passes::NONE), &chunk(Passes::ALL));
    let lines: Vec<&str> = d.lines().collect();
    assert!(lines[0].starts_with("-O0") && lines[0].contains("-O1"), "{d}");
    assert!(lines[1].contains("Constant        0  (2)") && lines[1].contains(" | ") && lines[1].contains("(6)"), "{d}");
    assert!(lines.iter().any(|l| l.contains("Mul") && l.contains(" < ")), "{d}");
    assert!(lines.iter().any(|l| l.contains("(4)") && l.matches("(4)").count() == 2), "unchanged lines pair up:\n{d}");
}

#[test]
fn optimizer_is_fast_on_the_largest_program() {
    // every example and benchmark concatenated into one program, ten times over
    let mut src = String::new();
    for path in std::fs::read_dir("examples").unwrap().chain(std::fs::read_dir("bench").unwrap()) {
        let path = path.unwrap().path();
        if path.extension().is_some_and(|e| e == "jo") {
            src += &std::fs::read_to_string(path).unwrap();
            src += "\n";
        }
    }
    let src = src.repeat(10);
    let stmts = jinso::parse(&src).unwrap();
    let start = std::time::Instant::now();
    jinso::compiler::compile(&stmts, Passes::ALL).unwrap();
    let optimized = start.elapsed();
    let start = std::time::Instant::now();
    jinso::compiler::compile(&stmts, Passes::NONE).unwrap();
    let plain = start.elapsed();
    eprintln!("{} lines: compile -O0 {plain:?}, -O1 {optimized:?}", src.lines().count());
    assert!(optimized.as_millis() < 100, "optimizing took {optimized:?}");
}

#[test]
fn fuses_constant_operands() {
    let src = "fn f(n) { let i = 0; while i < n { i = i + 1; } return [i * 2, i - 1, i / 2, i % 3]; } print f(10);";
    assert_eq!(run(src).as_deref(), Ok("[20, 9, 5, 1]\n"));
    let listing = dis(src);
    for op in ["AddConst", "MulConst", "SubConst", "DivConst", "ModConst"] {
        assert!(listing.contains(op), "missing {op}:\n{listing}");
    }
    // string appends go through AddConst, keeping the in-place append
    let (out, stats) = run_with("let s = \"\"; let i = 0; while i < 1000 { s = s + \"ab\"; i = i + 1; } print len(s);", true).unwrap();
    assert_eq!((out.as_str(), stats.live), ("2000\n", 2));
    assert!(dis("let s = \"a\"; print s + \"b\";").contains("AddConst"));
}

#[test]
fn fused_instructions_fail_like_the_originals() {
    for (src, err) in [
        ("let x = 1;\nprint x / 0;", "line 2: division by zero"),
        ("let x = 1;\nprint x % 0;", "line 2: division by zero"),
        ("let x = 9223372036854775807;\nprint x + 1;", "line 2: integer overflow"),
        ("let x = -9223372036854775807;\nprint x - 2;", "line 2: integer overflow"),
        ("let x = 4611686018427387904;\nprint x * 2;", "line 2: integer overflow"),
        ("let s = \"a\"; print s + 1;", "line 1: operands must be two ints or two strs, got str and int"),
        ("let x = 1; print x + \"a\";", "line 1: operands must be two ints or two strs, got int and str"),
        ("print nil + 1;", "line 1: operands must be two ints or two strs, got nil and int"),
        ("let a = [1]; print a - 1;", "line 1: operands must be ints, got array and int"),
        ("let s = \"a\"; print s * 2;", "line 1: operands must be ints, got str and int"),
    ] {
        assert_eq!(run(src).unwrap_err(), err, "{src}");
        assert!(dis(src).contains("Const "), "{src} should use a fused instruction:\n{}", dis(src));
    }
}

#[test]
fn fusing_runs_after_folding() {
    // 2 + 3 must fold to 5, not become `Constant 2; AddConst 3`
    assert_eq!(ops("print 2 + 3;"), ["Constant", "Print"]);
    assert_eq!(ops("let x = 1; print x + 2 * 3;"), ["Constant", "DefineGlobal", "GetGlobal", "AddConst", "Print"]);
}
