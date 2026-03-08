pub struct Pair {
    first: i32,
    second: i32,
}

impl Pair {
    pub fn new(a: i32, b: i32) -> Self {
        Pair { first: a, second: b }
    }

    pub fn first(&self) -> &i32 {
        &self.first
    }

    pub fn second(&self) -> &i32 {
        &self.second
    }

    pub fn take_both(self) -> (i32, i32) {
        (self.first, self.second)
    }
}
