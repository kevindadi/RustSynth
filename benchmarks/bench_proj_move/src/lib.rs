pub struct Pair {
    pub a: i32,
    pub b: String,
}
impl Pair {
    pub fn new(a: i32, b: String) -> Self {
        Pair { a, b }
    }
}
pub fn make_string(s: &str) -> String {
    s.to_string()
}
pub fn use_string(s: String) -> usize {
    s.len()
}
pub fn use_int(x: i32) -> i32 {
    x + 1
}
