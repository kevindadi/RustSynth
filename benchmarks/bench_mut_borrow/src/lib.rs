pub struct Buffer {
    data: Vec<i32>,
}
impl Buffer {
    pub fn new() -> Self {
        Buffer { data: Vec::new() }
    }
    pub fn push(&mut self, val: i32) {
        self.data.push(val);
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn get(&self, idx: usize) -> Option<&i32> {
        self.data.get(idx)
    }
    pub fn clear(&mut self) {
        self.data.clear();
    }
}
