use std::fmt;

// Plain enum for now (16 bytes per value); see docs/decisions on NaN-boxing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Fn(u16),  // index into Chunk::functions
    Str { h: u32, len: u32 }, // view of the first len bytes of heap buffer h; compare via the heap
    Array(u32),               // heap handle; == compares identity
    Closure(u32),             // heap handle of a function plus its captured upvalues
    Native(u8),               // index into vm::NATIVES
}

impl Value {
    /// The heap object this value refers to, if any.
    pub fn handle(self) -> Option<u32> {
        match self {
            Value::Str { h, .. } | Value::Array(h) | Value::Closure(h) => Some(h),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Fn(_) | Value::Native(_) | Value::Closure(_) => "fn",
            Value::Array(_) => "array",
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
            Value::Array(h) => write!(f, "<array #{h}>"),
            Value::Closure(h) => write!(f, "<closure #{h}>"),
            Value::Native(i) => write!(f, "<native #{i}>"),
        }
    }
}
