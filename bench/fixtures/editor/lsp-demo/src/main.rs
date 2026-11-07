/// A point on the plane.
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// Distance from the origin.
    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Moves the point by `dx`, `dy`.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }
}

fn main() {
    let mut p = Point { x: 3.0, y: 4.0 };
    p.translate(1.0, 1.0);
    println!("{:?} is {} from the origin", p, p.length());
    let n: i32 = "not a number";
    println!("{n}");
}
