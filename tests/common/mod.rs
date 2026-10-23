//! Helpers shared by every test file.
#![allow(dead_code)] // each test file uses a different subset

pub fn run_with(src: &str, gc_stress: bool) -> Result<(String, jinso::gc::Stats), String> {
    let chunk = jinso::compile(src)?;
    let mut out = Vec::new();
    let stats = jinso::vm::run(&chunk, &mut out, gc_stress)?;
    Ok((String::from_utf8(out).unwrap(), stats))
}

/// Runs every program twice, normally and with a collection before every allocation;
/// both must agree. A missing GC root shows up as a "dangling handle" panic under stress.
pub fn run(src: &str) -> Result<String, String> {
    let normal = run_with(src, false).map(|(out, _)| out);
    let stressed = run_with(src, true).map(|(out, _)| out);
    assert_eq!(normal, stressed, "--gc-stress changed the result of {src:?}");
    normal
}

/// The S-expression for `print <expr>;`.
pub fn ast(expr: &str) -> String {
    jinso::parse(&format!("print {expr};")).unwrap()[0].to_string()
}
