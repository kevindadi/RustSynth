pub struct Source(i32);

impl Source {
    pub fn new(v: i32) -> Self {
        Source(v)
    }

    pub fn consume(self) -> i32 {
        self.0
    }

    pub fn provide(&self) -> i32 {
        self.0
    }
}

pub struct Sink;

impl Sink {
    pub fn absorb(v: i32) -> bool {
        v > 0
    }
}
