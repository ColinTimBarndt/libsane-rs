pub mod frame_decoder;

use core::fmt;
use std::io;

use crate::{error, proxied_sys::IoMode, sys, DeviceHandle, Error, WithSane};

pub use frame_decoder::{DecodedImage, DecodedImageFormat, FrameDecodeError, FrameDecoder};

impl<S: WithSane> DeviceHandle<S> {
    pub fn scan_blocking(self) -> ScanReader<S> {
        ScanReader::new(self)
    }
}

pub struct ScanReader<S: WithSane> {
    device: DeviceHandle<S>,
    done: bool,
}

impl<S: WithSane> ScanReader<S> {
    fn new(device: DeviceHandle<S>) -> Self {
        Self {
            device,
            done: false,
        }
    }

    pub fn into_inner(mut self) -> DeviceHandle<S> {
        self.cancel();
        self.device
    }

    pub fn device(&self) -> &DeviceHandle<S> {
        &self.device
    }

    pub fn cancel(&mut self) {
        self.device.inner.cancel();
        self.done = true;
    }

    pub fn next_frame(&mut self) -> Result<Option<FrameReader<S>>, Error> {
        if self.done {
            return Ok(None);
        };
        let params = self.device.with_sane(|sane| {
            let handle = self.device.inner.handle;
            // SAFETY: handle is valid, library call is sequential (have access to Sane struct)
            unsafe { sane.sys_start(handle)? };
            // SAFETY: see above, and start has been called
            let res = unsafe { sane.sys_set_io_mode(handle, IoMode::Blocking) };
            // Blocking is always supported, but the backend might always return an error.
            // This is falsely documented behavior or a wrong backend implementation.
            if let Err(err) = res {
                if err.sys_status() != sys::Status::Unsupported {
                    return Err(err);
                }
            }
            // SAFETY: handle is valid, and call is sequential
            unsafe { sane.sys_get_parameters(handle) }
        })?;
        Ok(Some(FrameReader::new(self, params.into())))
    }
}

pub struct FrameReader<'a, S: WithSane> {
    scanner: &'a mut ScanReader<S>,
    params: FrameParameters,
    started: bool,
}

impl<'a, S: WithSane> FrameReader<'a, S> {
    fn new(scanner: &'a mut ScanReader<S>, params: FrameParameters) -> Self {
        Self {
            scanner,
            params,
            started: false,
        }
    }

    pub fn parameters(&self) -> &FrameParameters {
        &self.params
    }

    pub fn read_frame(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let last_frame = self.params.last_frame;
        self.scanner.device.with_sane(|sane| {
            self.started = true;
            // SAFETY: handle is valid, device is scanning, call is sequential
            let res = unsafe { sane.sys_read(self.scanner.device.inner.handle, buf) };
            if let Err(err) = &res {
                if matches!(err.sys_status(), sys::Status::Cancelled | sys::Status::Eof if last_frame) {
                    self.scanner.done = true;
                }
            }
            res
        })
    }

    pub fn read_full_frame(&mut self, buf_vec: &mut Vec<u8>) -> Result<(), Error> {
        assert!(
            !self.started,
            "attempt to read entire frame after partial read"
        );
        self.scanner.device.with_sane(|sane| {
            self.started = true;
            if self.params.last_frame {
                self.scanner.done = true;
            }
            let bytes_per_line = self.params.bytes_per_line;
            let lines = self.params.lines;
            let handle = self.scanner.device.inner.handle;
            if let Some(lines) = lines {
                let bytes_to_read = (bytes_per_line * lines) as usize;
                buf_vec.reserve_exact(bytes_to_read);
                let mut buf = &mut buf_vec.spare_capacity_mut()[..bytes_to_read];
                while !buf.is_empty() {
                    // SAFETY: handle is valid, device is scanning, call is sequential
                    let res = unsafe { sane.sys_read_uninit(handle, buf) };
                    match res {
                        Err(ref err) if err.sys_status() == sys::Status::Eof => {
                            panic!("too early eof")
                        }
                        Err(err) => return Err(err),
                        Ok(read_len) => {
                            debug_assert_ne!(read_len, 0);
                            buf = &mut buf[read_len..];
                        }
                    };
                }
                // SAFETY: bytes_to_read reserved length was fully initialized
                unsafe { buf_vec.set_len(buf_vec.len() + bytes_to_read) };
                Ok(())
            } else {
                // strategy:
                // - when only half was provided, half this number
                // - otherwise, increment by 1
                let mut try_lines = 32;
                loop {
                    let reserved_bytes = bytes_per_line as usize * try_lines;
                    buf_vec.reserve(reserved_bytes);
                    // note that this may be more than reservec_bytes,
                    // we just use all the Vec has given us
                    let buf = buf_vec.spare_capacity_mut();
                    // SAFETY: handle is valid, device is scanning, call is sequential
                    let res = unsafe { sane.sys_read_uninit(handle, buf) };
                    match res {
                        Err(ref err) if err.sys_status() == sys::Status::Eof => break,
                        Err(err) => return Err(err),
                        Ok(read_len) => {
                            debug_assert_ne!(read_len, 0);
                            // SAFETY: read_len bytes were initialized
                            unsafe { buf_vec.set_len(buf_vec.len() + read_len) }
                            if read_len < reserved_bytes / 2 {
                                try_lines /= 2;
                            } else {
                                try_lines += 1;
                            }
                        }
                    }
                }
                Ok(())
            }
        })
    }
}

