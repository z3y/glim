use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub};

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct Vector4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[allow(dead_code)]
impl Vector4 {
    #[inline]
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };

    pub const ONE: Self = Self {
        x: 1.0,
        y: 1.0,
        z: 1.0,
        w: 1.0,
    };
}

impl Add for Vector4 {
    type Output = Vector4;
    #[inline]
    fn add(self, rhs: Vector4) -> Vector4 {
        Vector4::new(
            self.x + rhs.x,
            self.y + rhs.y,
            self.z + rhs.z,
            self.w + rhs.w,
        )
    }
}

impl Sub for Vector4 {
    type Output = Vector4;
    #[inline]
    fn sub(self, rhs: Vector4) -> Vector4 {
        Vector4::new(
            self.x - rhs.x,
            self.y - rhs.y,
            self.z - rhs.z,
            self.w - rhs.w,
        )
    }
}

impl Mul for Vector4 {
    type Output = Vector4;
    #[inline]
    fn mul(self, rhs: Vector4) -> Vector4 {
        Vector4::new(
            self.x * rhs.x,
            self.y * rhs.y,
            self.z * rhs.z,
            self.w * rhs.w,
        )
    }
}

impl Div for Vector4 {
    type Output = Vector4;
    #[inline]
    fn div(self, rhs: Vector4) -> Vector4 {
        Vector4::new(
            self.x / rhs.x,
            self.y / rhs.y,
            self.z / rhs.z,
            self.w / rhs.w,
        )
    }
}

impl Mul<f32> for Vector4 {
    type Output = Vector4;
    #[inline]
    fn mul(self, rhs: f32) -> Vector4 {
        Vector4::new(self.x * rhs, self.y * rhs, self.z * rhs, self.w * rhs)
    }
}

impl Div<f32> for Vector4 {
    type Output = Vector4;
    #[inline]
    fn div(self, rhs: f32) -> Vector4 {
        Vector4::new(self.x / rhs, self.y / rhs, self.z / rhs, self.w / rhs)
    }
}

impl MulAssign<Vector4> for Vector4 {
    #[inline]
    fn mul_assign(&mut self, rhs: Vector4) {
        self.x *= rhs.x;
        self.y *= rhs.y;
        self.z *= rhs.z;
        self.w *= rhs.w;
    }
}

impl AddAssign<Vector4> for Vector4 {
    #[inline]
    fn add_assign(&mut self, rhs: Vector4) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
        self.w += rhs.w;
    }
}

impl DivAssign<Vector4> for Vector4 {
    #[inline]
    fn div_assign(&mut self, rhs: Vector4) {
        self.x /= rhs.x;
        self.y /= rhs.y;
        self.z /= rhs.z;
        self.w /= rhs.w;
    }
}

impl DivAssign<f32> for Vector4 {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
        self.z /= rhs;
        self.w /= rhs;
    }
}
