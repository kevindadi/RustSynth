//! Gold test: create Record, project fields, pass references to combine.

use bench_mixed_ctor_borrow_proj::*;

#[test]
fn gold_ctor_borrow_proj() {
    let r = Record::new(10, 32);
    let out = combine(&r.a, &r.b);
    assert_eq!(out, 42);
}
