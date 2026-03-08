pub struct Record {
    pub a: i32,
    pub b: i32,
}

impl Record {
    pub fn new(a: i32, b: i32) -> Self {
        Record { a, b }
    }
}

pub fn combine(x: &i32, y: &i32) -> i32 {
    *x + *y
}
