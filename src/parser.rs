use crate::ast::{BinOp, Expr, ExprKind, Stmt, UnOp};
use crate::lexer::{Tok, Token};

/// Every syntax error in a program, in order.
#[derive(Debug)]
pub struct SyntaxErrors {
    pub messages: Vec<String>,
    /// The first error was hitting the end of the input (`if x {`, `print 1 +`): the
    /// program isn't wrong so much as unfinished. The REPL reads another line when it sees
    /// this, instead of counting braces itself (which a `{` inside a string would fool).
    pub incomplete: bool,
}

/// One syntax error, and whether it happened at the end of the input.
struct Error {
    message: String,
    at_end: bool,
}

/// Parses the whole program, collecting every syntax error instead of stopping at the first.
pub fn parse(toks: Vec<Token>) -> Result<Vec<Stmt>, SyntaxErrors> {
    let mut p = Parser { toks, pos: 0, errors: Vec::new(), depth: 0 };
    let stmts = p.stmts_until(Tok::Eof);
    match p.errors.first() {
        None => Ok(stmts),
        Some(first) => Err(SyntaxErrors {
            incomplete: first.at_end,
            messages: p.errors.into_iter().map(|e| e.message).collect(),
        }),
    }
}

/// Bounds AST depth, so recursion in the parser, compiler and AST drop can't overflow
/// the Rust stack on input like `((((...` or `1+1+...+1`.
const MAX_DEPTH: usize = 128;

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    errors: Vec<Error>,
    depth: usize,
}

