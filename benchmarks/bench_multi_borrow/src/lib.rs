pub struct MultiField {
    pub a: i32,
    pub b: i32,
    pub c: i32,
}
impl MultiField {
    pub fn new(a: i32, b: i32, c: i32) -> Self {
        MultiField { a, b, c }
    }
    pub fn ref_a(&self) -> &i32 {
        &self.a
    }
    pub fn ref_b(&self) -> &i32 {
        &self.b
    }
    pub fn ref_c(&self) -> &i32 {
        &self.c
    }
    pub fn sum(&self) -> i32 {
        self.a + self.b + self.c
    }
}
pub fn add_refs(x: &i32, y: &i32) -> i32 {
    *x + *y
}
