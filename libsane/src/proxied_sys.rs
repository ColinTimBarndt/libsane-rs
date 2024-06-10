use std::{
    ffi::c_void,
    mem::MaybeUninit,
    os::fd::{FromRawFd, OwnedFd},
    ptr::NonNull,
};

use crate::{error, sys, sys_bool, ControlInfo, Error, Sane, SaneStr};

impl<A> Sane<A> {
    /// This function can be used to query the list of devices that are available. If the
    /// function executes successfully, it returns a pointer to a NULL terminated array of
    /// pointers to [`sys::Device`]. The returned list is guaranteed to remain unchanged
    /// and valid until another call to this function is performed. This function can be
    /// called repeatedly to detect when new devices become available. If argument
    /// `local_only` is true, only local devices are returned (devices directly attached
    /// to the machine that SANE is running on). If it is false, the device list includes
    /// all remote devices that are accessible to the SANE library.
    pub(crate) unsafe fn sys_get_devices(
        &self,
        local_only: bool,
    ) -> Result<NonNull<*const sys::Device>, Error> {
        let mut list = std::ptr::null_mut();
        error::status_result(sys::sane_get_devices(&mut list, sys_bool(local_only)))?;
        debug_assert!(!list.is_null());
        Ok(NonNull::new_unchecked(list))
    }
    /// This function is used to establish a connection to a particular device.
    /// As a special case, specifying a zero-length string as the device requests opening
    /// the first available device (if there is such a device).
    ///
    /// # Errors
    /// - [`DeviceBusy`][`crate::error::Status::DeviceBusy`]: The device is currently busy (in use by somebody else).
    /// - [`Inval`][`crate::error::Status::Inval`]: The device name is not valid.
    /// - [`IoError`][`crate::error::Status::IoError`]: An error occurred while communicating with the device.
    /// - [`NoMem`][`crate::error::Status::NoMem`]: An insufficient amount of memory is available.
    /// - [`AccessDenied`][`crate::error::Status::AccessDenied`]: Access to the device has been denied due to insufficient or invalid authentication.
    pub(crate) unsafe fn sys_open(&self, devicename: &SaneStr) -> Result<NonNull<c_void>, Error> {
        let mut handle = std::ptr::null_mut();
        error::status_result(sys::sane_open(devicename.as_ptr(), &mut handle))?;
        debug_assert!(!handle.is_null());
        Ok(NonNull::new_unchecked(handle))
    }

    /// This function terminates the association between the device handle and the device
    /// it represents. If the device is presently active, a call to [`Self::sys_cancel`] is
    /// performed first. After this function returns, `handle` must not be used anymore.
    pub(crate) unsafe fn sys_close(&self, handle: NonNull<c_void>) {
        sys::sane_close(handle.as_ptr())
    }

    /// This function is used to access option descriptors. The function returns the option
    /// descriptor for option `index` of the device represented by `handle`. Option number 0
    /// is guaranteed to be a valid option. Its value is an integer that specifies the
    /// number of options that are available for device handle h (the count includes option
    /// 0). If n is not a valid option index, the function returns NULL. The returned option
    /// descriptor is guaranteed to remain valid (and at the returned address) until the device
    /// is closed.
    pub(crate) unsafe fn sys_get_option_descriptor(
        &self,
        handle: NonNull<c_void>,
        index: u32,
    ) -> *const sys::OptionDescriptor {
        sys::sane_get_option_descriptor(handle.as_ptr(), index.try_into().expect("invalid index"))
    }

    /// Get current option value.
    ///
    /// # Safety
    /// The `value` pointer must point to the correct data type depending on the type
    /// of option and the memory location must be sufficiently large.
    pub(crate) unsafe fn sys_get_option_value(
        &self,
        handle: NonNull<c_void>,
        index: u32,
        value: *mut c_void,
    ) -> Result<(), Error> {
        error::status_result(sys::sane_control_option(
            handle.as_ptr(),
            index.try_into().expect("invalid index"),
            sys::Action::GetValue,
            value,
            std::ptr::null_mut(),
        ))
    }

    /// Set option value. The option value may be modified by the backend if the value
    /// cannot be set exactly.
    /// Additional information on how well the request has been met is returned.
    ///
    /// # Safety
    /// The `value` pointer must point to the correct data type depending on the type
    /// of option and the memory location must be sufficiently large.
    pub(crate) unsafe fn sys_set_option_value(
        &self,
        handle: NonNull<c_void>,
        index: u32,
        value: *mut c_void,
    ) -> Result<ControlInfo, Error> {
        let mut info: sys::Int = 0;
        error::status_result(sys::sane_control_option(
            handle.as_ptr(),
            index.try_into().expect("invalid index"),
            sys::Action::SetValue,
            value,
            &mut info,
        ))?;
        Ok(ControlInfo::from_bits_retain(info as u32))
    }

