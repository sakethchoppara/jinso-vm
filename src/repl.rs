//! `jinso repl`: an interactive session. Each input is compiled onto the end of the same
//! program and run in the same VM, so globals, functions and everything on the heap carry
//! over from one input to the next. Reads plain lines (no editing or history), so it works
//! the same piped as typed, and the tests drive it that way.

use crate::compiler::Compiler;
use crate::optimize::Passes;
use crate::vm::Vm;
use crate::{disasm, lexer, parser};
use std::io::{self, BufRead, Write};

const HELP: &str = ":dis shows the last input's bytecode, :reset forgets everything, :quit exits";

pub fn run(input: impl BufRead, out: &mut impl Write, gc_stress: bool, passes: Passes) -> io::Result<()> {
    let mut session = Session::new(gc_stress, passes);
    let mut pending = String::new();
    let mut lines = input.lines();
    loop {
        write!(out, "{}", if pending.is_empty() { "> " } else { "... " })?;
        out.flush()?;
        let Some(line) = lines.next() else {
            writeln!(out)?;
            if !pending.is_empty() {
                session.eval(&pending, true, out)?;
            }
            return Ok(());
        };
        let line = line?;
        if pending.is_empty() {
            match line.trim() {
                "" => continue,
                ":quit" => return Ok(()),
                ":reset" => {
                    session = Session::new(gc_stress, passes);
                    continue;
                }
                ":dis" => {
                    match session.last {
                        Some((from, to)) => write!(out, "{}", disasm::disassemble_range(session.compiler.chunk(), from, to))?,
                        None => writeln!(out, "nothing to show yet")?,
                    }
                    continue;
                }
                cmd if cmd.starts_with(':') => {
                    writeln!(out, "unknown command {cmd} ({HELP})")?;
                    continue;
                }
                _ => {}
            }
        }
        // A blank line ends a continuation: whatever is pending gets reported as it is,
        // so a missing `;` can't trap you at the `...` prompt.
        let finish = line.trim().is_empty();
        if !finish {
            pending.push_str(&line);
            pending.push('\n');
        }
        if session.eval(&pending, finish, out)? == Step::Done {
            pending.clear();
        }
    }
}

#[derive(PartialEq)]
enum Step {
    Done,
    NeedMore,
}

struct Session {
    compiler: Compiler,
    vm: Vm,
    /// The last input's code, for `:dis`.
    last: Option<(usize, usize)>,
    passes: Passes,
}

impl Session {
    fn new(gc_stress: bool, passes: Passes) -> Session {
        Session { compiler: Compiler::new(), vm: Vm::new(gc_stress), last: None, passes }
    }

    /// Compiles and runs one input. Line numbers in errors count from the input's first
    /// line. `finish` means no more lines are coming, so unfinished input is an error.
    fn eval(&mut self, src: &str, finish: bool, out: &mut impl Write) -> io::Result<Step> {
        let tokens = match lexer::lex(src) {
            Ok(tokens) => tokens,
            Err(e) => {
                writeln!(out, "{e}")?;
                return Ok(Step::Done);
            }
        };
        let stmts = match parser::parse(tokens) {
            Ok(stmts) => stmts,
            Err(e) if e.incomplete && !finish => return Ok(Step::NeedMore),
            Err(e) => {
                writeln!(out, "{}", e.messages.join("\n"))?;
                return Ok(Step::Done);
            }
        };
        // A failed compile adds nothing: no globals, no functions.
        let start = match self.compiler.add(&stmts, self.passes) {
            Ok(start) => start,
            Err(e) => {
                writeln!(out, "{e}")?;
                return Ok(Step::Done);
            }
        };
        self.last = Some((start, self.compiler.chunk().code.len()));
        // A runtime error resets the VM to the top level but keeps its heap and globals.
        if let Err(e) = self.vm.run(self.compiler.chunk(), start, out, u64::MAX) {
            writeln!(out, "{e}")?;
        }
        Ok(Step::Done)
    }
}
