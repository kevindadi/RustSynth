pub trait Producer {
    type Output;
    fn produce(&self) -> Self::Output;
}

pub struct IntProducer;
impl Producer for IntProducer {
    type Output = i32;
    fn produce(&self) -> i32 {
        42
    }
}

pub struct StrProducer;
impl Producer for StrProducer {
    type Output = String;
    fn produce(&self) -> String {
        "hello".to_string()
    }
}

pub fn make_int_producer() -> IntProducer {
    IntProducer
}
pub fn make_str_producer() -> StrProducer {
    StrProducer
}
pub fn consume_i32(x: i32) -> i32 {
    x
}
