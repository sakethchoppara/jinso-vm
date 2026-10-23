use jinso::optimize::Passes;

/// Feeds `input` to a REPL and returns the transcript: prompts (`> `, and `... ` while a
/// block is open) followed by whatever each input printed. Every session runs four ways,
/// unoptimized and optimized, each normally and under --gc-stress, and all four must
/// produce the same transcript (except `:dis`, which shows the bytecode itself).
fn session(input: &str) -> String {
    let at = |passes, stress| {
        let mut out = Vec::new();
        jinso::repl::run(input.as_bytes(), &mut out, stress, passes).unwrap();
        String::from_utf8(out).unwrap()
    };
    let reference = at(Passes::ALL, false);
    if !input.contains(":dis") {
        for (passes, stress) in [(Passes::ALL, true), (Passes::NONE, false), (Passes::NONE, true)] {
            assert_eq!(at(passes, stress), reference, "transcript differs (stress={stress}) for {input:?}");
        }
    }
    reference
}

#[test]
fn globals_and_functions_carry_over() {
    // the PRD's example session
    let input = "let x = 5;\nprint x * 2;\nfn double(n) { return n * 2; }\nprint double(x);\nprint 1 / 0;\nprint x;\n";
    assert_eq!(session(input), "> > 10\n> > 10\n> line 1: division by zero\n> 5\n> \n");
}

#[test]
fn a_function_can_use_a_global_from_a_later_input() {
    let input = "fn show() { print later; }\nshow();\nlet later = \"defined\";\nshow();\n";
    assert_eq!(session(input), "> > line 1: undefined variable 'later'\n> > defined\n> \n");
}

#[test]
fn multi_line_blocks_continue_until_complete() {
    let input = "let n = 3;\nwhile n > 0 {\n  print n;\n  n = n - 1;\n}\nprint \"done\";\n";
    assert_eq!(session(input), "> > ... ... ... 3\n2\n1\n> done\n> \n");
    let input = "fn f(a,\n      b) {\n  return a +\n    b;\n}\nprint f(1, 2);\n";
    assert_eq!(session(input), "> ... ... ... ... > 3\n> \n");
}

#[test]
fn braces_and_quotes_inside_strings_dont_continue() {
    let input = "print \"a { \\\" b\";\nprint \"}{\";\n// a comment with a {\nprint 1;\n";
    assert_eq!(session(input), "> a { \" b\n> }{\n> > 1\n> \n");
}

#[test]
fn a_runtime_error_keeps_the_session_and_its_globals() {
    let input = "let a = [1, 2];\nlet s = \"kept\";\nprint a[5];\npush(a, 3);\nprint a;\nprint s;\n";
    assert_eq!(session(input), "> > > line 1: index 5 out of bounds for array of length 3\n> > [1, 2, 3]\n> kept\n> \n"
        .replace("length 3", "length 2"));
}

#[test]
fn an_error_deep_in_calls_resets_the_stack() {
    let input = "fn down(n) { if n == 0 { return 1 / 0; } return down(n - 1); }\ndown(50);\nprint 2 + 2;\n";
    assert_eq!(session(input), "> > line 1: division by zero\n> 4\n> \n");
}

#[test]
fn a_closure_survives_an_error_in_the_call_that_made_it() {
    // v is captured (an open upvalue) when the call fails; recovery closes it, so the
    // closure stored in the global still works afterwards
    let input = "let g = nil;\nfn mk() { let v = [\"captured\"]; fn get() { return v; } g = get; return 1 / 0; }\nmk();\nprint g();\n";
    assert_eq!(session(input), "> > > line 1: division by zero\n> [\"captured\"]\n> \n");
}

#[test]
fn closures_and_arrays_live_across_inputs() {
    let input = "fn counter() { let c = 0; fn next() { c = c + 1; return c; } return next; }\nlet a = counter();\nlet b = counter();\na();\nprint a();\nprint b();\nlet log = [];\npush(log, a());\npush(log, \"x\");\nprint log;\n";
    assert_eq!(session(input), "> > > > > 2\n> 1\n> > > > [3, \"x\"]\n> \n");
}

#[test]
fn a_syntax_error_keeps_the_session_and_defines_nothing() {
    let input = "let ok = 1;\nlet = 2;\nprint ok;\nlet bad = 3; let = 4;\nprint bad;\n";
    assert_eq!(
        session(input),
        "> > 1:5: expected identifier, got '='\n> 1\n> 1:18: expected identifier, got '='\n> line 1: undefined variable 'bad'\n> \n"
    );
}

#[test]
fn a_compile_error_rolls_back_the_whole_input() {
    // `return` at the top level is a compile error, after `zed` and `f` were compiled:
    // neither may exist afterwards, and the next input reuses their slots cleanly
    let input = "let zed = 1; fn f() { return 1; } return 2;\nprint zed;\nprint f;\nlet zed = 7;\nprint zed;\n";
    assert_eq!(
        session(input),
        "> line 1: return outside function\n> line 1: undefined variable 'zed'\n> line 1: undefined variable 'f'\n> > 7\n> \n"
    );
}

#[test]
fn unfinished_input_is_reported_at_a_blank_line_or_the_end() {
    // a blank line gives up on the continuation, so a missing `;` can't trap you
    assert_eq!(session("print 1\n\nprint 2;\n"), "> ... 2:1: expected ';', got end of file\n> 2\n> \n");
    // end of input inside a block reports the missing `}`
    assert_eq!(session("if true {\n  print 1;\n"), "> ... ... \n3:1: expected '}', got end of file\n");
}

#[test]
fn escapes_and_errors_with_positions() {
    let input = "print \"tab\\there\";\nprint \"bad \\q\";\nprint \"ok\";\n";
    assert_eq!(session(input), "> tab\there\n> 1:12: unknown escape sequence '\\q'\n> ok\n> \n");
}

#[test]
fn commands() {
    // :dis shows only the last input's bytecode, optimized
    let out = session("let x = 1;\nprint x + 2 * 3;\n:dis\n");
    assert!(out.ends_with("> > 7\n> 0006    1 GetGlobal       0  (x)\n0009    | AddConst        3  (6)\n0012    | Print        \n> \n"), "{out}");
    assert_eq!(session(":dis\n"), "> nothing to show yet\n> \n");
    // :reset forgets everything
    assert_eq!(session("let x = 1;\n:reset\nprint x;\n"), "> > > line 1: undefined variable 'x'\n> \n");
    // :quit stops reading; blank lines are ignored
    assert_eq!(session("print 1;\n\n:quit\nprint 2;\n"), "> 1\n> > ");
    assert!(session(":help\n").contains("unknown command :help (:dis shows"));
}

#[test]
fn globals_survive_100_inputs_under_gc_stress() {
    // each input builds a string and an array from the previous global, so collections
    // between inputs must find everything through globals defined in earlier inputs
    let mut input = String::from("let v0 = [\"s\", 0];\n");
    for i in 1..=100 {
        input += &format!("let v{i} = [v{}[0] + \"s\", v{}[1] + {i}, v{}];\n", i - 1, i - 1, i - 1);
    }
    input += "print len(v100[0]);\nprint v100[1];\nprint v100[2][2][2][1];\n";
    let out = session(&input);
    assert!(out.ends_with("> 101\n> 5050\n> 4753\n> \n"), "{}", &out[out.len().saturating_sub(80)..]);

    let mut out = Vec::new();
    jinso::repl::run(input.as_bytes(), &mut out, true, Passes::ALL).unwrap();
    assert!(String::from_utf8(out).unwrap().ends_with("> 101\n> 5050\n> 4753\n> \n"));
}
