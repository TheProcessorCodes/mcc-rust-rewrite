//! Minimal f64 vector / quaternion math. Quaternions are (x, y, z, w), body to world, Hamilton product.
//! Handedness never enters: rotations act on coordinates, so the same code serves Reflex's frame.

use std::ops::{Add, AddAssign, Div, Index, IndexMut, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

impl Vec3 {
    pub const ZERO: Vec3 = v3(0.0, 0.0, 0.0);
    pub const X: Vec3 = v3(1.0, 0.0, 0.0);
    pub const Y: Vec3 = v3(0.0, 1.0, 0.0);
    pub const Z: Vec3 = v3(0.0, 0.0, 1.0);

    pub fn from_array(a: [f64; 3]) -> Self {
        v3(a[0], a[1], a[2])
    }
    pub fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }
    pub fn normalize(self) -> Vec3 {
        self / self.length()
    }
    /// Elementwise product (diagonal inertia times a vector).
    pub fn mul_elem(self, o: Vec3) -> Vec3 {
        v3(self.x * o.x, self.y * o.y, self.z * o.z)
    }
    pub fn div_elem(self, o: Vec3) -> Vec3 {
        v3(self.x / o.x, self.y / o.y, self.z / o.z)
    }
    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self + (o - self) * t
    }
    /// Two unit vectors that complete `self` (unit) to an orthonormal basis.
    pub fn any_orthonormal_pair(self) -> (Vec3, Vec3) {
        let other = if self.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let a = self.cross(other).normalize();
        (a, self.cross(a))
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        v3(-self.x, -self.y, -self.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Mul<Vec3> for f64 {
    type Output = Vec3;
    fn mul(self, v: Vec3) -> Vec3 {
        v * self
    }
}
impl Div<f64> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f64) -> Vec3 {
        v3(self.x / s, self.y / s, self.z / s)
    }
}
impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}
impl SubAssign for Vec3 {
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}
impl Index<usize> for Vec3 {
    type Output = f64;
    fn index(&self, i: usize) -> &f64 {
        match i {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Vec3 index {i}"),
        }
    }
}
impl IndexMut<usize> for Vec3 {
    fn index_mut(&mut self, i: usize) -> &mut f64 {
        match i {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.z,
            _ => panic!("Vec3 index {i}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Default for Quat {
    fn default() -> Self {
        Quat::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Quat { x, y, z, w }
    }
    pub fn from_axis_angle(axis: Vec3, angle: f64) -> Self {
        let (s, c) = (angle * 0.5).sin_cos();
        let a = axis.normalize() * s;
        Quat::new(a.x, a.y, a.z, c)
    }
    pub fn vec(self) -> Vec3 {
        v3(self.x, self.y, self.z)
    }
    pub fn conj(self) -> Quat {
        Quat::new(-self.x, -self.y, -self.z, self.w)
    }
    pub fn dot(self, o: Quat) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalize(self) -> Quat {
        let n = self.length();
        Quat::new(self.x / n, self.y / n, self.z / n, self.w / n)
    }
    /// Rotates a body-frame vector into the world frame: q v q*.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let u = self.vec();
        let t = 2.0 * u.cross(v);
        v + self.w * t + u.cross(t)
    }
    /// Rotates a world-frame vector into the body frame: q* v q.
    pub fn inv_rotate(self, v: Vec3) -> Vec3 {
        self.conj().rotate(v)
    }
    /// Normalised lerp / slerp for render interpolation.
    pub fn slerp(self, o: Quat, t: f64) -> Quat {
        let mut o = o;
        let mut d = self.dot(o);
        if d < 0.0 {
            o = Quat::new(-o.x, -o.y, -o.z, -o.w);
            d = -d;
        }
        if d > 0.9995 {
            return Quat::new(
                self.x + (o.x - self.x) * t,
                self.y + (o.y - self.y) * t,
                self.z + (o.z - self.z) * t,
                self.w + (o.w - self.w) * t,
            )
            .normalize();
        }
        let th = d.acos();
        let (a, b) = (((1.0 - t) * th).sin() / th.sin(), (t * th).sin() / th.sin());
        Quat::new(a * self.x + b * o.x, a * self.y + b * o.y, a * self.z + b * o.z, a * self.w + b * o.w)
    }
}

/// Hamilton product.
impl Mul for Quat {
    type Output = Quat;
    fn mul(self, b: Quat) -> Quat {
        let a = self;
        Quat::new(
            a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
            a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
        )
    }
}
