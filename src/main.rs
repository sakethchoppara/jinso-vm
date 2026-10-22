use std::process::exit;

const USAGE: &str = "usage: lumen <run|dis|ast> [--gc-stress] <file.lm>";

fn main() {
    let (flags, args): (Vec<String>, Vec<String>) = std::env::args().skip(1).partition(|a| a.starts_with("--"));
    let gc_stress = flags.iter().any(|f| f == "--gc-stress");
    if let Some(f) = flags.iter().find(|f| *f != "--gc-stress") {
        eprintln!("unknown flag {f}\n{USAGE}");
        exit(64);
    }
    let (Some(cmd), Some(path)) = (args.first(), args.get(1)) else {
        eprintln!("{USAGE}");
        exit(64);
    };
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        exit(66);
    });
    let result = match cmd.as_str() {
        "run" => lumen::compile(&src).and_then(|c| lumen::vm::run(&c, &mut std::io::stdout().lock(), gc_stress).map(|_| ())),
        "dis" => lumen::compile(&src).map(|c| print!("{}", lumen::disasm::disassemble(&c))),
        "ast" => lumen::parse(&src).map(|stmts| stmts.iter().for_each(|s| println!("{s}"))),
        _ => {
            eprintln!("{USAGE}");
            exit(64);
        }
    };
    if let Err(e) = result {
        eprintln!("{e}");
        exit(1);
    }
}
