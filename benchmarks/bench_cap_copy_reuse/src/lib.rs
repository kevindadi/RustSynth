// CopyNum: Copy+Clone — can be reused after being passed to add
#[derive(Copy, Clone)]
pub struct CopyNum {
    value: i32,
}

impl CopyNum {
    pub fn new(v: i32) -> Self {
        CopyNum { value: v }
    }
    pub fn val(&self) -> i32 {
        self.value
    }
    pub fn add(self, other: CopyNum) -> CopyNum {
        CopyNum {
            value: self.value + other.value,
        }
    }
}
