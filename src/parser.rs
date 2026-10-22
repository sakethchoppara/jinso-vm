use crate::ast::{BinOp, Expr, ExprKind, Stmt, UnOp};
use crate::lexer::{Tok, Token};

/// Parses the whole program, collecting every syntax error instead of stopping at the first.
pub fn parse(toks: Vec<Token>) -> Result<Vec<Stmt>, Vec<String>> {
    let mut p = Parser { toks, pos: 0, errors: Vec::new(), depth: 0 };
    let stmts = p.stmts_until(Tok::Eof);
    if p.errors.is_empty() { Ok(stmts) } else { Err(p.errors) }
}

/// Bounds AST depth, so recursion in the parser, compiler and AST drop can't overflow
/// the Rust stack on input like `((((...` or `1+1+...+1`.
const MAX_DEPTH: usize = 128;

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    errors: Vec<String>,
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

    fn error(&self, msg: &str) -> String {
        let t = &self.toks[self.pos];
        format!("{}:{}: {msg}, got {}", t.line, t.col, t.tok)
    }

    fn expect(&mut self, want: Tok, what: &str) -> Result<(), String> {
        if *self.peek() == want {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!("expected {what}")))
        }
    }

    fn ident(&mut self) -> Result<String, String> {
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
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, String>) -> Result<T, String> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error(&format!("nesting too deep (max {MAX_DEPTH})")));
        }
        self.depth += 1;
        let r = f(self);
        self.depth -= 1;
        r
    }

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(Tok::LBrace, "'{'")?;
        let stmts = self.stmts_until(Tok::RBrace);
        self.expect(Tok::RBrace, "'}'")?;
        Ok(stmts)
    }

    fn stmt(&mut self) -> Result<Stmt, String> {
        self.nested(Self::stmt_inner)
    }

    fn stmt_inner(&mut self) -> Result<Stmt, String> {
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
                let params = self.list(|p| p.ident())?;
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
            // `x = ...` is an assignment; anything else starting with an identifier is an expression.
            Tok::Ident(_) if self.toks[self.pos + 1].tok == Tok::Eq => {
                let name = self.ident()?;
                self.advance();
                Stmt::Assign { name, value: self.expr(1)?, line }
            }
            _ => Stmt::Expr(self.expr(1)?),
        };
        self.expect(Tok::Semi, "';'")?;
        Ok(s)
    }

    /// `( item, item, ... )`, at most 255 items (arity and argc fit in a byte).
    fn list<T>(&mut self, mut item: impl FnMut(&mut Self) -> Result<T, String>) -> Result<Vec<T>, String> {
        self.expect(Tok::LParen, "'('")?;
        let mut items = Vec::new();
        if *self.peek() != Tok::RParen {
            loop {
                if items.len() == 255 {
                    return Err(self.error("too many parameters or arguments (max 255)"));
                }
                items.push(item(self)?);
                if *self.peek() != Tok::Comma {
                    break;
                }
                self.advance();
            }
        }
        self.expect(Tok::RParen, "')'")?;
        Ok(items)
    }

    /// After `if`: cond block [else (block | if ...)]
    fn if_rest(&mut self, line: u32) -> Result<Stmt, String> {
        self.nested(|p| p.if_rest_inner(line))
    }

    fn if_rest_inner(&mut self, line: u32) -> Result<Stmt, String> {
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
    fn expr(&mut self, min: u8) -> Result<Expr, String> {
        let saved = self.depth;
        let r = self.expr_inner(min);
        self.depth = saved;
        r
    }

    /// Each operator in a chain like `1+1+1` deepens the (left-leaning) tree, so it counts as nesting.
    fn expr_inner(&mut self, min: u8) -> Result<Expr, String> {
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

    fn unary(&mut self) -> Result<Expr, String> {
        self.nested(Self::unary_inner)
    }

    fn unary_inner(&mut self) -> Result<Expr, String> {
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

    /// primary followed by any number of `(args)`, so `f(1)(2)` parses.
    fn call(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        while *self.peek() == Tok::LParen {
            let line = self.line();
            let args = self.list(|p| p.expr(1))?;
            e = Expr { kind: ExprKind::Call(Box::new(e), args), line };
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
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
