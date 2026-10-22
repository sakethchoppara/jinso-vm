use crate::ast::{BinOp, Expr, ExprKind, Stmt, UnOp};
use crate::chunk::{Chunk, FnInfo, Op, UpvalueDesc};
use crate::value::Value;
use std::collections::HashMap;

/// Walks the AST and emits stack-machine bytecode.
pub fn compile(stmts: &[Stmt]) -> Result<Chunk, String> {
    let mut c = Compiler { chunk: Chunk::default(), globals: HashMap::new(), fns: vec![FnState::default()] };
    for s in stmts {
        c.stmt(s)?;
    }
    Ok(c.chunk)
}

struct Compiler {
    chunk: Chunk,
    globals: HashMap<String, u16>,
    /// One entry per function being compiled, innermost last. Entry 0 is the top-level
    /// script, whose block locals live at the bottom of the stack (frame base 0).
    fns: Vec<FnState>,
}

#[derive(Default)]
struct FnState {
    /// A local's index here is its stack slot, relative to the frame's base: statements
    /// leave the stack balanced, so between statements the frame holds exactly these
    /// locals. Inside a function, the parameters are slots 0..arity.
    locals: Vec<Local>,
    /// 0 = top level of the script (variables are globals); each block adds 1.
    depth: u32,
    upvalues: Vec<UpvalueDesc>,
}

struct Local {
    name: String,
    depth: u32,
    /// Some closure captures it, so leaving its scope must close the upvalue, not just pop.
    captured: bool,
}

/// Where a name lives, decided at compile time.
enum Var {
    Local(u16),
    Upvalue(u16),
    Global(u16),
}

impl Compiler {
    fn cur(&mut self) -> &mut FnState {
        self.fns.last_mut().unwrap()
    }

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

    /// Innermost local with this name wins (shadowing); then a variable of an enclosing
    /// function, captured as an upvalue; otherwise it's a global.
    fn resolve(&mut self, name: &str) -> Result<Var, String> {
        let f = self.fns.len() - 1;
        if let Some(slot) = local(&self.fns[f], name) {
            return Ok(Var::Local(slot));
        }
        if let Some(i) = self.upvalue(f, name)? {
            return Ok(Var::Upvalue(i));
        }
        Ok(Var::Global(self.global(name)?))
    }

    /// Finds `name` in the functions enclosing `fns[f]`, adding an upvalue to every
    /// function between there and `f` so each closure can hand it down to the next.
    fn upvalue(&mut self, f: usize, name: &str) -> Result<Option<u16>, String> {
        if f == 0 {
            return Ok(None); // the script has no enclosing function
        }
        let enclosing = f - 1;
        if let Some(slot) = local(&self.fns[enclosing], name) {
            self.fns[enclosing].locals[slot as usize].captured = true;
            return self.add_upvalue(f, true, slot, name).map(Some);
        }
        match self.upvalue(enclosing, name)? {
            Some(i) => self.add_upvalue(f, false, i, name).map(Some),
            None => Ok(None),
        }
    }

    fn add_upvalue(&mut self, f: usize, is_local: bool, index: u16, name: &str) -> Result<u16, String> {
        let ups = &mut self.fns[f].upvalues;
        if let Some(i) = ups.iter().position(|u| u.is_local == is_local && u.index == index) {
            return Ok(i as u16);
        }
        let i = u16::try_from(ups.len()).map_err(|_| "too many captured variables".to_string())?;
        ups.push(UpvalueDesc { is_local, index, name: name.to_string() });
        Ok(i)
    }

    fn declare_local(&mut self, name: &str, line: u32) -> Result<(), String> {
        let f = self.cur();
        if f.locals.len() > u16::MAX as usize {
            return Err(format!("line {line}: too many locals"));
        }
        let depth = f.depth;
        f.locals.push(Local { name: name.to_string(), depth, captured: false });
        Ok(())
    }

    fn constant(&mut self, v: Value, line: u32) -> Result<(), String> {
        let i = u16::try_from(self.chunk.constants.len()).map_err(|_| format!("line {line}: too many constants"))?;
        self.chunk.constants.push(v);
        self.chunk.emit_arg(Op::Constant, i, line);
        Ok(())
    }

