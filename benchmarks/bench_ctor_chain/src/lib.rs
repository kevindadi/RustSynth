pub struct Builder {
    x: i32,
    y: i32,
    label: String,
}
impl Builder {
    pub fn new() -> Self {
        Builder {
            x: 0,
            y: 0,
            label: String::new(),
        }
    }
    pub fn set_x(mut self, x: i32) -> Self {
        self.x = x;
        self
    }
    pub fn set_y(mut self, y: i32) -> Self {
        self.y = y;
        self
    }
    pub fn set_label(mut self, label: String) -> Self {
        self.label = label;
        self
    }
    pub fn build(self) -> Product {
        Product {
            x: self.x,
            y: self.y,
            label: self.label,
        }
    }
}
pub struct Product {
    pub x: i32,
    pub y: i32,
    pub label: String,
}
impl Product {
    pub fn sum(&self) -> i32 {
        self.x + self.y
    }
    pub fn label_ref(&self) -> &String {
        &self.label
    }
}
pub fn make_label() -> String {
    "default".to_string()
}
