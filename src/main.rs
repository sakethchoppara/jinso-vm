use jinso::optimize::Passes;
use std::process::exit;

const USAGE: &str = "usage: jinso <run|dis|ast> [options] <file.jo>
       jinso repl [-O0] [--gc-stress]
  -O1          optimize (the default)
  -O0          don't optimize
  --gc-stress  collect garbage before every allocation
  --verify     run: also run unoptimized, and fail if the results differ
  --diff       dis: show unoptimized and optimized side by side
  --no-PASS    turn off one optimization pass, where PASS is one of: ";

fn usage(problem: &str) -> ! {
    eprintln!("{problem}{USAGE}{}", Passes::NAMES.join(", "));
    exit(64);
}

fn main() {
    let mut passes = Passes::ALL;
    let (mut gc_stress, mut verify, mut diff) = (false, false, false);
    let mut args = Vec::new();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-O0" => passes = Passes::NONE,
            "-O1" => passes = Passes::ALL,
            "--gc-stress" => gc_stress = true,
            "--verify" => verify = true,
            "--diff" => diff = true,
            f if f.starts_with("--no-") => {
                if !passes.disable(&f[5..]) {
                    usage(&format!("unknown pass in {f}\n"));
                }
            }
            f if f.starts_with('-') => usage(&format!("unknown flag {f}\n")),
            _ => args.push(a),
        }
    }
    if args.first().map(String::as_str) == Some("repl") {
        let stdin = std::io::stdin().lock();
        jinso::repl::run(stdin, &mut std::io::stdout().lock(), gc_stress, passes).unwrap_or_else(|e| {
            eprintln!("{e}");
            exit(1);
        });
        return;
    }
    let (Some(cmd), Some(path)) = (args.first(), args.get(1)) else { usage("") };
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        exit(66);
    });
    let result = match cmd.as_str() {
        "run" if verify => match jinso::verify(&src, gc_stress) {
            Ok(outcome) => {
                print!("{}", outcome.output);
                outcome.error.map_or(Ok(()), Err)
            }
            Err(mismatch) => {
                eprintln!("{mismatch}");
                exit(2);
            }
        },
        "dis" if diff => jinso::compile_with(&src, Passes::NONE)
            .and_then(|o0| jinso::compile_with(&src, passes).map(|o1| print!("{}", jinso::disasm::diff(&o0, &o1)))),
        "run" => jinso::compile_with(&src, passes)
            .and_then(|c| jinso::vm::run(&c, &mut std::io::stdout().lock(), gc_stress).map(|_| ())),
        "dis" => jinso::compile_with(&src, passes).map(|c| print!("{}", jinso::disasm::disassemble(&c))),
        "ast" => jinso::parse(&src).map(|stmts| stmts.iter().for_each(|s| println!("{s}"))),
        _ => usage(""),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        exit(1);
    }
}
