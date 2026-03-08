//! Gold test: nested scope pattern — shared borrow in inner scope, drop ref, then mutate.
//! NoStack cannot track that the borrow is dead when the scope ends.

use bench_stack_nested_scope::*;

#[test]
fn gold_nested_scope() {
    let mut r = Resource::new(42);
    {
        let _ref_val = r.borrow_val();
        // ref_val goes out of scope here — borrow ends
    }
    r.set_val(10);
    let out = r.consume();
    assert_eq!(out, 10);
}
