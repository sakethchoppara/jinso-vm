use crate::chunk::{Chunk, Op};
use crate::gc::{Heap, Stats};
use crate::value::Value;
use std::fmt::Write as _;
use std::io::Write;

type NativeFn = fn(&mut Heap, &[Value]) -> Result<Value, String>;

/// Built-in functions: (name, arity, implementation). A global with one of these names
/// starts out bound to it, so a program can still shadow it with its own `let` or `fn`.
pub const NATIVES: &[(&str, usize, NativeFn)] = &[("len", 1, native_len), ("push", 2, native_push)];

fn native_len(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let n = match args[0] {
        Value::Array(h) => heap.array(h).len(),
        Value::Str { h, len } => heap.get(h, len).chars().count(),
        v => return Err(format!("len() expects an array or str, got {}", v.type_name())),
    };
    Ok(Value::Int(n as i64))
}

fn native_push(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let Value::Array(h) = args[0] else {
        return Err(format!("push() expects an array, got {}", args[0].type_name()));
    };
    heap.push(h, args[1]);
    Ok(Value::Nil)
}

/// Checks `target[i]` and returns the array handle and element position.
fn element(heap: &Heap, target: Value, i: Value) -> Result<(u32, usize), String> {
    let Value::Array(h) = target else {
        return Err(format!("cannot index a value of type {}", target.type_name()));
    };
    let Value::Int(i) = i else {
        return Err(format!("array index must be an int, got {}", i.type_name()));
    };
    let len = heap.array(h).len();
    match usize::try_from(i) {
        Ok(at) if at < len => Ok((h, at)),
        _ => Err(format!("index {i} out of bounds for array of length {len}")),
    }
}

/// Arrays nested deeper than this print as `[...]`, so printing never recurses unboundedly.
const MAX_PRINT_DEPTH: usize = 64;

/// Formats a value the way `print` shows it. Strings are raw at the top level and quoted
/// inside arrays. An array that is already being printed (a cycle) shows as `[...]`.
// ponytail: an array that shares one sub-array many times prints it in full each time,
// so output can grow exponentially; cap total output if that ever matters.
fn show(s: &mut String, chunk: &Chunk, heap: &Heap, v: Value, path: &mut Vec<u32>) {
    match v {
        Value::Str { h, len } if path.is_empty() => s.push_str(heap.get(h, len)),
        Value::Str { h, len } => write!(s, "{:?}", heap.get(h, len)).unwrap(),
        Value::Array(h) if path.contains(&h) || path.len() >= MAX_PRINT_DEPTH => s.push_str("[...]"),
        Value::Array(h) => {
            path.push(h);
            s.push('[');
            for (i, &item) in heap.array(h).iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                show(s, chunk, heap, item, path);
            }
            s.push(']');
            path.pop();
        }
        v => s.push_str(&chunk.show(v)),
    }
}

/// Bounds recursion depth; past this, a user program gets "stack overflow" instead of eating memory.
const MAX_FRAMES: usize = 4096;

struct Frame {
    return_ip: usize,
    base: usize, // caller's base, restored on return
}

