pub struct Container {
    value: i32,
}
impl Container {
    pub fn new(v: i32) -> Self {
        Container { value: v }
    }
    pub fn get(&self) -> &i32 {
        &self.value
    }
    pub fn value(&self) -> i32 {
        self.value
    }
}
pub fn read_ref(r: &i32) -> i32 {
    *r
}
