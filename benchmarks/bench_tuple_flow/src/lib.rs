pub fn make_pair(a: i32, b: i32) -> (i32, i32) {
    (a, b)
}
pub fn fst(pair: (i32, i32)) -> i32 {
    pair.0
}
pub fn snd(pair: (i32, i32)) -> i32 {
    pair.1
}
pub fn swap(pair: (i32, i32)) -> (i32, i32) {
    (pair.1, pair.0)
}
pub fn sum_pair(pair: (i32, i32)) -> i32 {
    pair.0 + pair.1
}
