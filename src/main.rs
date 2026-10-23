use jinso::optimize::Passes;
use std::process::exit;

const USAGE: &str = "usage: jinso <run|dis|ast> [-O0|-O1] [--no-PASS] [--gc-stress] <file.jo>
  -O1         optimize (the default)     -O0        don't optimize
  --no-PASS   turn off one optimization pass, where PASS is one of: ";

fn usage(problem: &str) -> ! {
    eprintln!("{problem}{USAGE}{}", Passes::NAMES.join(", "));
    exit(64);
}

fn main() {
    let mut passes = Passes::ALL;
    let mut gc_stress = false;
    let mut args = Vec::new();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-O0" => passes = Passes::NONE,
            "-O1" => passes = Passes::ALL,
            "--gc-stress" => gc_stress = true,
            f if f.starts_with("--no-") => {
                if !passes.disable(&f[5..]) {
                    usage(&format!("unknown pass in {f}\n"));
                }
            }
            f if f.starts_with('-') => usage(&format!("unknown flag {f}\n")),
            _ => args.push(a),
        }
    }
    let (Some(cmd), Some(path)) = (args.first(), args.get(1)) else { usage("") };
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        exit(66);
    });
    let result = match cmd.as_str() {
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
