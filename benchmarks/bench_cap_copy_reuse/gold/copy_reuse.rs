//! Gold test: create two CopyNum, add them, reuse one of them again.
//! Copy semantics allow reuse after add; non-Copy would not.

use bench_cap_copy_reuse::*;

#[test]
fn gold_copy_reuse() {
    let a = CopyNum::new(10);
    let b = CopyNum::new(20);
    let sum = a.add(b);
    let sum2 = a.add(b);
    assert_eq!(sum.val(), 30);
    assert_eq!(sum2.val(), 30);
}
