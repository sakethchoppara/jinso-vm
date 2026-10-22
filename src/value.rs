use std::fmt;

// Plain enum for now (16 bytes per value); see docs/decisions on NaN-boxing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Fn(u16),  // index into Chunk::functions
    Str { h: u32, len: u32 }, // view of the first len bytes of heap buffer h; compare via the heap
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Fn(_) => "fn",
            Value::Str { .. } => "str",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Fn(i) => write!(f, "<fn #{i}>"), // Chunk::show prints the name
            Value::Str { h, .. } => write!(f, "<str #{h}>"), // contents live in the heap
        }
    }
}
