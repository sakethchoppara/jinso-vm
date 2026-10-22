pub mod ast;
pub mod chunk;
pub mod compiler;
pub mod disasm;
pub mod gc;
pub mod lexer;
pub mod parser;
pub mod value;
pub mod vm;
#[cfg(target_arch = "wasm32")]
mod wasm;

use std::io::Write;

/// Source to AST. Multiple syntax errors are joined with newlines.
pub fn parse(src: &str) -> Result<Vec<ast::Stmt>, String> {
    parser::parse(lexer::lex(src)?).map_err(|errs| errs.join("\n"))
}

pub fn compile(src: &str) -> Result<chunk::Chunk, String> {
    compiler::compile(&parse(src)?)
}

pub fn run(src: &str, out: &mut impl Write) -> Result<(), String> {
    vm::run(&compile(src)?, out, false).map(|_| ())
}
