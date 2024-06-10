#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/sane.rs"));

pub const fn version_code(major: Word, minor: Word, build: Word) -> Word {
    ((major & 0xff) << 24) | ((minor & 0xff) << 16) | (build & 0xffff)
}

pub const fn version_major(code: Word) -> u8 {
    (code >> 24) as u8
}

pub const fn version_minor(code: Word) -> u8 {
    (code >> 16) as u8
}

pub const fn version_build(code: Word) -> u16 {
    code as u16
}

pub fn fix(v: f64) -> Fixed {
    (v * (1 << FIXED_SCALE_SHIFT) as f64) as Fixed
}

pub fn unfix(v: Fixed) -> f64 {
    v as f64 / (1 << FIXED_SCALE_SHIFT) as f64
}

pub const fn option_is_active(cap: Int) -> bool {
    (cap & CAP_INACTIVE as Int) == 0
}

pub const fn option_is_settable(cap: Int) -> bool {
    (cap & CAP_SOFT_SELECT as Int) != 0
}
