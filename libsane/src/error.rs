use std::{
    error::Error as StdError,
    ffi::CStr,
    fmt::{Debug, Display},
};

use crate::sys;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Error {
    status: sys::Status,
}

impl Error {
    pub const fn status(&self) -> Status {
        Status::from_sys(self.status)
    }

    pub const fn sys_status(&self) -> sys::Status {
        self.status
    }

    pub fn message(&self) -> String {
        let msg = unsafe { CStr::from_ptr(sys::sane_strstatus(self.status)) };
        msg.to_string_lossy().into_owned()
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = self.status();

        f.debug_struct(stringify!(Error))
            .field(
                "status",
                match status {
                    Status::Unknown => &self.status.0,
                    _ => &status,
                },
            )
            .finish()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl StdError for Error {}

pub(crate) fn status_result(status: sys::Status) -> Result<(), Error> {
    match status {
        sys::Status::Good => Ok(()),
        status => Err(Error { status }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unsupported,
    Cancelled,
    DeviceBusy,
    Inval,
    Eof,
    Jammed,
    NoDocs,
    CoverOpen,
    IoError,
    NoMem,
    AccessDenied,
    Unknown,
}

impl Status {
    pub const fn from_sys(value: sys::Status) -> Self {
        match value {
            sys::Status::Unsupported => Self::Unsupported,
            sys::Status::Cancelled => Self::Cancelled,
            sys::Status::DeviceBusy => Self::DeviceBusy,
            sys::Status::Inval => Self::Inval,
            sys::Status::Eof => Self::Eof,
            sys::Status::Jammed => Self::Jammed,
            sys::Status::NoDocs => Self::NoDocs,
            sys::Status::CoverOpen => Self::CoverOpen,
            sys::Status::IoError => Self::IoError,
            sys::Status::NoMem => Self::NoMem,
            sys::Status::AccessDenied => Self::AccessDenied,
            _ => Self::Unknown,
        }
    }
}

impl From<sys::Status> for Status {
    fn from(value: sys::Status) -> Self {
        Self::from_sys(value)
    }
}
