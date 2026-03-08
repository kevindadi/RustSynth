pub trait Acceptable {
    fn value(&self) -> i32;
}

pub struct ProcessedOutput(i32);

impl ProcessedOutput {
    pub fn get(&self) -> i32 {
        self.0
    }
}

pub struct ValidInput(i32);

impl ValidInput {
    pub fn new(v: i32) -> Self {
        ValidInput(v)
    }
}

impl Acceptable for ValidInput {
    fn value(&self) -> i32 {
        self.0
    }
}

#[allow(dead_code)]
pub struct InvalidInput(i32);

impl InvalidInput {
    pub fn new(v: i32) -> Self {
        InvalidInput(v)
    }
}

pub fn process<T: Acceptable>(x: T) -> ProcessedOutput {
    ProcessedOutput(x.value())
}
