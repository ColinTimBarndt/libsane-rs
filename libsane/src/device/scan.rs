use core::fmt;
use std::{io, mem::MaybeUninit};

use crate::{
    error, proxied_sys::IoMode, slice_util::slice_as_maybe_uninit, sys, DeviceHandle, Error,
    WithSane,
};

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
        if self.done {
            return;
        }
        self.device.inner.cancel();
        self.done = true;
    }

    pub fn next_frame(&mut self) -> Result<Option<FrameReader<S>>, Error> {
        if self.done {
            return Ok(None);
        };
        let params = self.device.with_sane(|sane| unsafe {
            let handle = self.device.inner.handle;
            sane.sys_start(handle)?;
            // Blocking is always supported, but the backend might always return an error.
            // This is falsely documented behavior or a wrong backend implementation.
            let res = sane.sys_set_io_mode(handle, IoMode::Blocking);
            if let Err(err) = res {
                if err.sys_status() != sys::Status::Unsupported {
                    return Err(err);
                }
            }
            sane.sys_get_parameters(handle)
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
                // buf_vec reserved length was fully initialized
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
                    let res = unsafe { sane.sys_read_uninit(handle, buf) };
                    match res {
                        Err(ref err) if err.sys_status() == sys::Status::Eof => break,
                        Err(err) => return Err(err),
                        Ok(read_len) => {
                            debug_assert_ne!(read_len, 0);
                            // read_len bytes were initialized
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

#[derive(Debug, Clone)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
    state: FrameDecoderState,
    width: u32,
    height: u32,
    black_and_white_as_bytes: bool,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            state: FrameDecoderState::Initial,
            width: 0,
            height: 0,
            black_and_white_as_bytes: false,
        }
    }

    pub const fn with_buffer(buffer: Vec<u8>) -> Self {
        Self {
            buffer,
            state: FrameDecoderState::Initial,
            width: 0,
            height: 0,
            black_and_white_as_bytes: false,
        }
    }

    /// By default, black and white images are represented as a packed big-endian bitmap.
    /// Setting this to true will store every pixel as a byte with value `0` or `1`.
    pub fn decode_black_and_white_as_bytes(&mut self, do_it: bool) {
        self.black_and_white_as_bytes = do_it;
    }
}

impl FrameDecoder {
    pub const fn is_done(&self) -> bool {
        self.state.is_done()
    }

    pub fn into_image(self) -> Result<DecodedImage, Vec<u8>> {
        if let FrameDecoderState::Done(format) = self.state {
            Ok(DecodedImage {
                data: self.buffer,
                format,
                width: self.width,
                height: self.height,
            })
        } else {
            Err(self.buffer)
        }
    }

    pub fn write(
        &mut self,
        frame: &[u8],
        params: &FrameParameters,
    ) -> Result<(), FrameDecodeError> {
        if self.is_done() {
            return Err(FrameDecodeError::AlreadyDone);
        }

        if params.depth == 0 {
            return Err(FrameDecodeError::InvalidParameters);
        }

        let Ok(frame_len) = u32::try_from(frame.len()) else {
            return Err(FrameDecodeError::InvalidParameters);
        };

        if frame_len % params.bytes_per_line != 0 {
            return Err(FrameDecodeError::InvalidParameters);
        }

        let f_width = params.pixels_per_line;
        let f_height = frame_len / params.bytes_per_line;
        if params.lines.is_some_and(|l| l != f_height) {
            return Err(FrameDecodeError::InvalidParameters);
        }

        match (&mut self.state, params.sys_format()) {
            // black and white
            (FrameDecoderState::Initial, sys::Frame::Gray) if params.depth == 1 => {
                if params.pixels_per_line & 0b111 != 0 {
                    // only supports whole byte lines
                    return Err(FrameDecodeError::UnsupportedParameters);
                }
                let dst_len;
                if self.black_and_white_as_bytes {
                    dst_len = f_width as usize * f_height as usize;
                    self.buffer.reserve_exact(dst_len);
                    let bytes = frame
                        .chunks_exact(params.bytes_per_line as usize)
                        .flat_map(|line| line[..(params.pixels_per_line / 8) as usize].iter());
                    let dst = &mut self.buffer.spare_capacity_mut()[..dst_len];
                    for (i, byte) in bytes.enumerate() {
                        for j in 0..8 {
                            // Note: 0 = white, 1 = black
                            dst[8 * i + j] =
                                MaybeUninit::new(if *byte & (0x80 >> j) != 0 { 0 } else { 1 });
                        }
                    }
                } else {
                    dst_len = f_width as usize * f_height as usize / 8;
                    self.buffer.reserve_exact(dst_len);
                    let bytes = frame
                        .chunks_exact(params.bytes_per_line as usize)
                        .flat_map(|line| line[..(params.pixels_per_line / 8) as usize].iter());
                    for (dst, src) in self.buffer.spare_capacity_mut()[..dst_len]
                        .iter_mut()
                        .zip(bytes)
                    {
                        // Note: 0 = white, 1 = black
                        *dst = MaybeUninit::new(!*src);
                    }
                }
                // SAFETY: dst_len spare capacity was fully initialized
                unsafe { self.buffer.set_len(self.buffer.len() + dst_len) }
                self.width = f_width;
                self.height = f_height;
                self.state = FrameDecoderState::Done(DecodedImageFormat::BlackAndWhite);
                Ok(())
            }
            // grayscale
            (FrameDecoderState::Initial, sys::Frame::Gray) => {
                if params.depth & 0b111 != 0 {
                    // only supports whole byte channels
                    return Err(FrameDecodeError::UnsupportedParameters);
                }
                let bytes_per_pixel = params.depth / 8;
                let bytes = frame
                    .chunks_exact(params.bytes_per_line as usize)
                    .flat_map(|line| {
                        line[..params.pixels_per_line as usize * bytes_per_pixel as usize].iter()
                    });
                let dst_len = f_width as usize * f_height as usize * bytes_per_pixel as usize;
                self.buffer.reserve_exact(dst_len);
                for (dst, src) in self.buffer.spare_capacity_mut()[..dst_len]
                    .iter_mut()
                    .zip(bytes)
                {
                    *dst = MaybeUninit::new(*src);
                }
                // SAFETY: dst_len spare capacity was fully initialized
                unsafe { self.buffer.set_len(self.buffer.len() + dst_len) }
                self.width = f_width;
                self.height = f_height;
                self.state = FrameDecoderState::Done(DecodedImageFormat::Gray { bytes_per_pixel });
                Ok(())
            }
            // rgb
            (FrameDecoderState::Initial, sys::Frame::Rgb) => {
                if params.depth & 0b111 != 0 {
                    // only supports whole byte channels
                    return Err(FrameDecodeError::UnsupportedParameters);
                }
                let bytes_per_channel = params.depth / 8;
                let bytes_per_pixel = bytes_per_channel * 3;
                let bytes = frame
                    .chunks_exact(params.bytes_per_line as usize)
                    .flat_map(|line| {
                        line[..params.pixels_per_line as usize * bytes_per_pixel as usize].iter()
                    });
                let dst_len = f_width as usize * f_height as usize * bytes_per_pixel as usize;
                self.buffer.reserve_exact(dst_len);
                for (dst, src) in self.buffer.spare_capacity_mut()[..dst_len]
                    .iter_mut()
                    .zip(bytes)
                {
                    *dst = MaybeUninit::new(*src);
                }
                // SAFETY: spare capacity was fully initialized
                unsafe { self.buffer.set_len(self.buffer.len() + dst_len) }
                self.width = f_width;
                self.height = f_height;
                self.state = FrameDecoderState::Done(DecodedImageFormat::Rgb { bytes_per_channel });
                Ok(())
            }
            // rgb parts
            (
                FrameDecoderState::Initial,
                channel @ (sys::Frame::Red | sys::Frame::Green | sys::Frame::Blue),
            ) => {
                if params.depth & 0b111 != 0 {
                    // only supports whole byte channels
                    return Err(FrameDecodeError::UnsupportedParameters);
                }
                let bytes_per_channel = params.depth / 8;
                let bytes_per_pixel = bytes_per_channel * 3;
                let offset = bytes_per_pixel as usize
                    * match channel {
                        sys::Frame::Red => 0,
                        sys::Frame::Green => 1,
                        sys::Frame::Blue => 2,
                        _ => unreachable!(),
                    };
                let dst_len = f_width as usize * f_height as usize * bytes_per_pixel as usize;
                self.buffer.reserve_exact(dst_len);
                Self::write_channel(
                    &mut self.buffer.spare_capacity_mut()[..dst_len],
                    frame,
                    params.bytes_per_line as usize,
                    f_width as usize,
                    bytes_per_channel as usize,
                    offset,
                );
                self.width = f_width;
                self.height = f_height;
                self.state = FrameDecoderState::RgbParts {
                    bytes_per_channel,
                    has_red: channel == sys::Frame::Red,
                    has_green: channel == sys::Frame::Green,
                    has_blue: channel == sys::Frame::Blue,
                };
                Ok(())
            }
            // rgb parts after first
            (
                FrameDecoderState::RgbParts {
                    bytes_per_channel,
                    has_red: has_chan,
                    has_green: has_other1,
                    has_blue: has_other2,
                },
                channel @ sys::Frame::Red,
            )
            | (
                FrameDecoderState::RgbParts {
                    bytes_per_channel,
                    has_red: has_other1,
                    has_green: has_chan,
                    has_blue: has_other2,
                },
                channel @ sys::Frame::Green,
            )
            | (
                FrameDecoderState::RgbParts {
                    bytes_per_channel,
                    has_red: has_other1,
                    has_green: has_other2,
                    has_blue: has_chan,
                },
                channel @ sys::Frame::Blue,
            ) => {
                if *has_chan {
                    return Err(FrameDecodeError::DuplicateChannel);
                }
                if f_width != self.width || f_height != self.height {
                    return Err(FrameDecodeError::UnexpectedParameters);
                }
                if params.depth & 0b111 != 0 || params.depth / 8 != *bytes_per_channel {
                    // only supports whole byte channels
                    return Err(FrameDecodeError::UnexpectedParameters);
                }

                let bytes_per_pixel = *bytes_per_channel as usize * 3;
                let offset = bytes_per_pixel
                    * match channel {
                        sys::Frame::Red => 0,
                        sys::Frame::Green => 1,
                        sys::Frame::Blue => 2,
                        _ => unreachable!(),
                    };
                let dst_len = f_width as usize * f_height as usize * bytes_per_pixel;
                Self::write_channel(
                    &mut self.buffer.spare_capacity_mut()[..dst_len],
                    frame,
                    params.bytes_per_line as usize,
                    f_width as usize,
                    *bytes_per_channel as usize,
                    offset,
                );
                if *has_other1 && *has_other2 {
                    // SAFETY: All pixel channels were fully initialized
                    unsafe { self.buffer.set_len(self.buffer.len() + dst_len) };
                    self.state = FrameDecoderState::Done(DecodedImageFormat::Rgb {
                        bytes_per_channel: *bytes_per_channel,
                    })
                } else {
                    *has_chan = true;
                }
                Ok(())
            }
            // other unknown frame format
            _ => Err(FrameDecodeError::UnsupportedParameters),
        }
    }

    fn write_channel(
        dst: &mut [MaybeUninit<u8>],
        frame: &[u8],
        bytes_per_line: usize,
        width: usize,
        bytes_per_channel: usize,
        offset: usize,
    ) {
        let channels = frame
            .chunks_exact(bytes_per_line)
            .flat_map(|line| line[..width * bytes_per_channel].chunks_exact(bytes_per_channel));
        let dst_channels = dst
            .chunks_exact_mut(3 * bytes_per_channel)
            .map(|pixel| &mut pixel[offset..offset + bytes_per_channel]);
        for (dst, src) in dst_channels.zip(channels) {
            dst.copy_from_slice(slice_as_maybe_uninit(src));
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FrameDecoderState {
    Initial,
    Done(DecodedImageFormat),
    RgbParts {
        bytes_per_channel: u32,
        has_red: bool,
        has_green: bool,
        has_blue: bool,
    },
}

impl FrameDecoderState {
    const fn is_done(&self) -> bool {
        matches!(self, Self::Done(_))
    }
}

#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub data: Vec<u8>,
    pub format: DecodedImageFormat,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedImageFormat {
    /// Black and white images are represented as a packed big-endian bitmap unless
    /// [`FrameDecoder::decode_black_and_white_as_bytes`] was set to `true`, in which case
    /// every pixel is a byte with value `0` or `1`.
    BlackAndWhite,
    /// Gray pixel data with the given amount of bytes per pixel.
    Gray { bytes_per_pixel: u32 },
    /// RGB pixel data wit the given amount of bytes per color channel.
    Rgb { bytes_per_channel: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDecodeError {
    AlreadyDone,
    DuplicateChannel,
    UnexpectedParameters,
    UnsupportedParameters,
    InvalidParameters,
}

impl fmt::Display for FrameDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::AlreadyDone => "already received all frames",
            Self::DuplicateChannel => "channel was already received",
            Self::UnexpectedParameters => "parameters of this frame mismatch the predecessor",
            Self::UnsupportedParameters => "frame parameters are not supported by this decoder",
            Self::InvalidParameters => "frame parameters are invalid",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for FrameDecodeError {}
