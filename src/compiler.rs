use crate::ast::{BinOp, Expr, ExprKind, Stmt, UnOp};
use crate::chunk::{Chunk, FnInfo, Op};
use crate::value::Value;
use std::collections::HashMap;

/// Walks the AST and emits stack-machine bytecode.
pub fn compile(stmts: &[Stmt]) -> Result<Chunk, String> {
    let mut c = Compiler { chunk: Chunk::default(), globals: HashMap::new(), locals: Vec::new(), depth: 0, in_fn: false };
    for s in stmts {
        c.stmt(s)?;
    }
    Ok(c.chunk)
}

struct Compiler {
    chunk: Chunk,
    globals: HashMap<String, u16>,
    /// (name, scope depth). A local's index here is its stack slot, relative to the current
    /// frame's base: statements leave the stack balanced, so between statements the frame
    /// holds exactly these locals. Inside a function, the parameters are slots 0..arity.
    locals: Vec<(String, u32)>,
    /// 0 = top level (variables are globals); each block adds 1.
    depth: u32,
    in_fn: bool,
}

impl Compiler {
    /// Global name -> slot index. A slot is created on first mention, even before its
    /// `let`, so a function can refer to a global defined later; reading it before the
    /// `let` runs is a runtime "undefined variable" error.
    fn global(&mut self, name: &str) -> Result<u16, String> {
        if let Some(&i) = self.globals.get(name) {
            return Ok(i);
        }
        let i = u16::try_from(self.chunk.global_names.len()).map_err(|_| "too many globals".to_string())?;
        self.chunk.global_names.push(name.to_string());
        self.globals.insert(name.to_string(), i);
        Ok(i)
    }

    /// Innermost local with this name wins (shadowing); otherwise it's a global.
    fn resolve(&mut self, name: &str) -> Result<(u16, bool), String> {
        match self.locals.iter().rposition(|(n, _)| n == name) {
            Some(slot) => Ok((slot as u16, true)),
            None => Ok((self.global(name)?, false)),
        }
    }

    fn constant(&mut self, v: Value, line: u32) -> Result<(), String> {
        let i = u16::try_from(self.chunk.constants.len()).map_err(|_| format!("line {line}: too many constants"))?;
        self.chunk.constants.push(v);
        self.chunk.emit_arg(Op::Constant, i, line);
        Ok(())
    }

    fn block(&mut self, stmts: &[Stmt], line: u32) -> Result<(), String> {
        self.depth += 1;
        for s in stmts {
            self.stmt(s)?;
        }
        self.depth -= 1;
        while self.locals.last().is_some_and(|(_, d)| *d > self.depth) {
            self.locals.pop();
            self.chunk.emit(Op::Pop, line);
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::Let { name, init, line } => {
                self.expr(init)?;
                if self.depth == 0 {
                    let i = self.global(name)?;
                    self.chunk.emit_arg(Op::DefineGlobal, i, *line);
                } else {
                    // The initializer's value is already on the stack, in exactly the slot
                    // this local gets, so declaring it emits no code.
                    if self.locals.len() > u16::MAX as usize {
                        return Err(format!("line {line}: too many locals"));
                    }
                    self.locals.push((name.clone(), self.depth));
                }
            }
            Stmt::Assign { name, value, line } => {
                self.expr(value)?;
                let (i, local) = self.resolve(name)?;
                self.chunk.emit_arg(if local { Op::SetLocal } else { Op::SetGlobal }, i, *line);
            }
            Stmt::Print(e) => {
                self.expr(e)?;
                self.chunk.emit(Op::Print, e.line);
            }
            // Jump over; body...; Nil; Return; over: Constant <fn>; DefineGlobal name
            Stmt::Fn { name, params, body, line } => {
                if self.depth > 0 {
                    return Err(format!("line {line}: functions can only be declared at top level"));
                }
                let over = self.chunk.emit_jump(Op::Jump, *line);
                let idx = u16::try_from(self.chunk.functions.len()).map_err(|_| "too many functions".to_string())?;
                let offset = self.chunk.code.len();
                self.chunk.functions.push(FnInfo { name: name.clone(), arity: params.len() as u8, offset });

                // Top level has no locals, so the function's locals start empty.
                self.depth = 1;
                self.in_fn = true;
                for p in params {
                    if self.locals.iter().any(|(n, _)| n == p) {
                        return Err(format!("line {line}: duplicate parameter '{p}'"));
                    }
                    self.locals.push((p.clone(), 1));
                }
                for s in body {
                    self.stmt(s)?;
                }
                // Implicit `return nil;`. Return discards the frame, so no Pops are needed.
                self.constant(Value::Nil, *line)?;
                self.chunk.emit(Op::Return, *line);
                self.locals.clear();
                self.depth = 0;
                self.in_fn = false;

                self.chunk.patch_jump(over)?;
                self.constant(Value::Fn(idx), *line)?;
                let g = self.global(name)?;
                self.chunk.emit_arg(Op::DefineGlobal, g, *line);
            }
            Stmt::Return { value, line } => {
                if !self.in_fn {
                    return Err(format!("line {line}: return outside function"));
                }
                self.expr(value)?;
                self.chunk.emit(Op::Return, *line);
            }
            Stmt::Expr(e) => {
                self.expr(e)?;
                self.chunk.emit(Op::Pop, e.line);
            }
            // cond; JumpIfFalse else; then; [Jump end; else: els;] end:
            Stmt::If { cond, then, els, line } => {
                self.expr(cond)?;
                let to_else = self.chunk.emit_jump(Op::JumpIfFalse, cond.line);
                self.block(then, *line)?;
                if els.is_empty() {
                    self.chunk.patch_jump(to_else)?;
                } else {
                    let to_end = self.chunk.emit_jump(Op::Jump, *line);
                    self.chunk.patch_jump(to_else)?;
                    self.block(els, *line)?;
                    self.chunk.patch_jump(to_end)?;
                }
            }
            // start: cond; JumpIfFalse exit; body; Loop start; exit:
            Stmt::While { cond, body, line } => {
                let start = self.chunk.code.len();
                self.expr(cond)?;
                let to_exit = self.chunk.emit_jump(Op::JumpIfFalse, cond.line);
                self.block(body, *line)?;
                self.chunk.emit_loop(start, *line)?;
                self.chunk.patch_jump(to_exit)?;
            }
        }
        Ok(())
    }

