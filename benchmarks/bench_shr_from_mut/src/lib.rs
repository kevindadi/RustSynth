pub struct Data {
    pub value: i32,
}
impl Data {
    pub fn new(v: i32) -> Self {
        Data { value: v }
    }
    pub fn as_ref(&self) -> &i32 {
        &self.value
    }
    pub fn as_mut(&mut self) -> &mut i32 {
        &mut self.value
    }
    pub fn read_only(&self) -> i32 {
        self.value
    }
}
