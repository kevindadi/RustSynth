//! Gold test: NonCopyBox used once (move), no reuse after unwrap.
//! NoCapability won't distinguish move vs copy; Full PCPN correctly rejects reuse.

use bench_cap_move_vs_copy::*;

#[test]
fn gold_move_vs_copy() {
    let b = NonCopyBox::new(42);
    let out = b.unwrap();
    assert_eq!(out, 42);
}