    fn expr(&mut self, e: &Expr) -> Result<(), String> {
        let line = e.line;
        match &e.kind {
            ExprKind::Int(n) => self.constant(Value::Int(*n), line)?,
            ExprKind::Bool(b) => self.constant(Value::Bool(*b), line)?,
            ExprKind::Nil => self.constant(Value::Nil, line)?,
            ExprKind::Str(s) => {
                let i = self.chunk.strings.len() as u32;
                self.chunk.strings.push(s.clone());
                self.constant(Value::Str(i), line)?;
            }
            ExprKind::Var(name) => {
                let (i, local) = self.resolve(name)?;
                self.chunk.emit_arg(if local { Op::GetLocal } else { Op::GetGlobal }, i, line);
            }
            ExprKind::Call(callee, args) => {
                self.expr(callee)?;
                for a in args {
                    self.expr(a)?;
                }
                self.chunk.emit_arg(Op::Call, args.len() as u16, line);
            }
            ExprKind::Unary(op, inner) => {
                self.expr(inner)?;
                self.chunk.emit(if *op == UnOp::Neg { Op::Neg } else { Op::Not }, line);
            }
            // Short-circuit: r runs only if it decides the result. JumpIfFalse has already
            // checked that l is a bool, so when l decides, the result is a known constant.
            // l; JumpIfFalse short; r; Jump end; short: false; end:
            ExprKind::Binary(BinOp::And, l, r) => {
                self.expr(l)?;
                let to_short = self.chunk.emit_jump(Op::JumpIfFalse, line);
                self.expr(r)?;
                let to_end = self.chunk.emit_jump(Op::Jump, line);
                self.chunk.patch_jump(to_short)?;
                self.constant(Value::Bool(false), line)?;
                self.chunk.patch_jump(to_end)?;
            }
            // l; JumpIfFalse rhs; true; Jump end; rhs: r; end:
            ExprKind::Binary(BinOp::Or, l, r) => {
                self.expr(l)?;
                let to_rhs = self.chunk.emit_jump(Op::JumpIfFalse, line);
                self.constant(Value::Bool(true), line)?;
                let to_end = self.chunk.emit_jump(Op::Jump, line);
                self.chunk.patch_jump(to_rhs)?;
                self.expr(r)?;
                self.chunk.patch_jump(to_end)?;
            }
            ExprKind::Binary(op, l, r) => {
                self.expr(l)?;
                self.expr(r)?;
                // != <= >= are compiled as the negation of == > <, keeping the opcode set small.
                let (op, negate) = match op {
                    BinOp::Add => (Op::Add, false),
                    BinOp::Sub => (Op::Sub, false),
                    BinOp::Mul => (Op::Mul, false),
                    BinOp::Div => (Op::Div, false),
                    BinOp::Mod => (Op::Mod, false),
                    BinOp::Eq => (Op::Eq, false),
                    BinOp::Ne => (Op::Eq, true),
                    BinOp::Lt => (Op::Lt, false),
                    BinOp::Le => (Op::Gt, true),
                    BinOp::Gt => (Op::Gt, false),
                    BinOp::Ge => (Op::Lt, true),
                    BinOp::And | BinOp::Or => unreachable!("handled above"),
                };
                self.chunk.emit(op, line);
                if negate {
                    self.chunk.emit(Op::Not, line);
                }
            }
        }
        Ok(())
    }
}
