//! Gold test: read (shared borrow), end borrow, write (mut borrow), drain.

use bench_stack_last_reader::*;

#[test]
fn gold_last_reader() {
    let mut b = Buffer::new(5);
    {
        let _x = b.read();
        // _x goes out of scope — shared borrow ends
    }
    b.write(99);
    let out = b.drain();
    assert_eq!(out, 99);
}
