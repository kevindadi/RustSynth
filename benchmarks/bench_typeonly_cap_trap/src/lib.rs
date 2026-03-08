pub struct Guarded {
    value: i32,
    locked: bool,
}

impl Guarded {
    pub fn new(v: i32) -> Self {
        Guarded {
            value: v,
            locked: false,
        }
    }

    pub fn lock(&mut self) {
        self.locked = true;
    }

    pub fn unlock(&mut self) {
        self.locked = false;
    }

    pub fn read(&self) -> i32 {
        assert!(!self.locked);
        self.value
    }

    pub fn take(self) -> i32 {
        self.value
    }
}
