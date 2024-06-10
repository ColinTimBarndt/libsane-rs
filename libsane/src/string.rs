use core::fmt;
use std::{
    borrow::Borrow,
    cmp::Ordering,
    ffi::{c_char, CStr},
    fmt::{Debug, Display, Write},
    hash::Hash,
    iter::FusedIterator,
    marker::PhantomData,
    mem::MaybeUninit,
};

use crate::slice_util::{assume_init_slice, new_uninit_boxed_slice, slice_as_maybe_uninit};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SaneStr(CStr);

impl SaneStr {
    pub const EMPTY: &'static Self = Self::from_cstr(c"");

    /// # Safety
    /// The string must end with a NUL character.
    pub const unsafe fn new_unchecked(s: &[u8]) -> &Self {
        std::mem::transmute::<&[u8], &Self>(s)
    }

    /// # Safety
    /// See [`CStr::from_ptr`]'s safety section.
    pub unsafe fn from_ptr<'a>(ptr: *const c_char) -> &'a Self {
        Self::from_cstr(CStr::from_ptr(ptr))
    }

    pub const fn from_cstr(c: &CStr) -> &Self {
        unsafe { std::mem::transmute::<&CStr, &Self>(c) }
    }

    pub fn from_cstr_mut(c: &mut CStr) -> &mut Self {
        unsafe { std::mem::transmute::<&mut CStr, &mut Self>(c) }
    }

    pub fn count_bytes(&self) -> usize {
        self.0.count_bytes()
    }

    pub fn count_bytes_with_nul(&self) -> usize {
        self.0.count_bytes() + 1
    }

    pub fn to_bytes(&self) -> &[u8] {
        self.0.to_bytes()
    }

    pub fn to_bytes_with_nul(&self) -> &[u8] {
        self.0.to_bytes_with_nul()
    }

    pub fn as_ptr(&self) -> *const c_char {
        self.0.as_ptr()
    }

    pub fn chars(&self) -> Chars {
        unsafe { Chars::new(self.as_ptr()) }
    }

    pub fn bytes(&self) -> Bytes {
        unsafe { Bytes::new(self.as_ptr()) }
    }
}

impl AsRef<CStr> for SaneStr {
    fn as_ref(&self) -> &CStr {
        &self.0
    }
}

impl AsRef<SaneStr> for SaneStr {
    fn as_ref(&self) -> &SaneStr {
        self
    }
}

impl<'a> IntoIterator for &'a SaneStr {
    type IntoIter = Chars<'a>;
    type Item = char;

    fn into_iter(self) -> Self::IntoIter {
        self.chars()
    }
}

impl ToOwned for SaneStr {
    type Owned = SaneString;

    fn to_owned(&self) -> Self::Owned {
        SaneString::from_cstr(&self.0)
    }
}

#[derive(Clone)]
pub struct SaneString(
    // Invariant 1: All chars are initialized until the first NUL
    // Invariant 2: There is at least one NUL
    Box<[MaybeUninit<u8>]>,
);

impl SaneString {
    pub fn with_capacity(reserve: usize) -> Self {
        assert_ne!(
            reserve, 0,
            "SaneString must be at least one byte in size to fit a NUL"
        );
        let buf = new_uninit_boxed_slice(reserve);
        Self(buf)
    }

    pub fn from_cstr(c: &CStr) -> Self {
        let bytes = c.to_bytes_with_nul();
        let mut buf = Self::with_capacity(bytes.len());
        buf.0[..bytes.len()].copy_from_slice(slice_as_maybe_uninit(bytes));
        buf
    }

    pub fn set_contents(&mut self, value: &SaneStr) {
        let bytes = value.to_bytes_with_nul();
        assert!(bytes.len() <= self.capacity());
        self.0[..bytes.len()].copy_from_slice(slice_as_maybe_uninit(bytes));
    }

    pub fn count_bytes(&self) -> usize {
        let mut i = 0;
        loop {
            let ch = unsafe { self.0[i].assume_init() };
            if ch == 0 {
                return i;
            }
            i += 1;
        }
    }

    pub fn count_bytes_with_nul(&self) -> usize {
        let mut i = 0;
        loop {
            let ch = unsafe { self.0[i].assume_init() };
            i += 1;
            if ch == 0 {
                return i;
            }
        }
    }