    /// Turn on automatic mode. Backend or device will automatically select an appropriate
    /// value. This mode remains effective until overridden by an explicit set value
    /// request.
    pub(crate) unsafe fn sys_set_option_auto(
        &self,
        handle: NonNull<c_void>,
        index: u32,
    ) -> Result<(), Error> {
        error::status_result(sys::sane_control_option(
            handle.as_ptr(),
            index.try_into().expect("invalid index"),
            sys::Action::SetAuto,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ))?;
        Ok(())
    }

    /// This function is used to obtain the current scan parameters. The returned parameters
    /// are guaranteed to be accurate between the time a scan has been started ([`Self::sys_start`]
    /// has been called) and the completion of that request. Outside of that window, the
    /// returned values are best-effort estimates of what the parameters will be when
    /// [`Self::sys_start`] gets invoked. Calling this function before a scan has actually started
    /// allows, for example, to get an estimate of how big the scanned image will be.
    pub(crate) unsafe fn sys_get_parameters(
        &self,
        handle: NonNull<c_void>,
    ) -> Result<sys::Parameters, Error> {
        let mut params = sys::Parameters::default();
        error::status_result(sys::sane_get_parameters(handle.as_ptr(), &mut params))?;
        Ok(params)
    }

    /// This function initiates acquisition of an image from the device represented by
    /// `handle`.
    ///
    /// # Errors
    /// - [`Cancelled`][`crate::error::Status::Cancelled`]: The operation was cancelled through a call to [`Self::sys_cancel`].
    /// - [`DeviceBusy`][`crate::error::Status::DeviceBusy`]: The device is busy. The operation should be retried later.
    /// - [`Jammed`][`crate::error::Status::Jammed`]: The document feeder is jammed.
    /// - [`NoDocs`][`crate::error::Status::NoDocs`]: The document feeder is out of documents.
    /// - [`CoverOpen`][`crate::error::Status::CoverOpen`]: The scanner cover is open.
    /// - [`IoError`][`crate::error::Status::IoError`]: An error occurred while communicating with the device.
    /// - [`NoMem`][`crate::error::Status::NoMem`]: An insufficient amount of memory is available.
    /// - [`Inval`][`crate::error::Status::Inval`]: The scan cannot be started with the current set of options. The
    ///   frontend should reload the option descriptors, as if SANE_INFO_RELOAD_OPTIONS had been returned from
    ///   a call to sane_control_option(), since the device’s capabilities may have changed.
    pub(crate) unsafe fn sys_start(&self, handle: NonNull<c_void>) -> Result<(), Error> {
        error::status_result(sys::sane_start(handle.as_ptr()))
    }

    /// This function is used to read image data from the device represented by
    /// `handle`. The number of bytes written to `buf` is returned.
    ///
    /// If this function is called when no data is available, one of two things may
    /// happen, depending on the I/O mode that is in effect for `handle`.
    ///
    /// 1. If the device is in blocking I/O mode (the default mode), the call blocks until
    ///    at least one data byte is available (or until some error occurs).
    /// 2. If the device is in non-blocking I/O mode, the call returns immediately with
    ///    [`Ok`][`std::result::Result::Ok`] and zero bytes read.
    ///
    /// The I/O mode of the handle can be set via a call to [`Self::sys_set_io_mode`].
    ///
    /// # Safety
    /// The device must be scanning, i.e. this function must be called after [`Self::sys_start`] and before [`Self::sys_read`]
    /// fails with status [`Eof`][`crate::error::Status::Eof`].
    ///
    /// # Errors
    /// - [`Cancelled`][`crate::error::Status::Cancelled`]: The operation was cancelled through a call to [`Self::sys_cancel`].
    /// - [`Eof`][`crate::error::Status::Eof`]: No more data is available for the current frame.
    /// - [`Jammed`][`crate::error::Status::Jammed`]: The document feeder is jammed.
    /// - [`NoDocs`][`crate::error::Status::NoDocs`]: The document feeder is out of documents.
    /// - [`CoverOpen`][`crate::error::Status::CoverOpen`]: The scanner cover is open.
    /// - [`IoError`][`crate::error::Status::IoError`]: An error occurred while communicating with the device.
    /// - [`NoMem`][`crate::error::Status::NoMem`]: An insufficient amount of memory is available.
    /// - [`AccessDenied`][`crate::error::Status::AccessDenied`]: Access to the device has been denied due to insufficient or invalid authentication.
    pub(crate) unsafe fn sys_read(
        &self,
        handle: NonNull<c_void>,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        let mut length = 0;
        error::status_result(sys::sane_read(
            handle.as_ptr(),
            buf.as_mut_ptr(),
            buf.len().min(sys::Int::MAX as usize) as sys::Int,
            &mut length,
        ))?;
        Ok(length.try_into().unwrap())
    }

    /// See [`Self::sys_read`].
    pub(crate) unsafe fn sys_read_uninit(
        &self,
        handle: NonNull<c_void>,
        buf: &mut [MaybeUninit<u8>],
    ) -> Result<usize, Error> {
        let mut length = 0;
        error::status_result(sys::sane_read(
            handle.as_ptr(),
            buf.as_mut_ptr() as *mut sys::Byte,
            buf.len().min(sys::Int::MAX as usize) as sys::Int,
            &mut length,
        ))?;
        Ok(length.try_into().unwrap())
    }

