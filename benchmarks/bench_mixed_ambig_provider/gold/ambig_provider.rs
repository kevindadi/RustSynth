//! Gold test: use either provider to get i32, then Consumer::eat.

use bench_mixed_ambig_provider::*;

#[test]
fn gold_ambig_provider() {
    let v = ProviderA::make();
    let out = Consumer::eat(v);
    assert!(out);
}