impl<S: WithSane> io::Read for FrameReader<'_, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.read_frame(buf) {
            Ok(len) => Ok(len),
            Err(ref err) if err.status() == error::Status::Eof => Ok(0),
            Err(other) => Err(read_error_to_io(other)),
        }
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let len = buf.len();
        match self.read_full_frame(buf) {
            Ok(()) => Ok(buf.len() - len),
            Err(err) => Err(read_error_to_io(err)),
        }
    }
}

fn read_error_to_io(error: Error) -> io::Error {
    let kind = match error.status() {
        error::Status::Cancelled => io::ErrorKind::BrokenPipe,
        // This should be handled by returning length 0 in blocking mode
        // instead of returning it as an `io::Error`
        error::Status::Eof => io::ErrorKind::UnexpectedEof,
        error::Status::NoMem => io::ErrorKind::OutOfMemory,
        error::Status::AccessDenied => io::ErrorKind::PermissionDenied,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error)
}

#[derive(Clone, Copy)]
pub struct FrameParameters {
    format: sys::Frame,
    /// Whether this is the last frame of an image
    pub last_frame: bool,
    /// Size of one scanned line in bytes, which may include padding
    pub bytes_per_line: u32,
    /// Width of the frame in pixels
    pub pixels_per_line: u32,
    /// Height of the frame in pixels, if known
    pub lines: Option<u32>,
    /// Bits per pixel
    pub depth: u32,
}

impl FrameParameters {
    pub fn format(&self) -> FrameFormat {
        self.format.into()
    }

    pub fn sys_format(&self) -> sys::Frame {
        self.format
    }
}

impl fmt::Debug for FrameParameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(stringify!(ScanParameters))
            .field(
                "format",
                match self.format() {
                    FrameFormat::Unsupported => &self.format.0,
                    ref known => known,
                },
            )
            .field("last_frame", &self.last_frame)
            .field("bytes_per_line", &self.bytes_per_line)
            .field("pixels_per_line", &self.pixels_per_line)
            .field("lines", &self.lines)
            .field("depth", &self.depth)
            .finish()
    }
}

impl From<sys::Parameters> for FrameParameters {
    fn from(value: sys::Parameters) -> Self {
        Self {
            format: value.format,
            last_frame: value.last_frame != sys::FALSE as sys::Int,
            bytes_per_line: value.bytes_per_line as u32,
            pixels_per_line: value.pixels_per_line as u32,
            lines: if value.lines == -1 {
                None
            } else {
                Some(value.lines as u32)
            },
            depth: value.depth as u32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFormat {
    /// Band covering human visual range.
    Gray,
    /// Pixel-interleaved red/green/blue bands.
    Rgb,
    /// Red band of a red/green/blue image.
    Red,
    /// Green band of a red/green/blue image.
    Green,
    /// Blue band of a red/green/blue image.
    Blue,
    /// The scan format is unsupported by these bindings to SANE.
    Unsupported,
}

impl FrameFormat {
    pub const fn is_rgb(&self) -> bool {
        matches!(self, Self::Rgb | Self::Red | Self::Green | Self::Blue)
    }
}

impl From<sys::Frame> for FrameFormat {
    fn from(value: sys::Frame) -> Self {
        match value {
            sys::Frame::Gray => Self::Gray,
            sys::Frame::Rgb => Self::Rgb,
            sys::Frame::Red => Self::Red,
            sys::Frame::Green => Self::Green,
            sys::Frame::Blue => Self::Blue,
            _ => Self::Unsupported,
        }
    }
}

impl<S: WithSane> DeviceHandle<S> {
    pub fn get_parameters(&self) -> Result<FrameParameters, Error> {
        self.inner.get_parameters()
    }
}
