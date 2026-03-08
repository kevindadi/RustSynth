pub trait Transform {
    fn transform(&self) -> i32;
}

pub struct TransformResult(i32);

impl TransformResult {
    pub fn value(&self) -> i32 {
        self.0
    }
}

pub struct Doubler(i32);

impl Doubler {
    pub fn new(v: i32) -> Self {
        Doubler(v)
    }
}

impl Transform for Doubler {
    fn transform(&self) -> i32 {
        self.0 * 2
    }
}

pub struct Halver(i32);

impl Halver {
    pub fn new(v: i32) -> Self {
        Halver(v)
    }
}

pub fn apply_transform<T: Transform>(x: &T) -> TransformResult {
    TransformResult(x.transform())
}
