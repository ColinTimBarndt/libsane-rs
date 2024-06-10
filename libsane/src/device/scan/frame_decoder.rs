use core::fmt;
use std::mem::MaybeUninit;

use super::FrameParameters;
use crate::{slice_util::slice_as_maybe_uninit, sys};

#[derive(Debug, Clone)]
pub struct Builder {
    buffer: Vec<u8>,
    black_and_white_as_bytes: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            black_and_white_as_bytes: false,
        }
    }

    pub fn build(self) -> FrameDecoder {
        FrameDecoder {
            buffer: self.buffer,
            state: FrameDecoderState::Initial,
            width: 0,
            height: 0,
            black_and_white_as_bytes: self.black_and_white_as_bytes,
        }
    }

    /// By default, black and white images are represented as a packed big-endian bitmap.
    /// Setting this to true will store every pixel as a byte with value `0` or `1`.
    pub fn decode_black_and_white_as_bytes(self, do_id: bool) -> Self {
        Self {
            black_and_white_as_bytes: do_id,
            ..self
        }
    }

    pub fn with_buffer(self, buffer: Vec<u8>) -> Self {
        Self { buffer, ..self }
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
    pub const fn builder() -> Builder {
        Builder::new()
    }

    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            state: FrameDecoderState::Initial,
            width: 0,
            height: 0,
            black_and_white_as_bytes: false,
        }
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
