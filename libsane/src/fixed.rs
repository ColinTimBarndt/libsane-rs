use core::fmt;
use std::ops;

use crate::sys;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Fixed(sys::Fixed);

impl Fixed {
    pub const STEP: f64 = (1 << sys::FIXED_SCALE_SHIFT) as f64;

    pub fn new(v: f64) -> Self {
        Self(sys::fix(v))
    }

    pub const fn from_bits(bits: sys::Fixed) -> Self {
        Self(bits)
    }

    pub const fn to_bits(self) -> sys::Fixed {
        self.0
    }
}

impl From<f64> for Fixed {
    fn from(value: f64) -> Self {
        Self::new(value)
    }
}

impl From<Fixed> for f64 {
    fn from(value: Fixed) -> Self {
        sys::unfix(value.0)
    }
}

impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&f64::from(*self), f)
    }
}

impl fmt::Debug for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&f64::from(*self), f)
    }
}

impl ops::Add for Fixed {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl ops::Sub for Fixed {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl ops::AddAssign for Fixed {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0
    }
}

impl ops::SubAssign for Fixed {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0
    }
}