/// Fetch-decode-execute loop. User errors come back as `Err("line N: ...")`; the VM
/// never panics on a user program. The `unwrap`s on pop are compiler invariants
/// (every pop has a matching push), so hitting one is a compiler bug.
///
/// GC roots are the value stack (which also holds every frame's arguments and locals,
/// since frames only store ip and base), the globals, and the constant pool. Marking
/// follows array elements from there.
pub fn run(chunk: &Chunk, out: &mut impl Write, gc_stress: bool) -> Result<Stats, String> {
    let mut heap = Heap::new(&chunk.strings, gc_stress);
    let mut stack: Vec<Value> = Vec::with_capacity(256);
    let mut globals: Vec<Option<Value>> = chunk
        .global_names
        .iter()
        .map(|name| NATIVES.iter().position(|(n, ..)| n == name).map(|i| Value::Native(i as u8)))
        .collect();
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

        // Run a collection if one is due. Call this only while every value the current
        // instruction still needs is on the stack, in a global or in the constant pool.
        macro_rules! maybe_collect {
            () => {
                if heap.should_collect() {
                    heap.collect(stack.iter().chain(globals.iter().flatten()).chain(&chunk.constants).copied());
                }
            };
        }

        // Int binary op computed in place: the result overwrites the left operand.
        macro_rules! int_op {
            ($f:expr) => {{
                let n = stack.len();
                let (a, b) = (stack[n - 2], stack[n - 1]);
                let (Value::Int(x), Value::Int(y)) = (a, b) else {
                    let want = if op == Op::Add { "two ints or two strs" } else { "ints" };
                    return Err(err(format!("operands must be {want}, got {} and {}", a.type_name(), b.type_name())));
                };
                let f: fn(i64, i64) -> Result<Value, &'static str> = $f;
                stack[n - 2] = f(x, y).map_err(|m| err(m.into()))?;
                stack.pop();
            }};
        }

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
            Op::JumpIfFalse => match stack.pop().unwrap() {
                Value::Bool(true) => {}
                Value::Bool(false) => ip += arg(),
                v => return Err(err(format!("condition must be a bool, got {}", v.type_name()))),
            },
            Op::Pop => {
                stack.pop();
            }
            // Stack before: [.. callee arg0 .. argN-1]. The new frame's base points at arg0,
            // so parameters are simply its first locals.
            Op::Call if matches!(stack[stack.len() - 1 - arg()], Value::Native(_)) => {
                let argc = arg();
                let Value::Native(n) = stack[stack.len() - 1 - argc] else { unreachable!() };
                let (name, arity, f) = NATIVES[n as usize];
                if argc != arity {
                    return Err(err(format!("{name}() takes {arity} argument(s), got {argc}")));
                }
                let args_at = stack.len() - argc;
                let v = f(&mut heap, &stack[args_at..]).map_err(err)?;
                stack.truncate(args_at - 1);
                stack.push(v);
            }
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
            Op::Add if matches!(stack[stack.len() - 2..], [Value::Str { .. }, Value::Str { .. }]) => {
                let Some(Value::Str { h: bh, len: blen }) = stack.pop() else { unreachable!() };
                let Some(Value::Str { h: ah, len: alen }) = stack.pop() else { unreachable!() };
                if alen.checked_add(blen).is_none() {
                    return Err(err("string too long (max 4 GiB)".into()));
                }
                let v = match heap.try_append((ah, alen), (bh, blen)) {
                    Some(v) => v,
                    None => {
                        // Build the result as a plain String first: a and b are already off the
                        // stack (no longer roots), so they must be copied before a collection
                        // can free them.
                        let s = [heap.get(ah, alen), heap.get(bh, blen)].concat();
                        maybe_collect!();
                        heap.alloc_str(s)
                    }
                };
                stack.push(v);
            }
            Op::MakeArray => {
                // The elements are still on the stack, so a collection here can't free them.
                maybe_collect!();
                let items = stack.split_off(stack.len() - arg());
                stack.push(heap.alloc_array(items));
            }
            Op::Index => {
                let i = stack.pop().unwrap();
                let target = stack.pop().unwrap();
                let (h, at) = element(&heap, target, i).map_err(err)?;
                stack.push(heap.array(h)[at]);
            }
            Op::SetIndex => {
                let v = stack.pop().unwrap();
                let i = stack.pop().unwrap();
                let target = stack.pop().unwrap();
                let (h, at) = element(&heap, target, i).map_err(err)?;
                heap.array_mut(h)[at] = v;
            }
            Op::Add => int_op!(|x, y| x.checked_add(y).map(Value::Int).ok_or("integer overflow")),
            Op::Sub => int_op!(|x, y| x.checked_sub(y).map(Value::Int).ok_or("integer overflow")),
            Op::Mul => int_op!(|x, y| x.checked_mul(y).map(Value::Int).ok_or("integer overflow")),
            Op::Div => int_op!(|x, y| match y {
                0 => Err("division by zero"),
                _ => x.checked_div(y).map(Value::Int).ok_or("integer overflow"),
            }),
            Op::Mod => int_op!(|x, y| match y {
                0 => Err("division by zero"),
                _ => x.checked_rem(y).map(Value::Int).ok_or("integer overflow"),
            }),
            Op::Lt => int_op!(|x, y| Ok(Value::Bool(x < y))),
            Op::Gt => int_op!(|x, y| Ok(Value::Bool(x > y))),
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
                let eq = match (a, b) {
                    (Value::Str { h: x, len: xl }, Value::Str { h: y, len: yl }) => heap.get(x, xl) == heap.get(y, yl),
                    _ => a == b,
                };
                stack.push(Value::Bool(eq));
            }
            Op::Print => {
                let mut s = String::new();
                show(&mut s, chunk, &heap, stack.pop().unwrap(), &mut Vec::new());
                writeln!(out, "{s}").map_err(|e| e.to_string())?;
            }
        }
    }
    debug_assert!(stack.is_empty(), "compiler left values on the stack: {stack:?}");
    Ok(heap.stats())
}
