use core::fmt;
use std::{iter::FusedIterator, marker::PhantomData, ptr::NonNull};

use crate::{slice_util::boxed_slice_from_fn, sys, Error, Sane, SaneStr};

#[derive(Clone)]
pub struct DeviceDescription {
    buf: Vec<u8>,
    name_end: usize,
    vendor_end: usize,
    model_end: usize,
}

impl DeviceDescription {
    pub fn name(&self) -> &SaneStr {
        unsafe { SaneStr::new_unchecked(&self.buf[..self.name_end]) }
    }

    pub fn vendor(&self) -> &SaneStr {
        unsafe { SaneStr::new_unchecked(&self.buf[self.name_end..self.vendor_end]) }
    }

    pub fn model(&self) -> &SaneStr {
        unsafe { SaneStr::new_unchecked(&self.buf[self.vendor_end..self.model_end]) }
    }

    pub fn type_(&self) -> &SaneStr {
        unsafe { SaneStr::new_unchecked(&self.buf[self.model_end..]) }
    }

    fn from_sys_into(into: &mut Self, value: &sys::Device) {
        let name = unsafe { SaneStr::from_ptr(value.name) }.to_bytes_with_nul();
        let vendor = unsafe { SaneStr::from_ptr(value.vendor) }.to_bytes_with_nul();
        let model = unsafe { SaneStr::from_ptr(value.model) }.to_bytes_with_nul();
        let type_ = unsafe { SaneStr::from_ptr(value.type_) }.to_bytes_with_nul();

        into.name_end = name.len();
        into.vendor_end = into.name_end + vendor.len();
        into.model_end = into.vendor_end + model.len();
        let buf_size = into.model_end + type_.len();

        into.buf.clear();
        into.buf.reserve_exact(buf_size);
        into.buf.extend_from_slice(name);
        into.buf.extend_from_slice(vendor);
        into.buf.extend_from_slice(model);
        into.buf.extend_from_slice(type_);
    }
}

impl fmt::Debug for DeviceDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(stringify!(Device))
            .field("name", &self.name())
            .field("vendor", &self.vendor())
            .field("model", &self.model())
            .field("type", &self.type_())
            .finish()
    }
}

impl From<&sys::Device> for DeviceDescription {
    fn from(value: &sys::Device) -> Self {
        let mut res = Self {
            buf: Vec::new(),
            name_end: 0,
            vendor_end: 0,
            model_end: 0,
        };
        Self::from_sys_into(&mut res, value);
        res
    }
}

#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct DeviceDescriptionIter<'a> {
    data: NonNull<*const sys::Device>,
    _phant: PhantomData<&'a sys::Device>,
}

impl DeviceDescriptionIter<'_> {
    unsafe fn new(data: NonNull<*const sys::Device>) -> Self {
        Self {
            data,
            _phant: PhantomData,
        }
    }

    pub fn to_vec(self) -> Vec<DeviceDescription> {
        let mut buf = Vec::with_capacity(self.len());
        buf.extend(self);
        buf
    }

    pub fn to_boxed_slice(mut self) -> Box<[DeviceDescription]> {
        let len = self.len();
        boxed_slice_from_fn(len, |_| self.next().unwrap())
    }

    pub fn len(&self) -> usize {
        let mut count = 0;
        let mut ptr = self.data;
        while !unsafe { ptr.as_ref() }.is_null() {
            count += 1;
            // SAFETY: null-termination implies this memory being valid
            ptr = unsafe { ptr.add(1) };
        }
        count
    }

    pub fn is_empty(&self) -> bool {
        unsafe { self.data.as_ref() }.is_null()
    }

    /// Advances the iterator and gets the next item as a the reference provided by [`sys_get_devices`][`libsane_sys::sane_get_devices`].
    pub fn next_sys(&mut self) -> Option<&sys::Device> {
        let item = unsafe { self.data.as_ref().as_ref() }?;
        self.data = unsafe { self.data.add(1) };
        Some(item)
    }

    /// Writes the next description into the provided location, which re-uses the inner buffer.
    ///
    /// Returns `false` if this iterator is exhaused. In this case, `into` remains unchanged.
    #[must_use = "must check if the iterator is exhausted"]
    pub fn next_into(&mut self, into: &mut DeviceDescription) -> bool {
        let Some(item) = self.next_sys() else {
            return false;
        };
        DeviceDescription::from_sys_into(into, item);
        true
    }
}

impl Iterator for DeviceDescriptionIter<'_> {
    type Item = DeviceDescription;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_sys().map(DeviceDescription::from)
    }
}

impl FusedIterator for DeviceDescriptionIter<'_> {}

impl<A> Sane<A> {
    fn get_devices<R: 'static>(
        &self,
        local_only: bool,
        // this is needed because the references are only valid until the next
        // call to sys::sane_get_devices.
        extract: impl for<'a> FnOnce(DeviceDescriptionIter<'a>) -> R,
    ) -> Result<R, Error> {
        let device_list = unsafe { DeviceDescriptionIter::new(self.sys_get_devices(local_only)?) };
        Ok(extract(device_list))
    }

    pub fn get_devices_as_vec(&self, local_only: bool) -> Result<Vec<DeviceDescription>, Error> {
        self.get_devices(local_only, |it| it.to_vec())
    }

    pub fn get_devices_as_boxed_slice(
        &self,
        local_only: bool,
    ) -> Result<Box<[DeviceDescription]>, Error> {
        self.get_devices(local_only, |it| it.to_boxed_slice())
    }
}
