//! **A tour of compile errors**: each file in `tests/compile_errors/` is a
//! mistake semlex catches at compile time, and the `.stderr` beside it is
//! exactly what `rustc` says.

#[test]
fn compile_errors() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_errors/*.rs");
}
