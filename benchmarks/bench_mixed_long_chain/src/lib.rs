pub struct Step(i32);

impl Step {
    pub fn new(v: i32) -> Self {
        Step(v)
    }

    pub fn advance(self) -> Step {
        Step(self.0 + 1)
    }

    pub fn finish(self) -> i32 {
        self.0
    }
}
