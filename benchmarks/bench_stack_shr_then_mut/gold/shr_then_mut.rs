//! Gold test: shared borrow, end it, then mutable borrow on same value.

use bench_stack_shr_then_mut::*;

#[test]
fn gold_shr_then_mut() {
    let mut t = Toggle::new(false);
    {
        let _b = t.peek();
        // _b goes out of scope — shared borrow ends
    }
    t.flip();
    let out = t.into_inner();
    assert_eq!(out, true);
}
