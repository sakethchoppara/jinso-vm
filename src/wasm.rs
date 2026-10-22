//! WebAssembly entry point for the browser playground (web/). Plain exports, no wasm-bindgen:
//! JS copies the source into memory from `alloc`, calls `run`, then reads the result
//! from `result_ptr`.

use std::cell::RefCell;

thread_local! {
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Reserves `len` bytes for JS to write the source into; `run` takes ownership.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Compiles and runs the source written at `ptr` (from `alloc(len)`), and returns the byte
/// length of the result: five NUL-separated fields, `output, error, bytecode, ast, gc stats`.
///
/// # Safety
/// `ptr` must come from `alloc(len)` and hold `len` written bytes; it is freed here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn run(ptr: *mut u8, len: usize, gc_stress: u32) -> usize {
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let src = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    let (mut error, mut bytecode, mut ast, mut gc) = (String::new(), String::new(), String::new(), String::new());

    match crate::parse(&src) {
        Err(e) => error = e,
        Ok(stmts) => {
            ast = stmts.iter().map(|s| format!("{s}\n")).collect();
            match crate::compiler::compile(&stmts) {
                Err(e) => error = e,
                Ok(chunk) => {
                    bytecode = crate::disasm::disassemble(&chunk);
                    match crate::vm::run(&chunk, &mut out, gc_stress != 0) {
                        Ok(s) => gc = format!("{} GC cycles, {} strings freed, {} live", s.collections, s.freed, s.live),
                        Err(e) => error = e,
                    }
                }
            }
        }
    }

    let output = String::from_utf8_lossy(&out).into_owned();
    let result = [output, error, bytecode, ast, gc].join("\0");
    RESULT.with(|r| {
        *r.borrow_mut() = result.into_bytes();
        r.borrow().len()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn result_ptr() -> *const u8 {
    RESULT.with(|r| r.borrow().as_ptr())
}
