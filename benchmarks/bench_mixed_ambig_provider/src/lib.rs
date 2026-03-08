pub struct ProviderA;

impl ProviderA {
    pub fn make() -> i32 {
        42
    }
}

pub struct ProviderB;

impl ProviderB {
    pub fn make() -> i32 {
        100
    }
}

pub struct Consumer;

impl Consumer {
    pub fn eat(v: i32) -> bool {
        v > 0
    }
}
