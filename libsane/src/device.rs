pub mod enumerate;
pub mod options;
pub mod scan;

use core::ffi::c_void;
use core::ptr::NonNull;
use std::mem::ManuallyDrop;

use bitflags::bitflags;

use crate::{sys, Error, Sane, SaneStr, WithSane};

pub use enumerate::DeviceDescription;

pub(crate) struct RawDeviceHandle<S: WithSane> {
    handle: NonNull<c_void>,
    sane: S,
}

impl<S: WithSane> RawDeviceHandle<S> {
    pub fn map_sane<N: WithSane>(self, map_fn: impl FnOnce(S) -> N) -> RawDeviceHandle<N> {
        let handle = self.handle;
        // Prevents the device from being closed.
        let mut this = ManuallyDrop::new(self);

        RawDeviceHandle {
            handle,
            // SAFETY: This copies the value, but the original is ManuallyDrop and never accessed again.
            sane: map_fn(unsafe { (&mut this.sane as *mut S).read() }),
        }
    }

    pub(crate) fn get_option(&self, index: u32) -> Option<options::DeviceOption<S>> {
        let descriptor =
            // SAFETY: call is synchronized and device is not closed.
            self.with_sane(|sane| unsafe { sane.sys_get_option_descriptor(self.handle, index) });
        if descriptor.is_null() {
            None
        } else {
            // SAFETY: device option was obtained from the C library, thus valid by the specification.
            Some(unsafe { options::DeviceOption::new(self, descriptor, index) })
        }
    }

    pub fn get_parameters(&self) -> Result<scan::FrameParameters, Error> {
        // SAFETY: call is synchronized and device is not closed.
        self.with_sane(|sane| unsafe { sane.sys_get_parameters(self.handle) })
            .map(scan::FrameParameters::from)
    }

    pub fn cancel(&self) {
        // SAFETY: The handle is valid, no synchronization needed by specification.
        unsafe { Sane::<()>::sys_cancel(self.handle) }
    }
}

// SAFETY: C API access needs to be sequential and can move to another thread.
unsafe impl<S: WithSane> Send for RawDeviceHandle<S> where S: Send {}

// SAFETY: Every access to the handle is done though `S` and thus guarded.
unsafe impl<S: WithSane> Sync for RawDeviceHandle<S> where S: Sync {}

impl<S: WithSane> Drop for RawDeviceHandle<S> {
    fn drop(&mut self) {
        self.sane
            // SAFETY: This handle is dropped, which means that nothing else is referencing any resource to this handle.
            .with_sane(|sane| unsafe { sane.sys_close(self.handle) });
    }
}

impl<S: WithSane> WithSane for RawDeviceHandle<S> {
    type Auth = S::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        self.sane.with_sane(cb)
    }
}

bitflags! {
    pub struct ControlInfo: u32 {
        const INEXACT = sys::INFO_INEXACT;
        const RELOAD_OPTIONS = sys::INFO_RELOAD_OPTIONS;
        const RELOAD_PARAMS = sys::INFO_RELOAD_PARAMS;
    }
}

pub struct DeviceHandle<S: WithSane> {
    inner: RawDeviceHandle<S>,
}

impl<S: WithSane> DeviceHandle<S> {
    pub fn map_sane<N: WithSane>(self, map_fn: impl FnOnce(S) -> N) -> DeviceHandle<N> {
        DeviceHandle {
            inner: self.inner.map_sane(map_fn),
        }
    }
}

impl<A> Sane<A> {
    pub fn connect(
        &self,
        devicename: &(impl AsRef<SaneStr> + ?Sized),
    ) -> Result<DeviceHandle<&Self>, Error> {
        Self::connect_with(self, devicename)
    }

    pub fn connect_with<S: WithSane<Auth = A>>(
        with: S,
        devicename: &(impl AsRef<SaneStr> + ?Sized),
    ) -> Result<DeviceHandle<S>, Error> {
        // SAFETY: call is synchronized.
        let handle = with.with_sane(|sane| unsafe { sane.sys_open(devicename.as_ref()) })?;

        Ok(DeviceHandle {
            inner: RawDeviceHandle { handle, sane: with },
        })
    }
}

impl<S: WithSane> WithSane for DeviceHandle<S> {
    type Auth = S::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        self.inner.with_sane(cb)
    }
}
