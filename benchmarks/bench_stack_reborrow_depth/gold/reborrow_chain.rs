//! Gold test: nested reborrows requiring stack depth tracking.

use bench_stack_reborrow_depth::*;

#[test]
fn gold_reborrow_chain() {
    let d = DeepRef::new(7);
    let r1 = DeepRef::reborrow_shr(&d);
    let r2 = DeepRef::reborrow_shr(r1);
    let _v = r2.value();
    // r2, r1 drop; then we can use d
    let out = d.value();
    assert_eq!(out, 7);
}
