use std::{alloc::Layout, mem::MaybeUninit};

pub(crate) fn new_uninit_boxed_slice<T>(len: usize) -> Box<[MaybeUninit<T>]> {
    if len == 0 {
        return Box::new([]);
    }
    unsafe {
        let layout = Layout::array::<T>(len).unwrap();
        let raw = std::alloc::alloc(layout) as *mut MaybeUninit<T>;
        let raw = std::ptr::slice_from_raw_parts_mut(raw, len);
        Box::from_raw(raw)
    }
}

pub(crate) fn boxed_slice_from_fn<T>(len: usize, mut cb: impl FnMut(usize) -> T) -> Box<[T]> {
    let mut buf = new_uninit_boxed_slice(len);
    for i in 0..len {
        buf[i] = MaybeUninit::new(cb(i));
    }
    // SAFETY: entire slice was initialized above
    unsafe { assume_init_boxed_slice(buf) }
}

pub(crate) unsafe fn assume_init_boxed_slice<T>(mut data: Box<[MaybeUninit<T>]>) -> Box<[T]> {
    let raw = data.as_mut_ptr() as *mut T;
    let len = data.len();
    std::mem::forget(data);
    Box::from_raw(std::ptr::slice_from_raw_parts_mut(raw, len))
}

pub(crate) const unsafe fn assume_init_slice<T>(data: &[MaybeUninit<T>]) -> &[T] {
    std::slice::from_raw_parts(data.as_ptr() as *const T, data.len())
}

pub(crate) const fn slice_as_maybe_uninit<T>(data: &[T]) -> &[MaybeUninit<T>] {
    // SAFETY: MaybeUninit is repr(transparent)
    unsafe { std::slice::from_raw_parts(data.as_ptr() as *const MaybeUninit<T>, data.len()) }
}
