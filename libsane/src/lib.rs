mod device;
mod error;
mod fixed;
mod init_exit;
pub mod list;
mod proxied_sys;
pub(crate) mod slice_util;
pub mod string;
mod value;

use core::fmt;
use std::{cell::Cell, marker::PhantomData};

pub use ::libsane_sys as sys;
pub use device::*;
pub use error::Error;
pub use fixed::Fixed;
pub use init_exit::*;
pub use string::{SaneStr, SaneString};
pub use value::*;

/// Version of the `sane.h` header file.
pub const LIB_VERSION: Version =
    Version::new(sys::CURRENT_MAJOR as u8, sys::CURRENT_MINOR as u8, 0);

const fn sys_bool(v: bool) -> sys::Bool {
    match v {
        false => sys::FALSE,
        true => sys::TRUE,
    }
}

#[derive(Debug)]
pub struct Sane<A> {
    /// Sane is !Sync and Send iff A is Send
    _phant: PhantomData<Cell<A>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Version(sys::Int);

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(stringify!(Version))
            .field("major", &self.major())
            .field("minor", &self.minor())
            .field("build", &self.build())
            .finish()
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major(), self.minor(), self.build())
    }
}

impl Version {
    pub const fn new(major: u8, minor: u8, build: u16) -> Self {
        Self(sys::version_code(
            major as sys::Int,
            minor as sys::Int,
            build as sys::Int,
        ))
    }

    pub const fn major(&self) -> u8 {
        sys::version_major(self.0)
    }

    pub const fn minor(&self) -> u8 {
        sys::version_minor(self.0)
    }

    pub const fn build(&self) -> u16 {
        sys::version_build(self.0)
    }
}

impl AsRef<sys::Int> for Version {
    fn as_ref(&self) -> &sys::Int {
        // SAFETY: Self is repr(transparent) and inner type is sys::Int
        unsafe { &*(self as *const Self as *const sys::Int) }
    }
}

impl AsMut<sys::Int> for Version {
    fn as_mut(&mut self) -> &mut sys::Int {
        // SAFETY: Self is repr(transparent) and inner type is sys::Int
        unsafe { &mut *(self as *mut Self as *mut sys::Int) }
    }
}

/// The type this is implemented on needs to keep a reference to Sane.
pub trait WithSane {
    type Auth;

    /// Grants access to the Sane struct, which also guarantees that no other thread has acces to it.
    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R;
}

impl<A> WithSane for Sane<A> {
    type Auth = A;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        cb(self)
    }
}

impl<T: WithSane> WithSane for &T {
    type Auth = T::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        (**self).with_sane(cb)
    }
}

impl<T: WithSane> WithSane for Box<T> {
    type Auth = T::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        (**self).with_sane(cb)
    }
}

impl<T: WithSane> WithSane for std::rc::Rc<T> {
    type Auth = T::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        (**self).with_sane(cb)
    }
}

#[cfg(feature = "parking_lot")]
impl<T: WithSane> WithSane for parking_lot::Mutex<T> {
    type Auth = T::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        self.lock().with_sane(cb)
    }
}

#[cfg(feature = "parking_lot")]
impl<T: WithSane> WithSane for parking_lot::MutexGuard<'_, T> {
    type Auth = T::Auth;

    fn with_sane<R>(&self, cb: impl for<'b> FnOnce(&'b Sane<Self::Auth>) -> R) -> R {
        (**self).with_sane(cb)
    }
}

impl<T: WithSane> WithSane for std::sync::Mutex<T> {
    type Auth = T::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        self.lock().expect("poisoned Mutex").with_sane(cb)
    }
}

impl<T: WithSane> WithSane for std::sync::MutexGuard<'_, T> {
    type Auth = T::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        (**self).with_sane(cb)
    }
}

impl<T: WithSane> WithSane for std::sync::Arc<T> {
    type Auth = T::Auth;

    fn with_sane<R>(&self, cb: impl for<'a> FnOnce(&'a Sane<Self::Auth>) -> R) -> R {
        (**self).with_sane(cb)
    }
}
