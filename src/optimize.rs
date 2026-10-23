//! The compiler's output before it becomes bytecode: a list of instructions whose jumps
//! name labels instead of byte offsets. The optimizer rewrites this list; `relocate` then
//! lays it out as `Chunk::code` and computes every offset once, at the end.

use crate::chunk::{Chunk, Op};

pub type Label = u32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Item {
    /// An instruction and its u16 operand (0 for opcodes without one).
    Ins { op: Op, arg: u16, line: u32 },
    /// An unconditional jump. Encoded as `Jump` if the label is ahead, `Loop` if behind,
    /// so the optimizer can retarget it without caring about direction.
    Goto { target: Label, line: u32 },
    /// A conditional jump (`JumpIfFalse`, `JumpIfNotLt`, ...). Always forward.
    Branch { op: Op, target: Label, line: u32 },
    /// Marks the position a label refers to. Emits nothing.
    Label(Label),
}

/// Function `index` in `Chunk::functions` has its body between these two labels.
#[derive(Debug, Clone, Copy)]
pub struct FnLabels {
    pub index: usize,
    pub start: Label,
    pub end: Label,
}

impl Item {
    fn size(&self) -> usize {
        match self {
            Item::Ins { op, .. } => 1 + op.operand_len(),
            Item::Goto { .. } | Item::Branch { .. } => 3,
            Item::Label(_) => 0,
        }
    }
}

/// Appends `items` to `chunk.code` (after whatever is already there, which the REPL relies
/// on), turning labels into offsets and filling in each function's address.
pub fn relocate(items: &[Item], fns: &[FnLabels], chunk: &mut Chunk) -> Result<(), String> {
    let labels = items
        .iter()
        .filter_map(|it| match it {
            Item::Label(l) => Some(*l as usize + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let mut addr = vec![usize::MAX; labels];
    let mut at = chunk.code.len();
    for it in items {
        if let Item::Label(l) = it {
            addr[*l as usize] = at;
        }
        at += it.size();
    }

    for it in items {
        let here = chunk.code.len();
        match *it {
            Item::Ins { op, arg, line } => emit(chunk, op, arg, line),
            Item::Goto { target, line } => {
                let to = addr[target as usize];
                let (op, offset) = if to >= here + 3 { (Op::Jump, to - (here + 3)) } else { (Op::Loop, here + 3 - to) };
                emit(chunk, op, jump_offset(offset, line)?, line);
            }
            Item::Branch { op, target, line } => {
                let to = addr[target as usize];
                assert!(to >= here + 3, "conditional jump at {here} goes backward to {to}");
                emit(chunk, op, jump_offset(to - (here + 3), line)?, line);
            }
            Item::Label(_) => {}
        }
    }

    for f in fns {
        chunk.functions[f.index].offset = addr[f.start as usize];
        chunk.functions[f.index].end = addr[f.end as usize];
    }
    Ok(())
}

fn jump_offset(offset: usize, line: u32) -> Result<u16, String> {
    u16::try_from(offset).map_err(|_| format!("line {line}: jump too large"))
}

fn emit(chunk: &mut Chunk, op: Op, arg: u16, line: u32) {
    chunk.code.push(op as u8);
    chunk.lines.push(line);
    if op.operand_len() == 2 {
        chunk.code.extend(arg.to_be_bytes());
        chunk.lines.extend([line, line]);
    }
}