    /// This function is used to immediately or as quickly as possible cancel the currently
    /// pending operation of the device represented by this handle.
    ///
    /// This function can be called at any time (as long as this handle is a valid handle)
    /// but usually affects long-running operations only (such as image is acquisition). It
    /// is safe to call this function asynchronously (e.g., from within a signal handler).
    /// It is important to note that completion of this operation does not imply that the
    /// currently pending operation has been cancelled. It only guarantees that
    /// cancellation has been initiated. Cancellation completes only when the cancelled
    /// call returns (typically with a status value of
    /// [`Cancelled`][`crate::error::Status::Cancelled`]). Since the SANE API does not require
    /// any other operations to be re-entrant, this implies that a frontend must not call any
    /// other operation until the cancelled operation has returned.
    pub(crate) unsafe fn sys_cancel(handle: NonNull<c_void>) {
        sys::sane_cancel(handle.as_ptr())
    }

    /// This function is used to set the I/O mode of this handle. The I/O mode can be either
    /// blocking or non-blocking. This function can be called only after a call to
    /// [`Self::sys_start_scan`] has been performed.
    ///
    /// By default, newly opened handles operate in blocking mode. A backend may elect not to
    /// support non-blocking I/O mode. In such a case the status value
    /// [`Unsupported`][`crate::error::Status::Unsupported`] is returned. Blocking I/O must be
    /// supported by all backends, so calling this function with argument [`IoMode::Blocking`]
    /// is guaranteed to complete successfully.
    ///
    /// # Errors
    /// - [`Inval`][`crate::error::Status::Inval`]: No image acquisition is pending.
    /// - [`Unsupported`][`crate::error::Status::Unsupported`]: The backend does not support
    ///   the requested I/O mode.
    pub(crate) unsafe fn sys_set_io_mode(
        &self,
        handle: NonNull<c_void>,
        mode: IoMode,
    ) -> Result<(), Error> {
        error::status_result(sys::sane_set_io_mode(
            handle.as_ptr(),
            match mode {
                IoMode::Blocking => sys::FALSE,
                IoMode::NonBlocking => sys::TRUE,
            },
        ))
    }

    /// This function is used to obtain a (platform-specific) file-descriptor for this handle
    /// that is readable if and only if image data is available (i.e., when a call to [`Self::sys_read`]
    /// will return at least one byte of data). If the call completes successfully, the select
    /// file-descriptor is returned.
    ///
    /// This function can be called only after a call to [`Self::sys_start`] has been performed
    /// and the returned file-descriptor is guaranteed to remain valid for the duration of the
    /// current image acquisition (i.e., until [`Self::sys_cancel`] or [`Self::sys_start`] get called
    /// again or until [`Self::sys_read`] returns with status [`Eof`][`crate::error::Status::Eof`]).
    /// Indeed, a backend must guarantee to close the returned select file descriptor at the point
    /// when the next [`Self::sys_read`] call would return [`Eof`][`crate::error::Status::Eof`]. This
    /// is necessary to ensure the application can detect when this condition occurs without
    /// actually having to call [`Self::sys_read`].
    ///
    /// A backend may elect not to support this operation. In such a case, the function returns with
    /// status code [`Unsupported`][`crate::error::Status::Unsupported`].
    ///
    /// Note that the only operation supported by the returned file-descriptor is a host
    /// operating-system dependent test whether the file-descriptor is readable (e.g., this test can
    /// be implemented using select() or poll() under UNIX). If any other operation is performed on
    /// the file descriptor, the behavior of the backend becomes unpredictable. Once the
    /// file-descriptor signals "readable" status, it will remain in that state until a call to
    /// [`Self::sys_read`] is performed. Since many input devices are very slow, support for this
    /// operation is strongly encouraged as it permits an application to do other work while image
    /// acquisition is in progress.
    ///
    /// # Safety
    /// The device must be scanning, i.e. this function must be called after [`Self::sys_start`] and before [`Self::sys_read`]
    /// fails with status [`Eof`][`crate::error::Status::Eof`] or the image acquisition is cancelled.
    /// The file descriptor must be closed afterwards.
    ///
    /// # Errors
    /// - [`Inval`][`crate::error::Status::Inval`]: No image acquisition is pending.
    /// - [`Unsupported`][`crate::error::Status::Unsupported`]: The backend does not support
    ///   the requested I/O mode.
    pub(crate) unsafe fn sys_get_select_fd(
        &self,
        handle: NonNull<c_void>,
    ) -> Result<OwnedFd, Error> {
        let mut fd = 0;
        error::status_result(sys::sane_get_select_fd(handle.as_ptr(), &mut fd))?;
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoMode {
    Blocking,
    NonBlocking,
}
