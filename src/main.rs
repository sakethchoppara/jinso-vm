use std::process::exit;

const USAGE: &str = "usage: lumen <run|dis|ast> <file.lm>";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(cmd), Some(path)) = (args.get(1), args.get(2)) else {
        eprintln!("{USAGE}");
        exit(64);
    };
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        exit(66);
    });
    let result = match cmd.as_str() {
        "run" => lumen::run(&src, &mut std::io::stdout().lock()),
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
