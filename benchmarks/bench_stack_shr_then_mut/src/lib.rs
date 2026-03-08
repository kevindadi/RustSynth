pub struct Toggle {
    value: bool,
}

impl Toggle {
    pub fn new(v: bool) -> Self {
        Toggle { value: v }
    }

    pub fn peek(&self) -> bool {
        self.value
    }

    pub fn flip(&mut self) {
        self.value = !self.value;
    }

    pub fn into_inner(self) -> bool {
        self.value
    }
}
