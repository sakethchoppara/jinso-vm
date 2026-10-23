pub mod ast;
pub mod chunk;
pub mod compiler;
pub mod disasm;
pub mod gc;
pub mod lexer;
pub mod optimize;
pub mod parser;
pub mod upvalues;
pub mod value;
pub mod vm;
#[cfg(target_arch = "wasm32")]
mod wasm;

use std::io::Write;

/// Source to AST. Multiple syntax errors are joined with newlines.
pub fn parse(src: &str) -> Result<Vec<ast::Stmt>, String> {
    parser::parse(lexer::lex(src)?).map_err(|errs| errs.join("\n"))
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
