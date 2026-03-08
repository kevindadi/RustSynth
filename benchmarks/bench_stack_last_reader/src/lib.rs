pub struct Buffer {
    value: i32,
}

impl Buffer {
    pub fn new(v: i32) -> Self {
        Buffer { value: v }
    }

    pub fn read(&self) -> i32 {
        self.value
    }

    pub fn write(&mut self, v: i32) {
        self.value = v;
    }

    pub fn drain(self) -> i32 {
        self.value
    }
}
