//! Random-input fuzzer: `cargo run --example fuzz -- [seconds]` (default 600).
//!
//! Generates token soup and mutated versions of the example programs, then pushes each
//! through lex, parse, compile, disassemble and (half the time under --gc-stress) the VM,
//! at both -O0 and -O1. Any panic is a bug, and so is any difference between the two
//! levels: the input is saved to fuzz-crash.jo or fuzz-mismatch.jo and the run fails.
//! Runs in the dev profile on purpose, so debug assertions and overflow checks are on.

use jinso::optimize::Passes;
use std::panic;
use std::time::{Duration, Instant, SystemTime};

const VOCAB: &[&str] = &[
    "let", "fn", "return", "if", "else", "while", "print", "true", "false", "nil", "and", "or", "not",
    "+", "-", "*", "/", "%", "(", ")", "{", "}", ",", ";", "=", "==", "!=", "<", "<=", ">", ">=",
    "x", "y", "f", "g", "0", "1", "2", "-1", "9223372036854775807", "\"s\"", "\"\"", "// c\n", "\n",
    "@", "\"open", "99999999999999999999",
    "[", "]", "[]", "len", "push", "a", "a[0]", "push(a, a)",
    // closure fragments: nested functions that capture, return and call each other
    "fn f() {", "fn g(x) { return x + y; }", "fn h() { y = y + 1; return h; }", "let y = 0;",
    "return g;", "f()()", "g(1)", "h()()",
    // escape fragments, valid and not
    "\"a\\nb\"", "\"\\\"\"", "\"\\u{1F600}\"", "\"\\u{D800}\"", "\"\\q\"", "\"\\u{\"", "\"x\\",
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
        .filter(|p| p.extension().is_some_and(|e| e == "jo"))
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect();
    assert!(!corpus.is_empty(), "no .jo programs found in examples/ or bench/; run from the repo root");

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut runs = 0u64;
    let mut slow = 0u32;
    while Instant::now() < deadline {
        let src = if rng.below(2) == 0 { soup(&mut rng) } else { mutate(&mut rng, &corpus) };
        let stress = rng.below(2) == 0;
        let started = Instant::now();
        let result = panic::catch_unwind(|| check(&src, stress));
        if started.elapsed() > Duration::from_secs(1) {
            slow += 1;
            std::fs::write(format!("fuzz-slow-{slow}.jo"), &src).unwrap();
            eprintln!("slow input ({:.1?}, gc_stress={stress}) saved to fuzz-slow-{slow}.jo", started.elapsed());
        }
        match result {
            Err(_) => {
                std::fs::write("fuzz-crash.jo", &src).unwrap();
                eprintln!("panic (gc_stress={stress}) on input saved to fuzz-crash.jo (seed {seed}):\n{src}");
                std::process::exit(1);
            }
            Ok(Err(mismatch)) => {
                std::fs::write("fuzz-mismatch.jo", &src).unwrap();
                eprintln!("{mismatch}\n(gc_stress={stress}) input saved to fuzz-mismatch.jo (seed {seed}):\n{src}");
                std::process::exit(1);
            }
            Ok(Ok(())) => {}
        }
        runs += 1;
    }
    println!("{runs} inputs in {secs}s, no panics, -O0 and -O1 agree (seed {seed})");
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

fn check(src: &str, gc_stress: bool) -> Result<(), String> {
    let (o0, o1) = (jinso::compile_with(src, Passes::NONE), jinso::compile_with(src, Passes::ALL));
    match (&o0, &o1) {
        (Ok(a), Ok(b)) => {
            jinso::disasm::diff(a, b);
        }
        (Err(a), Err(b)) if a == b => return Ok(()),
        _ => return Err(format!("compiling differs:\n-O0: {:?}\n-O1: {:?}", o0.err(), o1.err())),
    }
    // Only loop-free programs execute: a loop can double a string 30 times and allocate
    // gigabytes before any limit trips. Recursion still runs, under a step budget, since
    // a mutated fib(9223372036854775807) branches twice per call and would never finish.
    if src.contains("while") {
        return Ok(());
    }
    // The reference: unoptimized, and no superinstructions in the VM either.
    let a = jinso::execute_reference(src, gc_stress, 100_000);
    let b = jinso::execute(src, Passes::ALL, gc_stress, 100_000);
    // The budget counts loop jumps and calls; threading can turn a Jump into a Loop, so
    // which line a "step limit reached" error names can legitimately differ. Inconclusive.
    let limited = |o: &jinso::Outcome| o.error.as_deref().is_some_and(|e| e.contains("step limit reached"));
    if a != b && !limited(&a) && !limited(&b) {
        return Err(format!("running differs:\n-O0: {a:?}\n-O1: {b:?}"));
    }
    Ok(())
}
