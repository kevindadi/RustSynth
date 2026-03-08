// src/lib.rs — a non-Copy wrapper and functions that distinguish move/copy
pub struct Wrapper {
    pub value: i32,
}

impl Wrapper {
    pub fn new(v: i32) -> Self {
        Wrapper { value: v }
    }
    pub fn take(self) -> i32 {
        self.value
    }
    pub fn peek(w: &Wrapper) -> i32 {
        w.value
    }
}

pub fn consume(w: Wrapper) -> i32 {
    w.value
}
pub fn copy_val(x: i32) -> i32 {
    x
}
