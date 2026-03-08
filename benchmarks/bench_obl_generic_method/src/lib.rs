pub struct Container(i32);

impl Container {
    pub fn new(v: i32) -> Self {
        Container(v)
    }

    pub fn get(&self) -> i32 {
        self.0
    }
}

pub struct SumResult(i32);

impl SumResult {
    pub fn value(&self) -> i32 {
        self.0
    }
}

pub trait Summable {
    fn sum(&self) -> i32;
}

impl Summable for Container {
    fn sum(&self) -> i32 {
        self.get()
    }
}

pub fn sum_all<T: Summable>(items: &T) -> SumResult {
    SumResult(items.sum())
}
