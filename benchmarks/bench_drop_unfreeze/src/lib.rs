pub struct Holder {
    inner: i32,
}
impl Holder {
    pub fn new(v: i32) -> Self {
        Holder { inner: v }
    }
    pub fn view(&self) -> &i32 {
        &self.inner
    }
    pub fn set(&mut self, v: i32) {
        self.inner = v;
    }
    pub fn into_inner(self) -> i32 {
        self.inner
    }
}
