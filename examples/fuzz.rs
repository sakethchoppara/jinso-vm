//! Random-input fuzzer: `cargo run --example fuzz -- [seconds]` (default 600).
//!
//! Generates token soup and mutated versions of the example programs, then pushes each
//! through lex, parse, compile, disassemble and (half the time under --gc-stress) the VM.
//! Any panic is a bug: the input is saved to fuzz-crash.lm and the run fails.
//! Runs in the dev profile on purpose, so debug assertions and overflow checks are on.

use std::panic;
use std::time::{Duration, Instant, SystemTime};

const VOCAB: &[&str] = &[
    "let", "fn", "return", "if", "else", "while", "print", "true", "false", "nil", "and", "or", "not",
    "+", "-", "*", "/", "%", "(", ")", "{", "}", ",", ";", "=", "==", "!=", "<", "<=", ">", ">=",
    "x", "y", "f", "g", "0", "1", "2", "-1", "9223372036854775807", "\"s\"", "\"\"", "// c\n", "\n",
    "@", "\"open", "99999999999999999999",
    "[", "]", "[]", "len", "push", "a", "a[0]", "push(a, a)",
];

/// xorshift64: plenty for picking tokens, and no dependency.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

fn main() {
    let secs = std::env::args().nth(1).map_or(600, |s| s.parse().expect("seconds must be a number"));
    let seed = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() as u64 | 1;
    let mut rng = Rng(seed);

    let corpus: Vec<String> = ["examples", "bench"]
        .iter()
        .flat_map(|dir| std::fs::read_dir(dir).unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "lm"))
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect();

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut runs = 0u64;
    while Instant::now() < deadline {
        let src = if rng.below(2) == 0 { soup(&mut rng) } else { mutate(&mut rng, &corpus) };
        let stress = rng.below(2) == 0;
        if panic::catch_unwind(|| check(&src, stress)).is_err() {
            std::fs::write("fuzz-crash.lm", &src).unwrap();
            eprintln!("panic (gc_stress={stress}) on input saved to fuzz-crash.lm (seed {seed}):\n{src}");
            std::process::exit(1);
        }
        runs += 1;
    }
    println!("{runs} inputs in {secs}s, no panics (seed {seed})");
}

fn soup(rng: &mut Rng) -> String {
    let len = 1 + rng.below(200);
    (0..len).map(|_| rng.pick(VOCAB)).collect::<Vec<_>>().join(" ")
}

/// A real program with a few tokens deleted, inserted, duplicated or swapped.
fn mutate(rng: &mut Rng, corpus: &[String]) -> String {
    let mut toks: Vec<&str> = corpus[rng.below(corpus.len())].split_whitespace().collect();
    for _ in 0..1 + rng.below(4) {
        let i = rng.below(toks.len() + 1);
        match rng.below(4) {
            0 if i < toks.len() => {
                toks.remove(i);
            }
            1 if i < toks.len() => toks.insert(i, toks[i]),
            2 if i + 1 < toks.len() => toks.swap(i, i + 1),
            _ => toks.insert(i, rng.pick(VOCAB)),
        }
    }
    toks.join(" ")
}

fn check(src: &str, gc_stress: bool) {
    let Ok(chunk) = lumen::compile(src) else { return };
    lumen::disasm::disassemble(&chunk);
    // A mutated `while` can loop forever, so only loop-free programs execute. Recursion is
    // still fine: MAX_FRAMES turns runaway recursion into an error.
    if !src.contains("while") {
        let _ = lumen::vm::run(&chunk, &mut std::io::sink(), gc_stress);
    }
}
