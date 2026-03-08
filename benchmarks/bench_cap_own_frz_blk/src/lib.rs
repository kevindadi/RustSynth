// State: transitions own -> frz (observe) -> own -> blk (modify) -> own -> finish
pub struct State {
    value: i32,
}

impl State {
    pub fn new(v: i32) -> Self {
        State { value: v }
    }
    pub fn observe(&self) -> i32 {
        self.value
    }
    pub fn modify(&mut self, v: i32) {
        self.value = v;
    }
    pub fn finish(self) -> i32 {
        self.value
    }
}
