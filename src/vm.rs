use crate::chunk::{Chunk, Op};
use crate::gc::{Heap, Stats, Upvalue};
use crate::upvalues::OpenUpvalues;
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

// Int arithmetic shared by `Add` and `AddConst` (and so on), so both fail the same way.
fn add(x: i64, y: i64) -> Result<Value, &'static str> {
    x.checked_add(y).map(Value::Int).ok_or("integer overflow")
}
fn sub(x: i64, y: i64) -> Result<Value, &'static str> {
    x.checked_sub(y).map(Value::Int).ok_or("integer overflow")
}
fn mul(x: i64, y: i64) -> Result<Value, &'static str> {
    x.checked_mul(y).map(Value::Int).ok_or("integer overflow")
}
fn div(x: i64, y: i64) -> Result<Value, &'static str> {
    if y == 0 { Err("division by zero") } else { x.checked_div(y).map(Value::Int).ok_or("integer overflow") }
}
fn rem(x: i64, y: i64) -> Result<Value, &'static str> {
    if y == 0 { Err("division by zero") } else { x.checked_rem(y).map(Value::Int).ok_or("integer overflow") }
}

/// Why `target[i]` failed. Only reached on an error: `Index` and `SetIndex` check the
/// common case inline, and keeping the formatting out of the main loop keeps it fast.
#[cold]
#[inline(never)]
fn index_error(heap: &Heap, target: Value, i: Value) -> String {
    let Value::Array(h) = target else {
        return format!("cannot index a value of type {}", target.type_name());
    };
    let Value::Int(i) = i else {
        return format!("array index must be an int, got {}", i.type_name());
    };
    format!("index {i} out of bounds for array of length {}", heap.array(h).len())
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
        Value::Closure(h) => s.push_str(&chunk.show(Value::Fn(heap.closure(h).0))),
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

/// One instruction, decoded once before the program runs.
#[derive(Clone, Copy)]
struct Instr {
    op: Op,
    arg: u16,  // the u16 operand, or 0 for opcodes without one
    next: u32, // byte offset of the following instruction
}

/// Decodes the bytecode into one `Instr` per byte offset, so `ip`, jump offsets and
/// function addresses keep meaning exactly what they mean in `Chunk::code`. Only offsets
/// where an instruction starts are filled; the rest hold `Op::Bad` and are never reached.
/// This moves the opcode lookup and the two-byte operand read out of the main loop.
/// Decodes only what `out` doesn't have yet, since the REPL keeps appending code.
fn predecode(code: &[u8], out: &mut Vec<Instr>) {
    let mut ip = out.len();
    out.resize(code.len(), Instr { op: Op::Bad, arg: 0, next: 0 });
    while ip < code.len() {
        let op = Op::ALL[code[ip] as usize];
        let next = ip + 1 + op.operand_len();
        let arg = if op.operand_len() == 2 { u16::from_be_bytes([code[ip + 1], code[ip + 2]]) } else { 0 };
        out[ip] = Instr { op, arg, next: next as u32 };
        ip = next;
    }
}

/// Bounds recursion depth; past this, a user program gets "stack overflow" instead of eating memory.
const MAX_FRAMES: usize = 4096;

struct Frame {
    return_ip: usize,
    base: usize,             // caller's base, restored on return
    closure: Option<u32>,    // caller's closure, restored on return
}

/// Runs a whole program in a fresh VM.
pub fn run(chunk: &Chunk, out: &mut impl Write, gc_stress: bool) -> Result<Stats, String> {
    run_limited(chunk, out, gc_stress, u64::MAX)
}

/// `run`, but stops with an error after `steps` loop iterations plus function calls. Those
/// are the only ways a program runs long (everything else moves forward through finite
/// code), so a budget on them bounds any program. The fuzzer uses it to tell a program
/// that never ends from a hang in the VM.
pub fn run_limited(chunk: &Chunk, out: &mut impl Write, gc_stress: bool, steps: u64) -> Result<Stats, String> {
    let mut vm = Vm::new(gc_stress);
    vm.run(chunk, 0, out, steps)?;
    Ok(vm.stats())
}

/// The VM's state that outlives one run. The REPL keeps a `Vm` across inputs, so globals,
/// functions and everything on the heap stay alive; `vm::run` uses a fresh one.
///
/// GC roots are the value stack (which also holds every frame's arguments and locals,
/// and each frame's callee, so every running closure), the globals, the constant pool and
/// the open upvalues. Marking follows array elements, closures' upvalues and closed
/// upvalues' values from there.
pub struct Vm {
    heap: Heap,
    stack: Vec<Value>,
    globals: Vec<Option<Value>>,
    open: OpenUpvalues,
    /// The chunk's constants, with string literals turned into heap handles.
    constants: Vec<Value>,
    /// The heap handle of each of the chunk's string literals loaded so far.
    literals: Vec<u32>,
    /// `Chunk::code`, decoded.
    code: Vec<Instr>,
}

impl Vm {
    pub fn new(gc_stress: bool) -> Vm {
        Vm {
            heap: Heap::new(gc_stress),
            stack: Vec::with_capacity(256),
            globals: Vec::new(),
            open: OpenUpvalues::default(),
            constants: Vec::new(),
            literals: Vec::new(),
            code: Vec::new(),
        }
    }

    pub fn stats(&self) -> Stats {
        self.heap.stats()
    }

    /// Runs `chunk` from byte `start` to its end, after loading whatever the chunk gained
    /// since the last run: string literals into the heap, constants, global slots, and
    /// the new code, decoded.
    ///
    /// User errors come back as `Err("line N: ...")`; the VM never panics on a user
    /// program. After an error the value stack and call frames are reset to the top level,
    /// but the heap and globals are kept: values defined before the error stay valid.
    pub fn run(&mut self, chunk: &Chunk, start: usize, out: &mut impl Write, steps: u64) -> Result<(), String> {
        // Loading allocates without collecting, so nothing new is unrooted in between.
        for s in &chunk.strings[self.literals.len()..] {
            let Value::Str { h, .. } = self.heap.alloc_str(s.clone()) else { unreachable!() };
            self.literals.push(h);
        }
        for &c in &chunk.constants[self.constants.len()..] {
            self.constants.push(match c {
                Value::Str { h, len } => Value::Str { h: self.literals[h as usize], len },
                c => c,
            });
        }
        for name in &chunk.global_names[self.globals.len()..] {
            self.globals.push(NATIVES.iter().position(|(n, ..)| n == name).map(|i| Value::Native(i as u8)));
        }
        predecode(&chunk.code, &mut self.code);

        let result = self.execute(chunk, start, out, steps);
        if result.is_err() {
            // Closures that outlive the failed run keep the values they captured.
            self.open.close(&mut self.heap, &self.stack, 0);
            self.stack.clear();
        }
        result
    }

    /// The fetch-decode-execute loop. The `unwrap`s on pop are compiler invariants (every
    /// pop has a matching push), so hitting one is a compiler bug.
    fn execute(&mut self, chunk: &Chunk, start: usize, out: &mut impl Write, steps: u64) -> Result<(), String> {
        let Vm { heap, stack, globals, open, constants, code, .. } = self;
        // None of these change size during a run, so plain slices let the loop keep their
        // pointers in registers instead of reloading them through `self`.
        let (code, constants, globals) = (&code[..], &constants[..], &mut globals[..]);
        let mut steps = steps;
        let mut frames: Vec<Frame> = Vec::new();
        let mut ip = start;
        // Stack index of the current function's slot 0 (its first argument); 0 at top level.
        let mut base = 0;
        // The running closure, whose upvalues GetUpvalue/SetUpvalue use. None at top level and
        // in functions that capture nothing.
        let mut closure: Option<u32> = None;

        while ip < code.len() {
            let at = ip;
            let Instr { op, arg, next } = code[ip];
            let arg = arg as usize;
            ip = next as usize;
            let err = |msg: String| format!("line {}: {msg}", chunk.lines[at]);

            // Run a collection if one is due. Call this only while every value the current
            // instruction still needs is on the stack, in a global or in the constant pool.
            macro_rules! maybe_collect {
                () => {
                    if heap.should_collect() {
                        let values = stack.iter().chain(globals.iter().flatten()).chain(constants.iter());
                        heap.collect(values.filter_map(|v| v.handle()).chain(open.handles()));
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

            // Pops two strings and pushes their concatenation.
            macro_rules! concat_strings {
                () => {{
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
                }};
            }

            // Pop two ints and jump unless `a OP b` holds.
            macro_rules! cmp_jump {
                ($cmp:tt) => {{
                    let n = stack.len();
                    let (a, b) = (stack[n - 2], stack[n - 1]);
                    let (Value::Int(x), Value::Int(y)) = (a, b) else {
                        return Err(err(format!("operands must be ints, got {} and {}", a.type_name(), b.type_name())));
                    };
                    stack.truncate(n - 2);
                    if !(x $cmp y) {
                        ip += arg;
                    }
                }};
            }

            match op {
                Op::JumpIfNotLt => cmp_jump!(<),
                Op::JumpIfNotLe => cmp_jump!(<=),
                Op::JumpIfNotGt => cmp_jump!(>),
                Op::JumpIfNotGe => cmp_jump!(>=),
                Op::Constant => stack.push(constants[arg]),
                Op::GetGlobal => match globals[arg] {
                    Some(v) => stack.push(v),
                    None => return Err(err(format!("undefined variable '{}'", chunk.global_names[arg]))),
                },
                Op::SetGlobal => {
                    if globals[arg].is_none() {
                        return Err(err(format!("undefined variable '{}'", chunk.global_names[arg])));
                    }
                    globals[arg] = stack.pop();
                }
                Op::DefineGlobal => globals[arg] = stack.pop(),
                Op::GetLocal => stack.push(stack[base + arg]),
                Op::SetLocal => stack[base + arg] = stack.pop().unwrap(),
                Op::Jump => ip += arg,
                Op::Loop => {
                    steps = steps.checked_sub(1).ok_or_else(|| err("step limit reached".into()))?;
                    ip -= arg;
                }
                Op::JumpIfFalse => match stack.pop().unwrap() {
                    Value::Bool(true) => {}
                    Value::Bool(false) => ip += arg,
                    v => return Err(err(format!("condition must be a bool, got {}", v.type_name()))),
                },
                Op::Pop => {
                    stack.pop();
                }
                // Stack before: [.. callee arg0 .. argN-1]. The new frame's base points at arg0,
                // so parameters are simply its first locals.
                Op::Call => {
                    let argc = arg;
                    let callee = stack[stack.len() - 1 - argc];
                    let (f, callee_closure) = match callee {
                        Value::Fn(f) => (f, None),
                        Value::Closure(h) => (heap.closure(h).0, Some(h)),
                        // Built-ins run right here and leave their result in the callee's slot.
                        Value::Native(n) => {
                            let (name, arity, native) = NATIVES[n as usize];
                            if argc != arity {
                                return Err(err(format!("{name}() takes {arity} argument(s), got {argc}")));
                            }
                            let args_at = stack.len() - argc;
                            let v = native(heap, &stack[args_at..]).map_err(err)?;
                            stack.truncate(args_at - 1);
                            stack.push(v);
                            continue;
                        }
                        _ => return Err(err(format!("can only call functions, got {}", callee.type_name()))),
                    };
                    let info = &chunk.functions[f as usize];
                    if argc != info.arity as usize {
                        return Err(err(format!("{}() takes {} argument(s), got {argc}", info.name, info.arity)));
                    }
                    if frames.len() == MAX_FRAMES {
                        return Err(err(format!("stack overflow (more than {MAX_FRAMES} nested calls)")));
                    }
                    steps = steps.checked_sub(1).ok_or_else(|| err("step limit reached".into()))?;
                    frames.push(Frame { return_ip: ip, base, closure });
                    base = stack.len() - argc;
                    closure = callee_closure;
                    ip = info.offset;
                }
                // Capture each variable the function's descriptors name: a slot of this frame
                // (shared with any other closure that captured it) or one of our own upvalues.
                Op::Closure => {
                    maybe_collect!(); // before allocating, while nothing new is unrooted
                    let f = arg;
                    let mut ups = Vec::with_capacity(chunk.functions[f].upvalues.len());
                    for u in &chunk.functions[f].upvalues {
                        ups.push(match u.is_local {
                            true => open.capture(heap, base + u.index as usize),
                            false => heap.closure(closure.unwrap()).1[u.index as usize],
                        });
                    }
                    stack.push(heap.alloc_closure(f as u16, ups));
                }
                Op::GetUpvalue => {
                    let u = heap.closure(closure.unwrap()).1[arg];
                    stack.push(match heap.upvalue(u) {
                        Upvalue::Open(slot) => stack[slot],
                        Upvalue::Closed(v) => v,
                    });
                }
                Op::SetUpvalue => {
                    let v = stack.pop().unwrap();
                    let u = heap.closure(closure.unwrap()).1[arg];
                    match heap.upvalue(u) {
                        Upvalue::Open(slot) => stack[slot] = v,
                        Upvalue::Closed(_) => heap.set_upvalue(u, Upvalue::Closed(v)),
                    }
                }
                // A captured local's block is ending: move its value into the upvalue, then pop.
                Op::CloseUpvalue => {
                    open.close(heap, stack, stack.len() - 1);
                    stack.pop();
                }
                // Drop the callee, args and locals in one go, then leave the return value in the callee's place.
                Op::Return => {
                    let v = stack.pop().unwrap();
                    // Closures that outlive this call keep the captured values.
                    open.close(heap, stack, base);
                    stack.truncate(base - 1);
                    stack.push(v);
                    let frame = frames.pop().unwrap();
                    ip = frame.return_ip;
                    base = frame.base;
                    closure = frame.closure;
                }
                Op::Add if matches!(stack[stack.len() - 2..], [Value::Str { .. }, Value::Str { .. }]) => concat_strings!(),
                // One arm for all five, so the main loop gets one copy of the string path and of
                // the error message, not five: five separate arms made the whole loop about 20%
                // slower in cycles (docs/decisions/0006), even for programs that never used them.
                Op::AddConst | Op::SubConst | Op::MulConst | Op::DivConst | Op::ModConst => {
                    let n = stack.len();
                    let (a, b) = (stack[n - 1], constants[arg]);
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => {
                            let r = match op {
                                Op::AddConst => add(x, y),
                                Op::SubConst => sub(x, y),
                                Op::MulConst => mul(x, y),
                                Op::DivConst => div(x, y),
                                _ => rem(x, y),
                            };
                            stack[n - 1] = r.map_err(|m| err(m.into()))?;
                        }
                        (Value::Str { .. }, Value::Str { .. }) if op == Op::AddConst => {
                            stack.push(b);
                            concat_strings!()
                        }
                        _ => {
                            let want = if op == Op::AddConst { "two ints or two strs" } else { "ints" };
                            return Err(err(format!("operands must be {want}, got {} and {}", a.type_name(), b.type_name())));
                        }
                    }
                }
                Op::MakeArray => {
                    // The elements are still on the stack, so a collection here can't free them.
                    maybe_collect!();
                    let items = stack.split_off(stack.len() - arg);
                    stack.push(heap.alloc_array(items));
                }
                // An array and an in-range int index is the whole fast path; anything else
                // is an error, explained out of line by `index_error`.
                Op::Index => {
                    let n = stack.len();
                    let (target, i) = (stack[n - 2], stack[n - 1]);
                    let found = match (target, i) {
                        (Value::Array(h), Value::Int(i)) => usize::try_from(i).ok().and_then(|i| heap.array(h).get(i).copied()),
                        _ => None,
                    };
                    let Some(v) = found else { return Err(err(index_error(heap, target, i))) };
                    stack[n - 2] = v;
                    stack.pop();
                }
                Op::SetIndex => {
                    let n = stack.len();
                    let (target, i, v) = (stack[n - 3], stack[n - 2], stack[n - 1]);
                    let slot = match (target, i) {
                        (Value::Array(h), Value::Int(i)) => usize::try_from(i).ok().and_then(|i| heap.array_mut(h).get_mut(i)),
                        _ => None,
                    };
                    let Some(slot) = slot else { return Err(err(index_error(heap, target, i))) };
                    *slot = v;
                    stack.truncate(n - 3);
                }
                Op::Add => int_op!(add),
                Op::Sub => int_op!(sub),
                Op::Mul => int_op!(mul),
                Op::Div => int_op!(div),
                Op::Mod => int_op!(rem),
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
                Op::Bad => unreachable!("corrupt bytecode at {at}"),
                Op::Print => {
                    let mut s = String::new();
                    show(&mut s, chunk, heap, stack.pop().unwrap(), &mut Vec::new());
                    writeln!(out, "{s}").map_err(|e| e.to_string())?;
                }
            }
        }
        debug_assert!(stack.is_empty(), "compiler left values on the stack: {stack:?}");
        Ok(())
    }
}
