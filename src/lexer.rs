#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Str(String),
    Ident(String),
    // keywords
    Let,
    Fn,
    Return,
    If,
    Else,
    While,
    Print,
    True,
    False,
    Nil,
    And,
    Or,
    Not,
    // symbols
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semi,
    Eq,
    EqEq,
    BangEq,
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

impl std::fmt::Display for Tok {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s = match self {
            Tok::Int(n) => return write!(f, "{n}"),
            Tok::Str(s) => return write!(f, "{s:?}"),
            Tok::Ident(name) => return write!(f, "identifier '{name}'"),
            Tok::Eof => return write!(f, "end of file"),
            Tok::Let => "let",
            Tok::Fn => "fn",
            Tok::Return => "return",
            Tok::If => "if",
            Tok::Else => "else",
            Tok::While => "while",
            Tok::Print => "print",
            Tok::True => "true",
            Tok::False => "false",
            Tok::Nil => "nil",
            Tok::And => "and",
            Tok::Or => "or",
            Tok::Not => "not",
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            Tok::Comma => ",",
            Tok::Semi => ";",
            Tok::Eq => "=",
            Tok::EqEq => "==",
            Tok::BangEq => "!=",
            Tok::Lt => "<",
            Tok::Le => "<=",
            Tok::Gt => ">",
            Tok::Ge => ">=",
        };
        write!(f, "'{s}'")
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
}

/// Turns source text into tokens, ending with Eof. Errors are "line:col: message".
pub fn lex(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line, mut col) = (0, 1u32, 1u32);

    while i < chars.len() {
        let (start, tl, tc) = (i, line, col);
        let c = chars[i];
        let peek = chars.get(i + 1).copied();
        i += 1;
        col += 1;

        let tok = match c {
            '\n' => {
                line += 1;
                col = 1;
                continue;
            }
            c if c.is_whitespace() => continue,
            '/' if peek == Some('/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '"' => {
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\n' {
                        line += 1;
                        col = 0;
                    }
                    i += 1;
                    col += 1;
                }
                if i == chars.len() {
                    return Err(format!("{tl}:{tc}: unterminated string"));
                }
                i += 1;
                col += 1;
                Tok::Str(chars[start + 1..i - 1].iter().collect())
            }
            c if c.is_ascii_digit() => {
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                    col += 1;
                }
                let s: String = chars[start..i].iter().collect();
                Tok::Int(s.parse().map_err(|_| format!("{tl}:{tc}: integer literal '{s}' is too large"))?)
            }
            c if c.is_alphabetic() || c == '_' => {
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                    col += 1;
                }
                let s: String = chars[start..i].iter().collect();
                match s.as_str() {
                    "let" => Tok::Let,
                    "fn" => Tok::Fn,
                    "return" => Tok::Return,
                    "if" => Tok::If,
                    "else" => Tok::Else,
                    "while" => Tok::While,
                    "print" => Tok::Print,
                    "true" => Tok::True,
                    "false" => Tok::False,
                    "nil" => Tok::Nil,
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    _ => Tok::Ident(s),
                }
            }
            '=' | '!' | '<' | '>' if peek == Some('=') => {
                i += 1;
                col += 1;
                match c {
                    '=' => Tok::EqEq,
                    '!' => Tok::BangEq,
                    '<' => Tok::Le,
                    _ => Tok::Ge,
                }
            }
            '+' => Tok::Plus,
            '-' => Tok::Minus,
            '*' => Tok::Star,
            '/' => Tok::Slash,
            '%' => Tok::Percent,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            ',' => Tok::Comma,
            ';' => Tok::Semi,
            '=' => Tok::Eq,
            '<' => Tok::Lt,
            '>' => Tok::Gt,
            _ => return Err(format!("{tl}:{tc}: unexpected character '{c}'")),
        };
        out.push(Token { tok, line: tl, col: tc });
    }
    out.push(Token { tok: Tok::Eof, line, col });
    Ok(out)
}
