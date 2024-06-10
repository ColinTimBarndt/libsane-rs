use core::fmt;
use std::marker::PhantomData;

use crate::{sys, SaneStr};

#[repr(transparent)]
pub struct ListIter<'a, T> {
    data: *const T,
    _phant: PhantomData<&'a T>,
}

impl<T> Clone for ListIter<'_, T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data,
            _phant: PhantomData,
        }
    }
}

impl<'a, T> ListIter<'a, T> {
    pub(crate) unsafe fn new(data: *const T) -> Self {
        Self {
            data,
            _phant: PhantomData,
        }
    }

    pub fn to_slice(&self) -> &'a [T] {
        let len = self.count_items();
        unsafe { std::slice::from_raw_parts(self.data, len) }
    }

    pub fn count_items(&self) -> usize {
        let mut ptr = self.data;
        let mut len = 0;
        while !ptr.is_null() {
            len += 1;
            ptr = unsafe { ptr.add(1) };
        }
        len
    }
}

impl<T> Default for ListIter<'_, T> {
    fn default() -> Self {
        Self {
            data: std::ptr::null(),
            _phant: PhantomData,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for ListIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

impl<'a, T> Iterator for ListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let item = unsafe { self.data.as_ref() }?;
        self.data = unsafe { self.data.add(1) };
        Some(item)
    }
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
    pub(crate) unsafe fn new(data: *const sys::StringConst) -> Self {
        Self {
            data,
            _phant: PhantomData,
        }
    }

    pub fn count_items(&self) -> usize {
        let mut ptr = self.data;
        let mut len = 0;
        while !unsafe { *ptr }.is_null() {
            len += 1;
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
        let item = unsafe { *self.data };
        if item.is_null() {
            None
        } else {
            self.data = unsafe { self.data.add(1) };
            Some(unsafe { SaneStr::from_ptr(item) })
        }
    }
}
