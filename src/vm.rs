use crate::chunk::{Chunk, Op};
use crate::value::Value;
use std::io::Write;

/// Bounds recursion depth; past this, a user program gets "stack overflow" instead of eating memory.
const MAX_FRAMES: usize = 4096;

struct Frame {
    return_ip: usize,
    base: usize, // caller's base, restored on return
}

/// Fetch-decode-execute loop. User errors come back as `Err("line N: ...")`; the VM
/// never panics on a user program. The `unwrap`s on pop are compiler invariants
/// (every pop has a matching push), so hitting one is a compiler bug.
pub fn run(chunk: &Chunk, out: &mut impl Write) -> Result<(), String> {
    let mut stack: Vec<Value> = Vec::with_capacity(256);
    let mut globals: Vec<Option<Value>> = vec![None; chunk.global_names.len()];
    let mut frames: Vec<Frame> = Vec::new();
    let mut ip = 0;
    // Stack index of the current function's slot 0 (its first argument); 0 at top level.
    let mut base = 0;

    while ip < chunk.code.len() {
        let at = ip;
        let op = Op::ALL[chunk.code[ip] as usize];
        ip += 1 + op.operand_len();
        let arg = || chunk.read_u16(at + 1) as usize;
        let err = |msg: String| format!("line {}: {msg}", chunk.lines[at]);

        match op {
            Op::Constant => stack.push(chunk.constants[arg()]),
            Op::GetGlobal => match globals[arg()] {
                Some(v) => stack.push(v),
                None => return Err(err(format!("undefined variable '{}'", chunk.global_names[arg()]))),
            },
            Op::SetGlobal => {
                if globals[arg()].is_none() {
                    return Err(err(format!("undefined variable '{}'", chunk.global_names[arg()])));
                }
                globals[arg()] = stack.pop();
            }
            Op::DefineGlobal => globals[arg()] = stack.pop(),
            Op::GetLocal => stack.push(stack[base + arg()]),
            Op::SetLocal => stack[base + arg()] = stack.pop().unwrap(),
            Op::Jump => ip += arg(),
            Op::Loop => ip -= arg(),
            Op::JumpIfFalse => match stack.last().unwrap() {
                Value::Bool(true) => {}
                Value::Bool(false) => ip += arg(),
                v => return Err(err(format!("condition must be a bool, got {}", v.type_name()))),
            },
            Op::Pop => {
                stack.pop();
            }
            // Stack before: [.. callee arg0 .. argN-1]. The new frame's base points at arg0,
            // so parameters are simply its first locals.
            Op::Call => {
                let argc = arg();
                let callee = stack[stack.len() - 1 - argc];
                let Value::Fn(f) = callee else {
                    return Err(err(format!("can only call functions, got {}", callee.type_name())));
                };
                let info = &chunk.functions[f as usize];
                if argc != info.arity as usize {
                    return Err(err(format!("{}() takes {} argument(s), got {argc}", info.name, info.arity)));
                }
                if frames.len() == MAX_FRAMES {
                    return Err(err(format!("stack overflow (more than {MAX_FRAMES} nested calls)")));
                }
                frames.push(Frame { return_ip: ip, base });
                base = stack.len() - argc;
                ip = info.offset;
            }
            // Drop the callee, args and locals in one go, then leave the return value in the callee's place.
            Op::Return => {
                let v = stack.pop().unwrap();
                stack.truncate(base - 1);
                stack.push(v);
                let frame = frames.pop().unwrap();
                ip = frame.return_ip;
                base = frame.base;
            }
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Mod | Op::Lt | Op::Gt => {
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                let (Value::Int(x), Value::Int(y)) = (a, b) else {
                    return Err(err(format!("operands must be ints, got {} and {}", a.type_name(), b.type_name())));
                };
                let result = match op {
                    Op::Add => x.checked_add(y).map(Value::Int),
                    Op::Sub => x.checked_sub(y).map(Value::Int),
                    Op::Mul => x.checked_mul(y).map(Value::Int),
                    Op::Div | Op::Mod if y == 0 => return Err(err("division by zero".into())),
                    Op::Div => x.checked_div(y).map(Value::Int),
                    Op::Mod => x.checked_rem(y).map(Value::Int),
                    Op::Lt => Some(Value::Bool(x < y)),
                    Op::Gt => Some(Value::Bool(x > y)),
                    _ => unreachable!(),
                };
                stack.push(result.ok_or_else(|| err("integer overflow".into()))?);
            }
            Op::Neg => match stack.pop().unwrap() {
                Value::Int(n) => stack.push(Value::Int(n.checked_neg().ok_or_else(|| err("integer overflow".into()))?)),
                v => return Err(err(format!("operand of '-' must be an int, got {}", v.type_name()))),
            },
            Op::Not => match stack.pop().unwrap() {
                Value::Bool(b) => stack.push(Value::Bool(!b)),
                v => return Err(err(format!("operand of 'not' must be a bool, got {}", v.type_name()))),
            },
            Op::Eq => {
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                stack.push(Value::Bool(a == b));
            }
            Op::Print => writeln!(out, "{}", chunk.show(stack.pop().unwrap())).map_err(|e| e.to_string())?,
        }
    }
    debug_assert!(stack.is_empty(), "compiler left values on the stack: {stack:?}");
    Ok(())
}
