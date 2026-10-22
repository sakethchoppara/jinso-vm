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
    Loop,         // u16 backward offset
    Pop,
    Call,         // u16 argument count; callee and args are on the stack
    MakeArray,    // u16 element count; elements are on the stack
    Index,        // target index -> element
    SetIndex,     // target index value ->
    Return,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    Not,
    Eq,
    Lt,
    Gt,
    Print,
}

impl Op {
    /// Bytes of operand following the opcode.
    pub fn operand_len(self) -> usize {
        match self {
            Op::Constant
            | Op::GetGlobal
            | Op::SetGlobal
            | Op::DefineGlobal
            | Op::GetLocal
            | Op::SetLocal
            | Op::Jump
            | Op::JumpIfFalse
            | Op::Loop
            | Op::Call
            | Op::MakeArray => 2,
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
    pub fn emit(&mut self, op: Op, line: u32) {
        self.code.push(op as u8);
        self.lines.push(line);
    }

    pub fn emit_arg(&mut self, op: Op, arg: u16, line: u32) {
        self.emit(op, line);
        self.code.extend(arg.to_be_bytes());
        self.lines.extend([line, line]);
    }

    /// Emits a forward jump with a placeholder offset; returns where to patch it.
    pub fn emit_jump(&mut self, op: Op, line: u32) -> usize {
        self.emit_arg(op, u16::MAX, line);
        self.code.len() - 2
    }

    /// Backpatches a forward jump to land on the next instruction to be emitted.
    /// The offset is relative to the end of the jump instruction (where ip is after decoding it).
    pub fn patch_jump(&mut self, at: usize) -> Result<(), String> {
        let offset = u16::try_from(self.code.len() - (at + 2)).map_err(|_| "jump too large".to_string())?;
        self.code[at..at + 2].copy_from_slice(&offset.to_be_bytes());
        Ok(())
    }

    /// Emits a backward jump to `start`.
    pub fn emit_loop(&mut self, start: usize, line: u32) -> Result<(), String> {
        let offset = u16::try_from(self.code.len() + 3 - start).map_err(|_| format!("line {line}: loop body too large"))?;
        self.emit_arg(Op::Loop, offset, line);
        Ok(())
    }

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
