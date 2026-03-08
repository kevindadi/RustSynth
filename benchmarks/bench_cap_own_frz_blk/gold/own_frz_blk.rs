//! Gold test: create, observe (freezes), drop ref, modify (blocks), drop mut ref, finish.
//! Tests own/frz/blk state transitions.

use bench_cap_own_frz_blk::*;

#[test]
fn gold_own_frz_blk() {
    let mut s = State::new(5);
    {
        let _o = s.observe();
        // _o goes out of scope — shared borrow ends
    }
    s.modify(15);
    let out = s.finish();
    assert_eq!(out, 15);
}