    fn block(&mut self, stmts: &[Stmt], line: u32) -> Result<(), String> {
        self.cur().depth += 1;
        for s in stmts {
            self.stmt(s)?;
        }
        let f = self.fns.last_mut().unwrap();
        f.depth -= 1;
        while f.locals.last().is_some_and(|l| l.depth > f.depth) {
            let local = f.locals.pop().unwrap();
            self.chunk.emit(if local.captured { Op::CloseUpvalue } else { Op::Pop }, line);
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::Let { name, init, line } => {
                self.expr(init)?;
                if self.fns.len() == 1 && self.cur().depth == 0 {
                    let i = self.global(name)?;
                    self.chunk.emit_arg(Op::DefineGlobal, i, *line);
                } else {
                    // The initializer's value is already on the stack, in exactly the slot
                    // this local gets, so declaring it emits no code.
                    self.declare_local(name, *line)?;
                }
            }
            Stmt::Assign { name, value, line } => {
                self.expr(value)?;
                let (op, i) = match self.resolve(name)? {
                    Var::Local(i) => (Op::SetLocal, i),
                    Var::Upvalue(i) => (Op::SetUpvalue, i),
                    Var::Global(i) => (Op::SetGlobal, i),
                };
                self.chunk.emit_arg(op, i, *line);
            }
            Stmt::IndexAssign { target, index, value, line } => {
                self.expr(target)?;
                self.expr(index)?;
                self.expr(value)?;
                self.chunk.emit(Op::SetIndex, *line);
            }
            Stmt::Print(e) => {
                self.expr(e)?;
                self.chunk.emit(Op::Print, e.line);
            }
            // Jump over; body...; Nil; Return; over: (Constant <fn> | Closure f); bind name
            Stmt::Fn { name, params, body, line } => {
                let global = self.fns.len() == 1 && self.cur().depth == 0;
                if !global {
                    // Declared before the body, so the body can call itself by name. Its
                    // slot gets filled when the Constant or Closure below runs.
                    self.declare_local(name, *line)?;
                }
                let over = self.chunk.emit_jump(Op::Jump, *line);
                let idx = u16::try_from(self.chunk.functions.len()).map_err(|_| "too many functions".to_string())?;
                let offset = self.chunk.code.len();
                self.chunk.functions.push(FnInfo { name: name.clone(), arity: params.len() as u8, offset, end: 0, upvalues: Vec::new() });

                self.fns.push(FnState { depth: 1, ..FnState::default() });
                for p in params {
                    if local(&self.fns[self.fns.len() - 1], p).is_some() {
                        return Err(format!("line {line}: duplicate parameter '{p}'"));
                    }
                    self.declare_local(p, *line)?;
                }
                for s in body {
                    self.stmt(s)?;
                }
                // Implicit `return nil;`. Return closes upvalues and discards the frame,
                // so no Pops or CloseUpvalues are needed.
                self.constant(Value::Nil, *line)?;
                self.chunk.emit(Op::Return, *line);
                let state = self.fns.pop().unwrap();

                let info = &mut self.chunk.functions[idx as usize];
                info.end = self.chunk.code.len();
                info.upvalues = state.upvalues;
                self.chunk.patch_jump(over)?;
                // A function that captures nothing needs no closure object.
                if self.chunk.functions[idx as usize].upvalues.is_empty() {
                    self.constant(Value::Fn(idx), *line)?;
                } else {
                    self.chunk.emit_arg(Op::Closure, idx, *line);
                }
                if global {
                    let g = self.global(name)?;
                    self.chunk.emit_arg(Op::DefineGlobal, g, *line);
                }
            }
            Stmt::Return { value, line } => {
                if self.fns.len() == 1 {
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
                let h = self.chunk.strings.len() as u32;
                let len = u32::try_from(s.len()).map_err(|_| format!("line {line}: string literal too long"))?;
                self.chunk.strings.push(s.clone());
                self.constant(Value::Str { h, len }, line)?;
            }
            ExprKind::Var(name) => {
                let (op, i) = match self.resolve(name)? {
                    Var::Local(i) => (Op::GetLocal, i),
                    Var::Upvalue(i) => (Op::GetUpvalue, i),
                    Var::Global(i) => (Op::GetGlobal, i),
                };
                self.chunk.emit_arg(op, i, line);
            }
            ExprKind::Call(callee, args) => {
                self.expr(callee)?;
                for a in args {
                    self.expr(a)?;
                }
                self.chunk.emit_arg(Op::Call, args.len() as u16, line);
            }
            ExprKind::Array(items) => {
                for item in items {
                    self.expr(item)?;
                }
                self.chunk.emit_arg(Op::MakeArray, items.len() as u16, line);
            }
            ExprKind::Index(target, index) => {
                self.expr(target)?;
                self.expr(index)?;
                self.chunk.emit(Op::Index, line);
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

fn local(f: &FnState, name: &str) -> Option<u16> {
    f.locals.iter().rposition(|l| l.name == name).map(|i| i as u16)
}
