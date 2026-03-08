use std::fmt::Display;

pub fn format_val<T: Display>(x: &T) -> String {
    format!("{}", x)
}
pub fn to_string_val<T: Display>(x: T) -> String {
    format!("{}", x)
}
pub fn combine<T: Display>(a: &T, b: &T) -> String {
    format!("{},{}", a, b)
}
pub fn make_i32() -> i32 {
    42
}
pub fn make_string() -> String {
    "hello".to_string()
}
