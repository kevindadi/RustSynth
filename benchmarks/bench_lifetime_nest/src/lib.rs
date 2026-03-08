pub struct Outer {
    inner: i32,
}
impl Outer {
    pub fn new(v: i32) -> Self {
        Outer { inner: v }
    }
    pub fn borrow_inner(&self) -> &i32 {
        &self.inner
    }
    pub fn set_inner(&mut self, v: i32) {
        self.inner = v;
    }
}
pub fn read_nested(r: &i32) -> i32 {
    *r
}
pub fn add_vals(a: i32, b: i32) -> i32 {
    a + b
}
