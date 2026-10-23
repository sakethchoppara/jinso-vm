//! Helpers shared by every test file.
#![allow(dead_code)] // each test file uses a different subset

use jinso::optimize::Passes;

pub fn run_at(src: &str, passes: Passes, gc_stress: bool) -> Result<(String, jinso::gc::Stats), String> {
    let chunk = jinso::compile_with(src, passes)?;
    let mut out = Vec::new();
    let stats = jinso::vm::run(&chunk, &mut out, gc_stress)?;
    Ok((String::from_utf8(out).unwrap(), stats))
}

/// Optimized (`-O1`), like `jinso run`.
pub fn run_with(src: &str, gc_stress: bool) -> Result<(String, jinso::gc::Stats), String> {
    run_at(src, Passes::ALL, gc_stress)
}

/// The reference: `-O0` on a VM without superinstructions.
fn run_reference(src: &str) -> Result<String, String> {
    let chunk = jinso::compile_with(src, Passes::NONE)?;
    let mut out = Vec::new();
    jinso::vm::run_reference(&chunk, &mut out, false, u64::MAX)?;
    Ok(String::from_utf8(out).unwrap())
}

/// Runs every program five ways: the reference (unoptimized, no superinstructions), then
/// `-O0` and `-O1` with superinstructions, each normally and with a collection before every
/// allocation. All must agree on the output and on the error, including its line: the
/// reference is the specification, and a missing GC root shows up as a "dangling handle"
/// panic under stress.
pub fn run(src: &str) -> Result<String, String> {
    let reference = run_reference(src);
    let o0 = run_at(src, Passes::NONE, false).map(|(out, _)| out);
    assert_eq!(reference, o0, "superinstructions change the result of {src:?}");
    for (passes, stress) in [(Passes::NONE, true), (Passes::ALL, false), (Passes::ALL, true)] {
        let other = run_at(src, passes, stress).map(|(out, _)| out);
        assert_eq!(reference, other, "-O{} (gc_stress={stress}) differs from -O0 on {src:?}", (passes == Passes::ALL) as u8);
    }
    reference
}

/// The S-expression for `print <expr>;`.
pub fn ast(expr: &str) -> String {
    jinso::parse(&format!("print {expr};")).unwrap()[0].to_string()
}

/// The optimized disassembly.
pub fn dis(src: &str) -> String {
    jinso::disasm::disassemble(&jinso::compile(src).unwrap())
}

pub fn dis_at(src: &str, passes: Passes) -> String {
    jinso::disasm::disassemble(&jinso::compile_with(src, passes).unwrap())
}
