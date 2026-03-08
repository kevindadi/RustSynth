pub struct Stack<T> {
    data: Vec<T>,
}
impl<T> Stack<T> {
    pub fn new() -> Self {
        Stack { data: Vec::new() }
    }
    pub fn push(&mut self, val: T) {
        self.data.push(val);
    }
    pub fn pop(&mut self) -> Option<T> {
        self.data.pop()
    }
    pub fn peek(&self) -> Option<&T> {
        self.data.last()
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
