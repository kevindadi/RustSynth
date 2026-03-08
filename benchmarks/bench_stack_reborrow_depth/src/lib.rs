pub struct DeepRef {
    value: i32,
}

impl DeepRef {
    pub fn new(v: i32) -> Self {
        DeepRef { value: v }
    }

    pub fn borrow(&self) -> &i32 {
        &self.value
    }

    pub fn reborrow_shr(val: &DeepRef) -> &DeepRef {
        val
    }

    pub fn value(&self) -> i32 {
        self.value
    }
}
