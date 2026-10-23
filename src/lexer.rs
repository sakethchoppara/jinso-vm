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
    LBracket,
    RBracket,
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
            Tok::LBracket => "[",
            Tok::RBracket => "]",
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
                // Escapes are decoded here, so the token carries the string's real value.
                // Running out of input anywhere inside is an unterminated string, reported
                // at the opening quote; a bad escape is reported at its backslash.
                let unterminated = || format!("{tl}:{tc}: unterminated string");
                let mut s = String::new();
                loop {
                    let Some(&c) = chars.get(i) else { return Err(unterminated()) };
                    i += 1;
                    col += 1;
                    match c {
                        '"' => break,
                        '\\' => {
                            let (el, ec) = (line, col - 1);
                            let Some(&e) = chars.get(i) else { return Err(unterminated()) };
                            i += 1;
                            col += 1;
                            match e {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                'r' => s.push('\r'),
                                '\\' => s.push('\\'),
                                '"' => s.push('"'),
                                '0' => s.push('\0'),
                                'u' => {
                                    if i == chars.len() {
                                        return Err(unterminated());
                                    }
                                    if chars.get(i) != Some(&'{') {
                                        return Err(format!("{el}:{ec}: expected '{{' after \\u"));
                                    }
                                    i += 1;
                                    col += 1;
                                    let digits_at = i;
                                    while chars.get(i).is_some_and(|c| c.is_ascii_hexdigit()) {
                                        i += 1;
                                        col += 1;
                                    }
                                    let hex: String = chars[digits_at..i].iter().collect();
                                    match chars.get(i) {
                                        None => return Err(unterminated()),
                                        Some('}') => {}
                                        Some(_) => return Err(format!("{el}:{ec}: \\u{{...}} needs 1 to 6 hex digits and a closing '}}'")),
                                    }
                                    i += 1;
                                    col += 1;
                                    if hex.is_empty() || hex.len() > 6 {
                                        return Err(format!("{el}:{ec}: \\u{{{hex}}} needs 1 to 6 hex digits"));
                                    }
                                    let code = u32::from_str_radix(&hex, 16).unwrap();
                                    let Some(ch) = char::from_u32(code) else {
                                        return Err(format!("{el}:{ec}: \\u{{{hex}}} is not a valid Unicode scalar value"));
                                    };
                                    s.push(ch);
                                }
                                e => return Err(format!("{el}:{ec}: unknown escape sequence '\\{e}'")),
                            }
                        }
                        '\n' => {
                            line += 1;
                            col = 1;
                            s.push(c);
                        }
                        c => s.push(c),
                    }
                }
                Tok::Str(s)
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
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
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
