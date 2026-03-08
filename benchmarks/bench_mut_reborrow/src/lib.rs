pub struct State {
    pub count: i32,
}
impl State {
    pub fn new() -> Self {
        State { count: 0 }
    }
    pub fn modify(&mut self) -> &mut i32 {
        &mut self.count
    }
    pub fn read(&self) -> i32 {
        self.count
    }
}
pub fn increment(val: &mut i32) {
    *val += 1;
}
