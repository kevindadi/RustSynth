pub trait Printable {
    fn describe(&self) -> i32;
}

pub struct PrintResult(i32);

impl PrintResult {
    pub fn value(&self) -> i32 {
        self.0
    }
}

pub struct PrintableItem(i32);

impl PrintableItem {
    pub fn new(v: i32) -> Self {
        PrintableItem(v)
    }
}

impl Printable for PrintableItem {
    fn describe(&self) -> i32 {
        self.0
    }
}

pub struct PlainItem(i32);

impl PlainItem {
    pub fn new(v: i32) -> Self {
        PlainItem(v)
    }
}

pub fn print_it<T: Printable>(x: &T) -> PrintResult {
    PrintResult(x.describe())
}
