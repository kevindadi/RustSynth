// Freezable: freeze (shared borrow) blocks thaw (mut borrow) until ref is dropped
pub struct Freezable {
    value: i32,
}

impl Freezable {
    pub fn new(v: i32) -> Self {
        Freezable { value: v }
    }
    pub fn freeze(&self) -> i32 {
        self.value
    }
    pub fn thaw(&mut self, v: i32) {
        self.value = v;
    }
    pub fn consume(self) -> i32 {
        self.value
    }
}
