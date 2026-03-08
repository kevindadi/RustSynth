pub struct Resource {
    value: i32,
}

impl Resource {
    pub fn new(v: i32) -> Self {
        Resource { value: v }
    }

    pub fn borrow_val(&self) -> &i32 {
        &self.value
    }

    pub fn set_val(&mut self, v: i32) {
        self.value = v;
    }

    pub fn consume(self) -> i32 {
        self.value
    }
}
