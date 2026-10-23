use crate::value::Value;

macro_rules! opcodes {
    ($($name:ident),* $(,)?) => {
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub enum Op { $($name),* }

        impl Op {
            /// Indexed by opcode byte, so decoding is `Op::ALL[byte]`.
            pub const ALL: &[Op] = &[$(Op::$name),*];
        }
    };
}

opcodes! {
    Constant,     // u16 constant index
    GetGlobal,    // u16 global index
    SetGlobal,    // u16 global index
    DefineGlobal, // u16 global index
    GetLocal,     // u16 stack slot
    SetLocal,     // u16 stack slot
    Jump,         // u16 forward offset
    JumpIfFalse,  // u16 forward offset; pops the condition, which must be a bool
    // Compare-and-branch for `if a < b` / `while a < b`: pop two ints, jump (u16 forward
    // offset) unless the comparison holds. Saves pushing and popping a bool.
    JumpIfNotLt,
    JumpIfNotLe,
    JumpIfNotGt,
    JumpIfNotGe,
    Loop,         // u16 backward offset
    Pop,
    Call,         // u16 argument count; callee and args are on the stack
    Closure,      // u16 function index; captures per that function's upvalue descriptors
    GetUpvalue,   // u16 index into the running closure's upvalues
    SetUpvalue,   // u16 index into the running closure's upvalues
    CloseUpvalue, // closes upvalues pointing at the top slot, then pops it
    MakeArray,    // u16 element count; elements are on the stack
    Index,        // target index -> element
    SetIndex,     // target index value ->
    Return,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // `Constant c` fused with the arithmetic after it, by the optimizer's fuse pass:
    // u16 constant index; the left operand is on the stack.
    AddConst,
    SubConst,
    MulConst,
    DivConst,
    ModConst,
    Neg,
    Not,
    Eq,
    Lt,
    Gt,
    Print,
    Bad, // never emitted; marks offsets in the VM's decoded code that aren't instruction starts
}

impl Op {
    /// Bytes of operand following the opcode.
    pub const fn operand_len(self) -> usize {
        match self {
            Op::Constant
            | Op::GetGlobal
            | Op::SetGlobal
            | Op::DefineGlobal
            | Op::GetLocal
            | Op::SetLocal
            | Op::Jump
            | Op::JumpIfFalse
            | Op::JumpIfNotLt
            | Op::JumpIfNotLe
            | Op::JumpIfNotGt
            | Op::JumpIfNotGe
            | Op::Loop
            | Op::Call
            | Op::Closure
            | Op::GetUpvalue
            | Op::SetUpvalue
            | Op::MakeArray
            | Op::AddConst
            | Op::SubConst
            | Op::MulConst
            | Op::DivConst
            | Op::ModConst => 2,
            _ => 0,
        }
    }
}

/// Compiled program: bytecode, a parallel line table (one entry per byte), and the
/// constant pool. Globals are resolved to indices at compile time; names are kept
/// only for error messages and the disassembler.
pub struct FnInfo {
    pub name: String,
    pub arity: u8,
    pub offset: usize, // where the body's bytecode starts
    pub end: usize,    // one past the body's last byte
    pub upvalues: Vec<UpvalueDesc>,
}

/// How a closure finds one captured variable when it is created.
pub struct UpvalueDesc {
    /// true: a local of the enclosing function (stack slot `index` of its frame).
    /// false: upvalue `index` of the enclosing closure.
    pub is_local: bool,
    pub index: u16,
    pub name: String,
}

#[derive(Default)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub lines: Vec<u32>,
    pub constants: Vec<Value>,
    pub global_names: Vec<String>,
    pub functions: Vec<FnInfo>,
    /// String literals; the VM seeds its heap with these so `Value::Str { h: i, .. }` constants are valid.
    pub strings: Vec<String>,
}

impl Chunk {
    /// Like Display, but resolves function names and string literals (not runtime strings).
    pub fn show(&self, v: Value) -> String {
        match v {
            Value::Fn(i) => format!("<fn {}>", self.functions[i as usize].name),
            Value::Native(i) => format!("<native {}>", crate::vm::NATIVES[i as usize].0),
            Value::Str { h, .. } => format!("{:?}", self.strings[h as usize]),
            v => v.to_string(),
        }
    }

    pub fn read_u16(&self, at: usize) -> u16 {
        u16::from_be_bytes([self.code[at], self.code[at + 1]])
    }
}
