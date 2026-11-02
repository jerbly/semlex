//! Compile-time behavior of the macros, as `trybuild` UI tests. Snapshots are
//! recorded on the toolchain pinned in `rust-toolchain.toml`.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass_*.rs");
    t.compile_fail("tests/ui/fail_*.rs");
}
