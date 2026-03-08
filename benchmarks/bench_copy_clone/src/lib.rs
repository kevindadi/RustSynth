#[derive(Clone, Copy)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone)]
pub struct Line {
    pub start: Point,
    pub end: Point,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Point { x, y }
    }
    pub fn distance_sq(&self) -> i32 {
        self.x * self.x + self.y * self.y
    }
}

impl Line {
    pub fn new(start: Point, end: Point) -> Self {
        Line { start, end }
    }
    pub fn length_sq(&self) -> i32 {
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        dx * dx + dy * dy
    }
}

pub fn point_sum(a: Point, b: Point) -> Point {
    Point {
        x: a.x + b.x,
        y: a.y + b.y,
    }
}
