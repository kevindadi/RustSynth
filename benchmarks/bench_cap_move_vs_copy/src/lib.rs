// NonCopyBox: NOT Copy — move semantics only
pub struct NonCopyBox {
    value: i32,
}

impl NonCopyBox {
    pub fn new(v: i32) -> Self {
        NonCopyBox { value: v }
    }
    pub fn get(&self) -> i32 {
        self.value
    }
    pub fn unwrap(self) -> i32 {
        self.value
    }
}

// CopyVal: IS Copy — can be reused after use
#[derive(Copy, Clone)]
pub struct CopyVal {
    value: i32,
}

impl CopyVal {
    pub fn new(v: i32) -> Self {
        CopyVal { value: v }
    }
    pub fn get(&self) -> i32 {
        self.value
    }
}
