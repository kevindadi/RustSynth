//! Gold test: proper borrow sequence - use into_val without conflicting borrows.

use bench_typeonly_borrow_kind::*;

#[test]
fn gold_borrow_kind() {
    let h = Holder::new(42);
    let out = h.into_val();
    assert_eq!(out, 42);
}