/// Infix operators: (binding power, op). Higher binds tighter.
fn infix(t: &Tok) -> Option<(u8, BinOp)> {
    Some(match t {
        Tok::Or => (1, BinOp::Or),
        Tok::And => (2, BinOp::And),
        Tok::EqEq => (3, BinOp::Eq),
        Tok::BangEq => (3, BinOp::Ne),
        Tok::Lt => (4, BinOp::Lt),
        Tok::Le => (4, BinOp::Le),
        Tok::Gt => (4, BinOp::Gt),
        Tok::Ge => (4, BinOp::Ge),
        Tok::Plus => (5, BinOp::Add),
        Tok::Minus => (5, BinOp::Sub),
        Tok::Star => (6, BinOp::Mul),
        Tok::Slash => (6, BinOp::Div),
        Tok::Percent => (6, BinOp::Mod),
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn line(&self) -> u32 {
        self.toks[self.pos].line
    }

    fn advance(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if t != Tok::Eof {
            self.pos += 1;
        }
        t
    }

    fn error(&self, msg: &str) -> Error {
        let t = &self.toks[self.pos];
        Error { message: format!("{}:{}: {msg}, got {}", t.line, t.col, t.tok), at_end: t.tok == Tok::Eof }
    }

    fn expect(&mut self, want: Tok, what: &str) -> Result<(), Error> {
        if *self.peek() == want {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!("expected {what}")))
        }
    }

    fn ident(&mut self) -> Result<String, Error> {
        match self.peek() {
            Tok::Ident(_) => match self.advance() {
                Tok::Ident(name) => Ok(name),
                _ => unreachable!(),
            },
            _ => Err(self.error("expected identifier")),
        }
    }

    /// Parses statements until `end` (not consumed). A bad statement is recorded and
    /// skipped so parsing continues, both at top level and inside blocks.
    fn stmts_until(&mut self, end: Tok) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while *self.peek() != end && *self.peek() != Tok::Eof {
            let before = self.pos;
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(e) => {
                    self.errors.push(e);
                    self.sync();
                    if self.pos == before {
                        self.advance(); // guarantee progress, e.g. on a stray `}`
                    }
                }
            }
        }
        stmts
    }

    /// Panic-mode recovery: skip to just past the next `;`, or to the next statement keyword or `}`.
    fn sync(&mut self) {
        loop {
            match self.peek() {
                Tok::Eof | Tok::RBrace | Tok::Let | Tok::Print | Tok::If | Tok::While | Tok::Fn | Tok::Return => {
                    return;
                }
                Tok::Semi => {
                    self.advance();
                    return;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }

    /// Runs `f` one nesting level deeper; the depth is restored even when `f` fails.
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error(&format!("nesting too deep (max {MAX_DEPTH})")));
        }
        let saved = self.depth;
        self.depth += 1;
        let r = f(self);
        self.depth = saved;
        r
    }

    fn block(&mut self) -> Result<Vec<Stmt>, Error> {
        self.expect(Tok::LBrace, "'{'")?;
        let stmts = self.stmts_until(Tok::RBrace);
        self.expect(Tok::RBrace, "'}'")?;
        Ok(stmts)
    }

    fn stmt(&mut self) -> Result<Stmt, Error> {
        self.nested(Self::stmt_inner)
    }

    fn stmt_inner(&mut self) -> Result<Stmt, Error> {
        let line = self.line();
        let s = match self.peek() {
            Tok::If => {
                self.advance();
                return self.if_rest(line);
            }
            Tok::While => {
                self.advance();
                let cond = self.expr(1)?;
                return Ok(Stmt::While { cond, body: self.block()?, line });
            }
            Tok::Fn => {
                self.advance();
                let name = self.ident()?;
                let params = self.list(Tok::LParen, Tok::RParen, |p| p.ident())?;
                return Ok(Stmt::Fn { name, params, body: self.block()?, line });
            }
            Tok::Return => {
                self.advance();
                let value = if *self.peek() == Tok::Semi { Expr { kind: ExprKind::Nil, line } } else { self.expr(1)? };
                Stmt::Return { value, line }
            }
            Tok::Let => {
                self.advance();
                let name = self.ident()?;
                self.expect(Tok::Eq, "'='")?;
                Stmt::Let { name, init: self.expr(1)?, line }
            }
            Tok::Print => {
                self.advance();
                Stmt::Print(self.expr(1)?)
            }
            // An expression, or an assignment if `=` follows a variable or an index.
            _ => {
                let e = self.expr(1)?;
                if *self.peek() != Tok::Eq {
                    Stmt::Expr(e)
                } else {
                    let t = &self.toks[self.pos];
                    let bad = Error {
                        message: format!("{}:{}: can only assign to a variable or an array element", t.line, t.col),
                        at_end: false,
                    };
                    self.advance();
                    let value = self.expr(1)?;
                    match e.kind {
                        ExprKind::Var(name) => Stmt::Assign { name, value, line },
                        ExprKind::Index(target, index) => Stmt::IndexAssign { target: *target, index: *index, value, line: e.line },
                        _ => return Err(bad),
                    }
                }
            }
        };
        self.expect(Tok::Semi, "';'")?;
        Ok(s)
    }

    /// `open item, item, ... close`, at most 255 items (arity and argc fit in a byte).
    fn list<T>(&mut self, open: Tok, close: Tok, mut item: impl FnMut(&mut Self) -> Result<T, Error>) -> Result<Vec<T>, Error> {
        let (open_s, close_s) = (format!("{open}"), format!("{close}"));
        self.expect(open, &open_s)?;
        let mut items = Vec::new();
        if *self.peek() != close {
            loop {
                if items.len() == 255 {
                    return Err(self.error("too many items (max 255)"));
                }
                items.push(item(self)?);
                if *self.peek() != Tok::Comma {
                    break;
                }
                self.advance();
            }
        }
        self.expect(close, &close_s)?;
        Ok(items)
    }

    /// After `if`: cond block [else (block | if ...)]
    fn if_rest(&mut self, line: u32) -> Result<Stmt, Error> {
        self.nested(|p| p.if_rest_inner(line))
    }

    fn if_rest_inner(&mut self, line: u32) -> Result<Stmt, Error> {
        let cond = self.expr(1)?;
        let then = self.block()?;
        let els = if *self.peek() != Tok::Else {
            Vec::new()
        } else {
            self.advance();
            if *self.peek() == Tok::If {
                let line = self.line();
                self.advance();
                vec![self.if_rest(line)?]
            } else {
                self.block()?
            }
        };
        Ok(Stmt::If { cond, then, els, line })
    }

    /// Precedence climbing: only consume operators binding at least as tightly as `min`.
    /// Recursing with `p + 1` makes same-precedence operators left-associative.
    fn expr(&mut self, min: u8) -> Result<Expr, Error> {
        let saved = self.depth;
        let r = self.expr_inner(min);
        self.depth = saved;
        r
    }

    /// Each operator in a chain like `1+1+1` deepens the (left-leaning) tree, so it counts as nesting.
    fn expr_inner(&mut self, min: u8) -> Result<Expr, Error> {
        let mut lhs = self.unary()?;
        while let Some((p, op)) = infix(self.peek()) {
            if p < min {
                break;
            }
            if self.depth >= MAX_DEPTH {
                return Err(self.error(&format!("nesting too deep (max {MAX_DEPTH})")));
            }
            self.depth += 1;
            let line = self.line();
            self.advance();
            let rhs = self.expr(p + 1)?;
            lhs = Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), line };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, Error> {
        self.nested(Self::unary_inner)
    }

    fn unary_inner(&mut self) -> Result<Expr, Error> {
        let line = self.line();
        let kind = match self.peek() {
            Tok::Minus => {
                self.advance();
                ExprKind::Unary(UnOp::Neg, Box::new(self.unary()?))
            }
            // Like Python, `not` binds looser than comparisons but tighter than `and`:
            // `not a < b` is `not (a < b)`.
            Tok::Not => {
                self.advance();
                ExprKind::Unary(UnOp::Not, Box::new(self.expr(3)?))
            }
            _ => return self.call(),
        };
        Ok(Expr { kind, line })
    }

    /// primary followed by any number of `(args)` and `[index]`, so `f(1)(2)` and
    /// `m[1][0]` parse. Each one deepens the tree, so it counts as nesting.
    fn call(&mut self) -> Result<Expr, Error> {
        let mut e = self.primary()?;
        while matches!(self.peek(), Tok::LParen | Tok::LBracket) {
            if self.depth >= MAX_DEPTH {
                return Err(self.error(&format!("nesting too deep (max {MAX_DEPTH})")));
            }
            self.depth += 1;
            let line = self.line();
            let kind = if *self.peek() == Tok::LParen {
                ExprKind::Call(Box::new(e), self.list(Tok::LParen, Tok::RParen, |p| p.expr(1))?)
            } else {
                self.advance();
                let index = self.expr(1)?;
                self.expect(Tok::RBracket, "']'")?;
                ExprKind::Index(Box::new(e), Box::new(index))
            };
            e = Expr { kind, line };
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, Error> {
        let line = self.line();
        let kind = match self.peek() {
            Tok::Int(n) => {
                let n = *n;
                self.advance();
                ExprKind::Int(n)
            }
            Tok::True | Tok::False => ExprKind::Bool(self.advance() == Tok::True),
            Tok::Nil => {
                self.advance();
                ExprKind::Nil
            }
            Tok::Str(_) => match self.advance() {
                Tok::Str(s) => ExprKind::Str(s),
                _ => unreachable!(),
            },
            Tok::Ident(_) => ExprKind::Var(self.ident()?),
            Tok::LBracket => ExprKind::Array(self.list(Tok::LBracket, Tok::RBracket, |p| p.expr(1))?),
            Tok::LParen => {
                self.advance();
                let e = self.expr(1)?;
                self.expect(Tok::RParen, "')'")?;
                return Ok(e);
            }
            _ => return Err(self.error("expected expression")),
        };
        Ok(Expr { kind, line })
    }
}
