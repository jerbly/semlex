//! `attrs!` diagnostics, as `trybuild` UI tests against this crate's
//! registry (default features: non-stable items need opt-in). Snapshots are
//! recorded on the toolchain pinned in `rust-toolchain.toml`.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass_*.rs");
    t.compile_fail("tests/ui/fail_*.rs");
}
