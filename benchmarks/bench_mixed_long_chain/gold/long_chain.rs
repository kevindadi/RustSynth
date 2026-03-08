//! Gold test: chain of advance().advance().advance().advance().finish().

use bench_mixed_long_chain::*;

#[test]
fn gold_long_chain() {
    let s = Step::new(38);
    let out = s.advance().advance().advance().advance().finish();
    assert_eq!(out, 42);
}
