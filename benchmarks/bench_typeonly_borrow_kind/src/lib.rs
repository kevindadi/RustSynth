pub struct Holder(i32);

impl Holder {
    pub fn new(v: i32) -> Self {
        Holder(v)
    }

    pub fn read(&self) -> i32 {
        self.0
    }

    pub fn write(&mut self, v: i32) {
        self.0 = v;
    }

    pub fn into_val(self) -> i32 {
        self.0
    }
}
