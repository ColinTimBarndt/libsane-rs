use core::fmt;
use std::marker::PhantomData;

use crate::{sys, SaneStr};

/// # Safety
/// - `T` must have the same size as `i32` and at most the same alignment.
/// - `data` must have at least the same alignment as `i32`.
/// - `data` must point to a `i32` length and the next length values must be valid `T`s.
pub(crate) unsafe fn new_word_list<'a, T>(data: *const i32) -> &'a [T] {
    debug_assert_eq!(std::mem::size_of::<T>(), 4);
    debug_assert!(std::mem::align_of::<T>() <= 4);
    debug_assert!(data.is_aligned());
    debug_assert!((data as *const T).is_aligned());
    debug_assert!(*data >= 0);
    // SAFETY: data is a valid `u32` representing the size
    let len = *data as usize;
    // SAFETY: the next len values are `T`s layout-compatible with `u32`
    let data = data.add(1) as *const T;
    std::slice::from_raw_parts(data, len)
}

#[repr(transparent)]
pub struct SaneStrListIter<'a> {
    data: *const sys::StringConst,
    _phant: PhantomData<&'a sys::StringConst>,
}

impl Clone for SaneStrListIter<'_> {
    fn clone(&self) -> Self {
        Self {
            data: self.data,
            _phant: PhantomData,
        }
    }
}

impl<'a> SaneStrListIter<'a> {
    /// # Safety
    /// The pointer is a null-terminated C-String pointer list.
    pub(crate) unsafe fn new(data: *const sys::StringConst) -> Self {
        Self {
            data,
            _phant: PhantomData,
        }
    }

    pub fn count_items(&self) -> usize {
        let mut ptr = self.data;
        let mut len = 0;
        // SAFETY: Until the null pointer, all pointers are valid
        while !unsafe { *ptr }.is_null() {
            len += 1;
            // SAFETY: No null pointer => next value is part of this list
            ptr = unsafe { ptr.add(1) };
        }
        len
    }
}

impl Default for SaneStrListIter<'_> {
    fn default() -> Self {
        Self {
            data: std::ptr::null(),
            _phant: PhantomData,
        }
    }
}

impl fmt::Debug for SaneStrListIter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

impl<'a> Iterator for SaneStrListIter<'a> {
    type Item = &'a SaneStr;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: Until the null pointer, all pointers are valid
        let item = unsafe { *self.data };
        if item.is_null() {
            None
        } else {
            // SAFETY: No null pointer => next value is part of this list
            self.data = unsafe { self.data.add(1) };
            // SAFETY: item is a valid C-String pointer
            Some(unsafe { SaneStr::from_ptr(item) })
        }
    }
}
