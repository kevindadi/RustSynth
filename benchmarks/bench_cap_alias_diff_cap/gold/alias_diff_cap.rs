//! Gold test: Use both Slot and Slot2, showing capability matters.
//! Slot is Copy (can take and still have value), Slot2 is not.

use bench_cap_alias_diff_cap::*;

#[test]
fn gold_alias_diff_cap() {
    let s1 = Slot::new(42);
    let mut s2 = Slot2::new(2);
    let _ = s2.peek();
    s2.poke(3);
    let out = s1.take();
    assert_eq!(out, 42);
}
