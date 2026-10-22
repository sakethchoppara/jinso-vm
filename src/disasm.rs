use crate::chunk::{Chunk, Op};
use std::fmt::Write;

/// One instruction per line: byte offset, source line ("|" if same as previous), opcode, operand.
pub fn disassemble(chunk: &Chunk) -> String {
    let mut s = String::new();
    let mut ip = 0;
    while ip < chunk.code.len() {
        let op = Op::ALL[chunk.code[ip] as usize];
        if let Some(f) = chunk.functions.iter().find(|f| f.offset == ip) {
            writeln!(s, "== fn {} ==", f.name).unwrap();
        }
        let line = chunk.lines[ip];
        if ip > 0 && chunk.lines[ip - 1] == line {
            write!(s, "{ip:04}    | ").unwrap();
        } else {
            write!(s, "{ip:04} {line:4} ").unwrap();
        }
        write!(s, "{:<13}", format!("{op:?}")).unwrap();
        if op.operand_len() == 2 {
            let i = chunk.read_u16(ip + 1) as usize;
            match op {
                Op::Constant => write!(s, "{i:4}  ({})", chunk.show(chunk.constants[i])),
                Op::GetLocal | Op::SetLocal | Op::Call => write!(s, "{i:4}"),
                Op::Jump | Op::JumpIfFalse => write!(s, "{i:4}  (-> {:04})", ip + 3 + i),
                Op::Loop => write!(s, "{i:4}  (-> {:04})", ip + 3 - i),
                _ => write!(s, "{i:4}  ({})", chunk.global_names[i]),
            }
            .unwrap();
        }
        s.push('\n');
        ip += 1 + op.operand_len();
    }
    s
}
