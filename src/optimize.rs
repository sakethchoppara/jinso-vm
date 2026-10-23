//! The compiler's output before it becomes bytecode: a list of instructions whose jumps
//! name labels instead of byte offsets. The optimizer rewrites this list; `relocate` then
//! lays it out as `Chunk::code` and computes every offset once, at the end.

use crate::chunk::{Chunk, Op};
use crate::value::Value;
use std::collections::HashSet;

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

/// Which optimization passes run. `-O0` is `Passes::NONE`, `-O1` is `Passes::ALL`, and each
/// pass can be switched off on its own (`--no-fold`, ...) to measure what it's worth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Passes {
    pub fold: bool,
    pub cond: bool,
    pub thread: bool,
    pub dead_jumps: bool,
    pub unreachable: bool,
}

impl Passes {
    pub const NONE: Passes = Passes { fold: false, cond: false, thread: false, dead_jumps: false, unreachable: false };
    pub const ALL: Passes = Passes { fold: true, cond: true, thread: true, dead_jumps: true, unreachable: true };
    pub const NAMES: &[&str] = &["fold", "cond", "thread", "deadjump", "unreachable"];

    /// Turns the named pass off; false if there's no such pass.
    pub fn disable(&mut self, name: &str) -> bool {
        match name {
            "fold" => self.fold = false,
            "cond" => self.cond = false,
            "thread" => self.thread = false,
            "deadjump" => self.dead_jumps = false,
            "unreachable" => self.unreachable = false,
            _ => return false,
        }
        true
    }
}

/// Runs the enabled passes until none of them changes anything, at most 10 rounds.
/// `constants` is the chunk's constant pool, which folding adds results to. `entries` are
/// labels reached from outside the instruction stream (function bodies, entered by
/// `Call`), which unreachable-code removal must keep.
pub fn optimize(items: &mut Vec<Item>, constants: &mut Vec<Value>, entries: &[Label], passes: Passes) {
    for _ in 0..10 {
        let mut changed = false;
        if passes.fold {
            changed |= rewrite_tail(items, |tail| fold(tail, constants));
        }
        if passes.cond {
            changed |= rewrite_tail(items, |tail| constant_condition(tail, constants));
        }
        if passes.thread {
            changed |= thread_jumps(items);
        }
        if passes.dead_jumps {
            changed |= remove_dead_jumps(items);
        }
        if passes.unreachable {
            changed |= remove_unreachable(items, entries);
        }
        if !changed {
            break;
        }
    }
}

/// Rebuilds `items`, letting `rule` replace the end of the output after each item is
/// added. `rule` gets the output so far and returns how many trailing items to replace
/// and what with, or None. Running it again after a replacement lets results cascade:
/// `2 * 3 + 1` folds `2 * 3`, then `6 + 1`.
fn rewrite_tail(items: &mut Vec<Item>, mut rule: impl FnMut(&[Item]) -> Option<(usize, Vec<Item>)>) -> bool {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    let mut changed = false;
    for &item in items.iter() {
        out.push(item);
        while let Some((n, with)) = rule(&out) {
            out.truncate(out.len() - n);
            out.extend(with);
            changed = true;
        }
    }
    *items = out;
    changed
}

fn constant(constants: &[Value], item: &Item) -> Option<Value> {
    match item {
        Item::Ins { op: Op::Constant, arg, .. } => Some(constants[*arg as usize]),
        _ => None,
    }
}

/// The index of `v` in the pool, adding it if needed. None if the pool is full, in which
/// case the fold is skipped.
fn intern(constants: &mut Vec<Value>, v: Value) -> Option<u16> {
    if let Some(i) = constants.iter().position(|c| *c == v && !matches!(c, Value::Str { .. })) {
        return u16::try_from(i).ok();
    }
    let i = u16::try_from(constants.len()).ok()?;
    constants.push(v);
    Some(i)
}

/// Constant folding. The stack machine makes this a pattern match on adjacent items:
/// `Constant a; Constant b; Add` always adds exactly a and b. A label anywhere in the
/// pattern means another path can arrive there, so it doesn't match.
///
/// Refusal rules: only int, bool and nil literals fold (strings are a non-goal), and any
/// operation that would fail at runtime (overflow, division by zero, a type error) is left
/// alone so it still fails at runtime with the same message and line. The folded constant
/// takes the operator's line.
fn fold(tail: &[Item], constants: &mut Vec<Value>) -> Option<(usize, Vec<Item>)> {
    let [.., before, last] = tail else { return None };
    let Item::Ins { op, line, .. } = *last else { return None };

    // unary: Constant a; Neg | Not
    if let Some(a) = constant(constants, before) {
        let folded = match (op, a) {
            (Op::Neg, Value::Int(x)) => x.checked_neg().map(Value::Int),
            (Op::Not, Value::Bool(b)) => Some(Value::Bool(!b)),
            _ => None,
        };
        if let Some(v) = folded {
            let arg = intern(constants, v)?;
            return Some((2, vec![Item::Ins { op: Op::Constant, arg, line }]));
        }
    }

    // binary: Constant a; Constant b; op
    let [.., first, second, _] = tail else { return None };
    let (a, b) = (constant(constants, first)?, constant(constants, second)?);
    use Value::{Bool, Int, Nil};
    let folded = match (op, a, b) {
        (Op::Add, Int(x), Int(y)) => x.checked_add(y).map(Int),
        (Op::Sub, Int(x), Int(y)) => x.checked_sub(y).map(Int),
        (Op::Mul, Int(x), Int(y)) => x.checked_mul(y).map(Int),
        (Op::Div, Int(x), Int(y)) => x.checked_div(y).map(Int), // None for /0 and MIN / -1
        (Op::Mod, Int(x), Int(y)) => x.checked_rem(y).map(Int),
        (Op::Lt, Int(x), Int(y)) => Some(Bool(x < y)),
        (Op::Gt, Int(x), Int(y)) => Some(Bool(x > y)),
        (Op::Eq, Int(_) | Bool(_) | Nil, Int(_) | Bool(_) | Nil) => Some(Bool(a == b)),
        _ => None,
    }?;
    let arg = intern(constants, folded)?;
    Some((3, vec![Item::Ins { op: Op::Constant, arg, line }]))
}

