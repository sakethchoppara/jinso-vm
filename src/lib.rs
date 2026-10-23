pub mod ast;
pub mod chunk;
pub mod compiler;
pub mod disasm;
pub mod gc;
pub mod lexer;
pub mod optimize;
pub mod parser;
pub mod repl;
pub mod upvalues;
pub mod value;
pub mod vm;
#[cfg(target_arch = "wasm32")]
mod wasm;

use std::io::Write;

/// Source to AST. Multiple syntax errors are joined with newlines.
pub fn parse(src: &str) -> Result<Vec<ast::Stmt>, String> {
    parser::parse(lexer::lex(src)?).map_err(|errs| errs.messages.join("\n"))
}

/// Compiles with every optimization (`-O1`).
pub fn compile(src: &str) -> Result<chunk::Chunk, String> {
    compile_with(src, optimize::Passes::ALL)
}

pub fn compile_with(src: &str, passes: optimize::Passes) -> Result<chunk::Chunk, String> {
    compiler::compile(&parse(src)?, passes)
}

pub fn run(src: &str, out: &mut impl Write) -> Result<(), String> {
    vm::run(&compile(src)?, out, false).map(|_| ())
}

/// What running a program produced: everything it printed and, if it failed, the error.
#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub output: String,
    pub error: Option<String>,
}

/// Compiles with `passes` and runs under a step budget (`u64::MAX` for none).
pub fn execute(src: &str, passes: optimize::Passes, gc_stress: bool, steps: u64) -> Outcome {
    let mut out = Vec::new();
    let error = compile_with(src, passes).and_then(|c| vm::run_limited(&c, &mut out, gc_stress, steps)).err();
    Outcome { output: String::from_utf8_lossy(&out).into_owned(), error }
}

/// The reference run: unoptimized (`-O0`), on a VM without superinstructions, so every
/// instruction runs exactly as the compiler first emitted it.
pub fn execute_reference(src: &str, gc_stress: bool, steps: u64) -> Outcome {
    let mut out = Vec::new();
    let error = compile_with(src, optimize::Passes::NONE).and_then(|c| vm::run_reference(&c, &mut out, gc_stress, steps)).err();
    Outcome { output: String::from_utf8_lossy(&out).into_owned(), error }
}

/// `--verify`: runs the program as the reference (unoptimized, no superinstructions) and
/// fully optimized, and compares what they printed and how they failed, line numbers
/// included. The reference is the specification, so any difference is an optimizer or VM
/// bug. Ok carries the (agreed) outcome.
pub fn verify(src: &str, gc_stress: bool) -> Result<Outcome, String> {
    let o0 = execute_reference(src, gc_stress, u64::MAX);
    let o1 = execute(src, optimize::Passes::ALL, gc_stress, u64::MAX);
    if o0 == o1 {
        Ok(o1)
    } else {
        Err(format!("-O0 and -O1 disagree\n-O0: {o0:?}\n-O1: {o1:?}"))
    }
}
