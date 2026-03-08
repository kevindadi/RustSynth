// Slot: Copy — same API shape
#[derive(Copy, Clone)]
pub struct Slot {
    value: i32,
}

impl Slot {
    pub fn new(v: i32) -> Self {
        Slot { value: v }
    }
    pub fn peek(&self) -> i32 {
        self.value
    }
    pub fn poke(&mut self, v: i32) {
        self.value = v;
    }
    pub fn take(self) -> i32 {
        self.value
    }
}

// Slot2: NOT Copy — same API shape, different capability
pub struct Slot2 {
    value: i32,
}

impl Slot2 {
    pub fn new(v: i32) -> Self {
        Slot2 { value: v }
    }
    pub fn peek(&self) -> i32 {
        self.value
    }
    pub fn poke(&mut self, v: i32) {
        self.value = v;
    }
    pub fn take(self) -> i32 {
        self.value
    }
}
