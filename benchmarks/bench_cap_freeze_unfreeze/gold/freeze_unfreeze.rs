//! Gold test: freeze, drop ref, thaw, consume.
//! NoCapability loses freeze/thaw distinction; Full PCPN tracks shared vs mut.

use bench_cap_freeze_unfreeze::*;

#[test]
fn gold_freeze_unfreeze() {
    let mut f = Freezable::new(10);
    {
        let _x = f.freeze();
        // _x goes out of scope — shared borrow ends
    }
    f.thaw(20);
    let out = f.consume();
    assert_eq!(out, 20);
}