    pub fn to_bytes(&self) -> &[u8] {
        let len = self.count_bytes();
        unsafe { assume_init_slice(&self.0[..len]) }
    }

    pub fn to_bytes_with_nul(&self) -> &[u8] {
        let len = self.count_bytes_with_nul();
        unsafe { assume_init_slice(&self.0[..len]) }
    }

    pub const fn capacity(&self) -> usize {
        self.0.len()
    }

    pub fn as_ptr(&self) -> *const c_char {
        self.0.as_ptr() as *const c_char
    }

    pub fn as_mut_ptr(&mut self) -> *mut c_char {
        self.0.as_mut_ptr() as *mut c_char
    }

    pub fn chars(&self) -> Chars {
        unsafe { Chars::new(self.as_ptr()) }
    }

    pub fn bytes(&self) -> Bytes {
        unsafe { Bytes::new(self.as_ptr()) }
    }
}

impl AsRef<CStr> for SaneString {
    fn as_ref(&self) -> &CStr {
        unsafe { CStr::from_ptr(self.as_ptr()) }
    }
}

impl AsRef<SaneStr> for SaneString {
    fn as_ref(&self) -> &SaneStr {
        self.borrow()
    }
}

impl<'a> IntoIterator for &'a SaneString {
    type IntoIter = Chars<'a>;
    type Item = char;

    fn into_iter(self) -> Self::IntoIter {
        unsafe { Chars::new(self.as_ptr()) }
    }
}

impl Borrow<SaneStr> for SaneString {
    fn borrow(&self) -> &SaneStr {
        SaneStr::from_cstr(self.as_ref())
    }
}

impl PartialEq for SaneString {
    fn eq(&self, other: &Self) -> bool {
        self.chars().zip(other.chars()).all(|(a, b)| a == b)
    }
}

impl Eq for SaneString {}

impl PartialOrd for SaneString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(Ord::cmp(self, other))
    }
}

impl Ord for SaneString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.chars()
            .zip(other.chars())
            .find_map(|(a, b)| {
                let ord = a.cmp(&b);
                if ord.is_eq() {
                    None
                } else {
                    Some(ord)
                }
            })
            .unwrap_or(Ordering::Equal)
    }
}

impl Hash for SaneString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for ch in self.bytes() {
            state.write_i8(ch)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Bytes<'a> {
    data: *const c_char,
    _phant: PhantomData<&'a c_char>,
}

impl<'a> Bytes<'a> {
    const unsafe fn new(data: *const c_char) -> Self {
        Self {
            data,
            _phant: PhantomData,
        }
    }
}

impl Iterator for Bytes<'_> {
    type Item = c_char;

    fn next(&mut self) -> Option<Self::Item> {
        let ch = unsafe { *self.data };
        if ch == 0 {
            None
        } else {
            self.data = unsafe { self.data.add(1) };
            Some(ch)
        }
    }
}

impl FusedIterator for Bytes<'_> {}

#[derive(Debug, Clone, Copy)]
pub struct Chars<'a> {
    data: *const c_char,
    _phant: PhantomData<&'a c_char>,
}

impl<'a> Chars<'a> {
    const unsafe fn new(data: *const c_char) -> Self {
        Self {
            data,
            _phant: PhantomData,
        }
    }
}

impl Iterator for Chars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        let ch = unsafe { *self.data };
        if ch == 0 {
            None
        } else {
            self.data = unsafe { self.data.add(1) };
            // Latin-1 is a subset of UTF-8
            Some(unsafe { char::from_u32_unchecked(ch as u32) })
        }
    }
}

impl FusedIterator for Chars<'_> {}

impl Display for Chars<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ch in *self {
            f.write_char(ch)?;
        }
        Ok(())
    }
}

impl Display for SaneStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.chars(), f)
    }
}

impl Debug for SaneStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let escaped = self.chars().flat_map(char::escape_debug);
        f.write_char('"')?;
        for ch in escaped {
            f.write_char(ch)?;
        }
        f.write_char('"')
    }
}

impl Display for SaneString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.chars(), f)
    }
}

impl Debug for SaneString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let escaped = self.chars().flat_map(char::escape_debug);
        f.write_char('"')?;
        for ch in escaped {
            f.write_char(ch)?;
        }
        f.write_char('"')
    }
}
