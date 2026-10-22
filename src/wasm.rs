//! WebAssembly entry point for the browser playground (web/). No wasm-bindgen: JS copies
//! the source into memory from `alloc` and calls `run`, which reports back through one
//! imported function, `emit(kind, ptr, len)`. Program output is emitted line by line while
//! the VM runs, so the page still shows it if the program is later stopped.

use std::io::{self, Write};

// Naming the import module makes this an explicit import. Without it, newer linkers
// treat `emit` as an undefined symbol and fail the build.
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn emit(kind: u32, ptr: *const u8, len: usize);
}

// `kind` values for `emit`; web/worker.js has the same list.
const OUTPUT: u32 = 0;
const ERROR: u32 = 1;
const BYTECODE: u32 = 2;
const AST: u32 = 3;
const GC: u32 = 4;

/// Keeps `while true { print 1; }` from flooding the page with messages.
const MAX_LINES: usize = 10_000;

fn send(kind: u32, bytes: &[u8]) {
    unsafe { emit(kind, bytes.as_ptr(), bytes.len()) }
}

/// Forwards program output to JS one batch of complete lines at a time.
struct JsOut {
    pending: Vec<u8>,
    lines: usize,
}

impl Write for JsOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.lines += buf.iter().filter(|&&b| b == b'\n').count();
        if self.lines > MAX_LINES {
            self.pending.clear(); // drop the partial line so the error starts on its own line
            return Err(io::Error::other(format!("output limit reached ({MAX_LINES} lines)")));
        }
        self.pending.extend_from_slice(buf);
        if let Some(end) = self.pending.iter().rposition(|&b| b == b'\n') {
            let rest = self.pending.split_off(end + 1);
            send(OUTPUT, &self.pending);
            self.pending = rest;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            send(OUTPUT, &self.pending);
            self.pending.clear();
        }
        Ok(())
    }
}

/// Reserves `len` bytes for JS to write the source into; `run` takes ownership.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Compiles and runs the source written at `ptr`, emitting the syntax tree and bytecode
/// first, then output as it is printed, then the error or GC stats.
///
/// # Safety
/// `ptr` must come from `alloc(len)` and hold `len` written bytes; it is freed here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn run(ptr: *mut u8, len: usize, gc_stress: u32) {
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let src = String::from_utf8_lossy(&bytes);

    let stmts = match crate::parse(&src) {
        Ok(stmts) => stmts,
        Err(e) => return send(ERROR, e.as_bytes()),
    };
    let ast: String = stmts.iter().map(|s| format!("{s}\n")).collect();
    send(AST, ast.as_bytes());

    let chunk = match crate::compiler::compile(&stmts) {
        Ok(chunk) => chunk,
        Err(e) => return send(ERROR, e.as_bytes()),
    };
    send(BYTECODE, crate::disasm::disassemble(&chunk).as_bytes());

    let mut out = JsOut { pending: Vec::new(), lines: 0 };
    let result = crate::vm::run(&chunk, &mut out, gc_stress != 0);
    let _ = out.flush();
    match result {
        Err(e) => send(ERROR, e.as_bytes()),
        // Programs that never touch the heap have nothing to report.
        Ok(s) if s.collections == 0 && s.live == 0 => {}
        Ok(s) => send(GC, format!("{} GC cycles, {} objects freed, {} live", s.collections, s.freed, s.live).as_bytes()),
    }
}
