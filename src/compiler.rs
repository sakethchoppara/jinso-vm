use crate::ast::{BinOp, Expr, ExprKind, Stmt, UnOp};
use crate::chunk::{Chunk, FnInfo, Op, UpvalueDesc};
use crate::optimize::{FnLabels, Item, Label, Passes, optimize, relocate};
use crate::value::Value;
use std::collections::HashMap;

/// Walks the AST and emits stack-machine bytecode: first as an instruction list with
/// symbolic jump labels, then optimized with `passes`, then laid out by `relocate`.
pub fn compile(stmts: &[Stmt], passes: Passes) -> Result<Chunk, String> {
    let mut c = Compiler {
        chunk: Chunk::default(),
        globals: HashMap::new(),
        fns: vec![FnState::default()],
        items: Vec::new(),
        next_label: 0,
        fn_labels: Vec::new(),
    };
    for s in stmts {
        c.stmt(s)?;
    }
    let entries: Vec<Label> = c.fn_labels.iter().map(|f| f.start).collect();
    optimize(&mut c.items, &mut c.chunk.constants, &entries, passes);
    relocate(&c.items, &c.fn_labels, &mut c.chunk)?;
    Ok(c.chunk)
}

struct Compiler {
    chunk: Chunk,
    globals: HashMap<String, u16>,
    /// One entry per function being compiled, innermost last. Entry 0 is the top-level
    /// script, whose block locals live at the bottom of the stack (frame base 0).
    fns: Vec<FnState>,
    /// The program so far, with jumps pointing at labels.
    items: Vec<Item>,
    next_label: Label,
    fn_labels: Vec<FnLabels>,
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
    fn emit(&mut self, op: Op, line: u32) {
        self.items.push(Item::Ins { op, arg: 0, line });
    }

    fn emit_arg(&mut self, op: Op, arg: u16, line: u32) {
        self.items.push(Item::Ins { op, arg, line });
    }

    fn label(&mut self) -> Label {
        self.next_label += 1;
        self.next_label - 1
    }

    fn place(&mut self, label: Label) {
        self.items.push(Item::Label(label));
    }

    fn goto(&mut self, target: Label, line: u32) {
        self.items.push(Item::Goto { target, line });
    }

