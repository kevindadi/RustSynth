//! Gold test: two borrows from same owner discharged in LIFO order.

use bench_stack_discharge_order::*;

#[test]
fn gold_discharge_order() {
    let p = Pair::new(1, 2);
    {
        let _f = p.first();
        let _s = p.second();
        // f, s dropped in LIFO order when block ends
    }
    let (a, b) = p.take_both();
    assert_eq!(a, 1);
    assert_eq!(b, 2);
}
