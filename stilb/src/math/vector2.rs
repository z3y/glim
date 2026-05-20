use std::ops::{Add, Div, Mul, Sub};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vector2 {
    pub x: f32,
    pub y: f32,
}

#[allow(dead_code)]
impl Vector2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };

    pub fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let clamped_t = t.clamp(0.0, 1.0);
        Self {
            x: a.x + (b.x - a.x) * clamped_t,
            y: a.y + (b.y - a.y) * clamped_t,
        }
    }

    pub fn distance(a: Self, b: Self) -> f32 {
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn abs(self) -> Self {
        Self {
            x: f32::abs(self.x),
            y: f32::abs(self.y),
        }
    }
}

impl Add for Vector2 {
    type Output = Vector2;
    fn add(self, rhs: Vector2) -> Vector2 {
        Vector2::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vector2 {
    type Output = Vector2;
    fn sub(self, rhs: Vector2) -> Vector2 {
        Vector2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul for Vector2 {
    type Output = Vector2;
    fn mul(self, rhs: Vector2) -> Vector2 {
        Vector2::new(self.x * rhs.x, self.y * rhs.y)
    }
}

impl Div for Vector2 {
    type Output = Vector2;
    fn div(self, rhs: Vector2) -> Vector2 {
        Vector2::new(self.x / rhs.x, self.y / rhs.y)
    }
}