/// Branches on literal conditions: `Constant false; JumpIfFalse L` always jumps, so it
/// becomes `Goto L`; with `true` it never jumps, so both go. The fused compare-and-branch
/// does the same when both operands are int literals. A non-bool literal (`if 1 {}`) is
/// left alone so it still fails at runtime.
fn constant_condition(tail: &[Item], constants: &[Value]) -> Option<(usize, Vec<Item>)> {
    let [.., before, last] = tail else { return None };
    let Item::Branch { op, target, line } = *last else { return None };
    let (n, jumps) = match op {
        Op::JumpIfFalse => match constant(constants, before)? {
            Value::Bool(b) => (2, !b),
            _ => return None,
        },
        Op::JumpIfNotLt | Op::JumpIfNotLe | Op::JumpIfNotGt | Op::JumpIfNotGe => {
            let [.., first, _, _] = tail else { return None };
            let (Value::Int(x), Value::Int(y)) = (constant(constants, first)?, constant(constants, before)?) else {
                return None;
            };
            let holds = match op {
                Op::JumpIfNotLt => x < y,
                Op::JumpIfNotLe => x <= y,
                Op::JumpIfNotGt => x > y,
                _ => x >= y,
            };
            (3, !holds)
        }
        _ => return None,
    };
    Some((n, if jumps { vec![Item::Goto { target, line }] } else { vec![] }))
}

/// Index of each label's `Item::Label` in `items`.
fn label_positions(items: &[Item]) -> Vec<usize> {
    let mut pos = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if let Item::Label(l) = *item {
            if pos.len() <= l as usize {
                pos.resize(l as usize + 1, usize::MAX);
            }
            pos[l as usize] = i;
        }
    }
    pos
}

/// Jump threading: a jump to a label whose first instruction is `Goto M` might as well jump
/// to M. Follows chains. A conditional jump only takes a target that's still ahead of it,
/// because the VM's conditional jumps are forward-only; an unconditional one can go either
/// way (relocation turns it into `Loop` if needed).
fn thread_jumps(items: &mut [Item]) -> bool {
    let pos = label_positions(items);
    let goto_at = |l: Label| match items[pos[l as usize]..].iter().find(|it| !matches!(it, Item::Label(_))) {
        Some(Item::Goto { target, .. }) => Some(*target),
        _ => None,
    };
    let mut retarget = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let (target, conditional) = match *item {
            Item::Goto { target, .. } => (target, false),
            Item::Branch { target, .. } => (target, true),
            _ => continue,
        };
        let (mut t, mut best) = (target, target);
        // Bounded, because `L: Goto M; M: Goto L` (an empty infinite loop) is a cycle.
        for _ in 0..items.len() {
            match goto_at(t) {
                Some(next) if next != t => {
                    t = next;
                    if !conditional || pos[t as usize] > i {
                        best = t;
                    }
                }
                _ => break,
            }
        }
        if best != target {
            retarget.push((i, best));
        }
    }
    for &(i, new) in &retarget {
        match &mut items[i] {
            Item::Goto { target, .. } | Item::Branch { target, .. } => *target = new,
            _ => unreachable!(),
        }
    }
    !retarget.is_empty()
}

/// A `Goto` to a label with nothing but labels between them does nothing. (Conditional
/// jumps stay: they still pop and type-check their condition.)
fn remove_dead_jumps(items: &mut Vec<Item>) -> bool {
    let dead: Vec<bool> = (0..items.len())
        .map(|i| match items[i] {
            Item::Goto { target, .. } => items[i + 1..]
                .iter()
                .take_while(|it| matches!(it, Item::Label(_)))
                .any(|it| *it == Item::Label(target)),
            _ => false,
        })
        .collect();
    let mut i = 0;
    items.retain(|_| {
        i += 1;
        !dead[i - 1]
    });
    dead.contains(&true)
}

/// Code after a `Return` or `Goto` can only run if something jumps to it, so everything up
/// to the next label that's actually jumped to (or is a function's entry) is dropped. This
/// also removes the implicit `return nil;` after a function's last `return`. Labels are
/// always kept, since they mark positions (like a function's end) even when unused.
fn remove_unreachable(items: &mut Vec<Item>, entries: &[Label]) -> bool {
    let mut live: HashSet<Label> = entries.iter().copied().collect();
    for item in items.iter() {
        if let Item::Goto { target, .. } | Item::Branch { target, .. } = *item {
            live.insert(target);
        }
    }
    let before = items.len();
    let mut dead = false;
    items.retain(|item| match item {
        Item::Label(l) => {
            if live.contains(l) {
                dead = false;
            }
            true
        }
        _ if dead => false,
        Item::Goto { .. } | Item::Ins { op: Op::Return, .. } => {
            dead = true;
            true
        }
        _ => true,
    });
    items.len() != before
}