    /// A conditional jump to a new label, returned for the caller to place.
    fn branch(&mut self, op: Op, line: u32) -> Label {
        let target = self.label();
        self.items.push(Item::Branch { op, target, line });
        target
    }

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
        self.emit_arg(Op::Constant, i, line);
        Ok(())
    }

    fn block(&mut self, stmts: &[Stmt], line: u32) -> Result<(), String> {
        self.cur().depth += 1;
        for s in stmts {
            self.stmt(s)?;
        }
        let f = self.fns.last_mut().unwrap();
        f.depth -= 1;
        let mut ended = Vec::new();
        while f.locals.last().is_some_and(|l| l.depth > f.depth) {
            ended.push(f.locals.pop().unwrap().captured);
        }
        for captured in ended {
            self.emit(if captured { Op::CloseUpvalue } else { Op::Pop }, line);
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::Let { name, init, line } => {
                self.expr(init)?;
                if self.fns.len() == 1 && self.cur().depth == 0 {
                    let i = self.global(name)?;
                    self.emit_arg(Op::DefineGlobal, i, *line);
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
                self.emit_arg(op, i, *line);
            }
            Stmt::IndexAssign { target, index, value, line } => {
                self.expr(target)?;
                self.expr(index)?;
                self.expr(value)?;
                self.emit(Op::SetIndex, *line);
            }
            Stmt::Print(e) => {
                self.expr(e)?;
                self.emit(Op::Print, e.line);
            }
            // Jump over; body...; Nil; Return; over: (Constant <fn> | Closure f); bind name
            Stmt::Fn { name, params, body, line } => {
                let global = self.fns.len() == 1 && self.cur().depth == 0;
                if !global {
                    // Declared before the body, so the body can call itself by name. Its
                    // slot gets filled when the Constant or Closure below runs.
                    self.declare_local(name, *line)?;
                }
                let over = self.label();
                self.goto(over, *line);
                let idx = u16::try_from(self.chunk.functions.len()).map_err(|_| "too many functions".to_string())?;
                let (start, end) = (self.label(), self.label());
                self.place(start);
                // offset and end are filled in by `relocate`.
                self.chunk.functions.push(FnInfo { name: name.clone(), arity: params.len() as u8, offset: 0, end: 0, upvalues: Vec::new() });
                self.fn_labels.push(FnLabels { index: idx as usize, start, end });

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
                self.emit(Op::Return, *line);
                let state = self.fns.pop().unwrap();

                self.chunk.functions[idx as usize].upvalues = state.upvalues;
                self.place(end);
                self.place(over);
                // A function that captures nothing needs no closure object.
                if self.chunk.functions[idx as usize].upvalues.is_empty() {
                    self.constant(Value::Fn(idx), *line)?;
                } else {
                    self.emit_arg(Op::Closure, idx, *line);
                }
                if global {
                    let g = self.global(name)?;
                    self.emit_arg(Op::DefineGlobal, g, *line);
                }
            }
            Stmt::Return { value, line } => {
                if self.fns.len() == 1 {
                    return Err(format!("line {line}: return outside function"));
                }
                self.expr(value)?;
                self.emit(Op::Return, *line);
            }
            Stmt::Expr(e) => {
                self.expr(e)?;
                self.emit(Op::Pop, e.line);
            }
            // cond; JumpIfFalse else; then; [Jump end; else: els;] end:
            Stmt::If { cond, then, els, line } => {
                let to_else = self.cond_jump(cond)?;
                self.block(then, *line)?;
                if els.is_empty() {
                    self.place(to_else);
                } else {
                    let end = self.label();
                    self.goto(end, *line);
                    self.place(to_else);
                    self.block(els, *line)?;
                    self.place(end);
                }
            }
            // start: cond; JumpIfFalse exit; body; Loop start; exit:
            Stmt::While { cond, body, line } => {
                let start = self.label();
                self.place(start);
                let exit = self.cond_jump(cond)?;
                self.block(body, *line)?;
                self.goto(start, *line);
                self.place(exit);
            }
        }
        Ok(())
    }

    /// Compiles an `if`/`while` condition and the jump taken when it's false; returns the
    /// label to place where the false case continues. A comparison becomes one compare-and-branch instruction instead of
    /// `Lt` (push a bool) followed by `JumpIfFalse` (pop it).
    fn cond_jump(&mut self, cond: &Expr) -> Result<Label, String> {
        let fused = match &cond.kind {
            ExprKind::Binary(BinOp::Lt, l, r) => Some((Op::JumpIfNotLt, l, r)),
            ExprKind::Binary(BinOp::Le, l, r) => Some((Op::JumpIfNotLe, l, r)),
            ExprKind::Binary(BinOp::Gt, l, r) => Some((Op::JumpIfNotGt, l, r)),
            ExprKind::Binary(BinOp::Ge, l, r) => Some((Op::JumpIfNotGe, l, r)),
            _ => None,
        };
        let op = match fused {
            Some((op, l, r)) => {
                self.expr(l)?;
                self.expr(r)?;
                op
            }
            None => {
                self.expr(cond)?;
                Op::JumpIfFalse
            }
        };
        Ok(self.branch(op, cond.line))
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
                self.emit_arg(op, i, line);
            }
            ExprKind::Call(callee, args) => {
                self.expr(callee)?;
                for a in args {
                    self.expr(a)?;
                }
                self.emit_arg(Op::Call, args.len() as u16, line);
            }
            ExprKind::Array(items) => {
                for item in items {
                    self.expr(item)?;
                }
                self.emit_arg(Op::MakeArray, items.len() as u16, line);
            }
            ExprKind::Index(target, index) => {
                self.expr(target)?;
                self.expr(index)?;
                self.emit(Op::Index, line);
            }
            ExprKind::Unary(op, inner) => {
                self.expr(inner)?;
                self.emit(if *op == UnOp::Neg { Op::Neg } else { Op::Not }, line);
            }
            // Short-circuit: r runs only if it decides the result. JumpIfFalse has already
            // checked that l is a bool, so when l decides, the result is a known constant.
            // l; JumpIfFalse short; r; Jump end; short: false; end:
            ExprKind::Binary(BinOp::And, l, r) => {
                self.expr(l)?;
                let short = self.branch(Op::JumpIfFalse, line);
                self.expr(r)?;
                let end = self.label();
                self.goto(end, line);
                self.place(short);
                self.constant(Value::Bool(false), line)?;
                self.place(end);
            }
            // l; JumpIfFalse rhs; true; Jump end; rhs: r; end:
            ExprKind::Binary(BinOp::Or, l, r) => {
                self.expr(l)?;
                let rhs = self.branch(Op::JumpIfFalse, line);
                self.constant(Value::Bool(true), line)?;
                let end = self.label();
                self.goto(end, line);
                self.place(rhs);
                self.expr(r)?;
                self.place(end);
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
                self.emit(op, line);
                if negate {
                    self.emit(Op::Not, line);
                }
            }
        }
        Ok(())
    }
}

fn local(f: &FnState, name: &str) -> Option<u16> {
    f.locals.iter().rposition(|l| l.name == name).map(|i| i as u16)
}
