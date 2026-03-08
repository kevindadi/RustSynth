//! Gold test: proper lock/unlock sequence before take.

use bench_typeonly_cap_trap::*;

#[test]
fn gold_cap_trap() {
    let mut g = Guarded::new(42);
    g.lock();
    g.unlock();
    let out = g.take();
    assert_eq!(out, 42);
}
