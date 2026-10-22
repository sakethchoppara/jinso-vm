use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: u32,
}

#[derive(Debug)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Nil,
    Var(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
}

#[derive(Debug)]
pub enum Stmt {
    Let { name: String, init: Expr, line: u32 },
    Assign { name: String, value: Expr, line: u32 },
    Print(Expr),
    If { cond: Expr, then: Vec<Stmt>, els: Vec<Stmt>, line: u32 }, // `else if` nests an If in els
    While { cond: Expr, body: Vec<Stmt>, line: u32 },
    Fn { name: String, params: Vec<String>, body: Vec<Stmt>, line: u32 },
    Return { value: Expr, line: u32 }, // bare `return;` returns nil
    Expr(Expr),                        // e.g. a call made for its side effects
}

// S-expression printing, used by `lumen ast` and the precedence tests.

impl fmt::Display for UnOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            UnOp::Neg => "-",
            UnOp::Not => "not",
        })
    }
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        })
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.kind {
            ExprKind::Int(n) => write!(f, "{n}"),
            ExprKind::Bool(b) => write!(f, "{b}"),
            ExprKind::Nil => write!(f, "nil"),
            ExprKind::Var(name) => write!(f, "{name}"),
            ExprKind::Unary(op, e) => write!(f, "({op} {e})"),
            ExprKind::Binary(op, l, r) => write!(f, "({op} {l} {r})"),
            ExprKind::Call(callee, args) => {
                write!(f, "(call {callee}")?;
                for a in args {
                    write!(f, " {a}")?;
                }
                write!(f, ")")
            }
        }
    }
}

impl fmt::Display for Stmt {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Stmt::Let { name, init, .. } => write!(f, "(let {name} {init})"),
            Stmt::Assign { name, value, .. } => write!(f, "(set {name} {value})"),
            Stmt::Print(e) => write!(f, "(print {e})"),
            Stmt::If { cond, then, els, .. } if els.is_empty() => write!(f, "(if {cond} {})", Block(then)),
            Stmt::If { cond, then, els, .. } => write!(f, "(if {cond} {} {})", Block(then), Block(els)),
            Stmt::While { cond, body, .. } => write!(f, "(while {cond} {})", Block(body)),
            Stmt::Fn { name, params, body, .. } => write!(f, "(fn {name} ({}) {})", params.join(" "), Block(body)),
            Stmt::Return { value, .. } => write!(f, "(return {value})"),
            Stmt::Expr(e) => write!(f, "{e}"),
        }
    }
}

struct Block<'a>(&'a [Stmt]);

impl fmt::Display for Block<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(block")?;
        for s in self.0 {
            write!(f, " {s}")?;
        }
        write!(f, ")")
    }
}
