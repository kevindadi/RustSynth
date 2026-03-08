//! Gold test: use Source.consume to get i32, then Sink::absorb.

use bench_typeonly_dead_provider::*;

#[test]
fn gold_dead_provider() {
    let s = Source::new(42);
    let v = s.consume();
    let out = Sink::absorb(v);
    assert!(out);
}
