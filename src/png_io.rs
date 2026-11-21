use crate::{Document, DocumentError, Rgba8};
use std::fmt;
use std::io::{BufRead, Seek, Write};

pub(crate) fn write(document: &Document, output: impl Write) -> Result<(), png::EncodingError> {
    let mut encoder = png::Encoder::new(output, document.width, document.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    let pixels: Vec<u8> = document
        .composite()
        .into_iter()
        .flat_map(|pixel| [pixel.red, pixel.green, pixel.blue, pixel.alpha])
        .collect();
    writer.write_image_data(&pixels)
}

pub(crate) fn read(input: impl BufRead + Seek) -> Result<Document, PngImportError> {
    let mut decoder = png::Decoder::new(input);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let output_size = reader
        .output_buffer_size()
        .ok_or(PngImportError::MissingOutputBuffer)?;
    let mut buffer = vec![0; output_size];
    let info = reader.next_frame(&mut buffer)?;
    let samples = &buffer[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgba => samples
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| Rgba8::new(pixel[0], pixel[1], pixel[2], pixel[3]))
            .collect(),
        png::ColorType::Rgb => samples
            .as_chunks::<3>()
            .0
            .iter()
            .map(|pixel| Rgba8::new(pixel[0], pixel[1], pixel[2], u8::MAX))
            .collect(),
        png::ColorType::Grayscale => samples
            .iter()
            .map(|&value| Rgba8::new(value, value, value, u8::MAX))
            .collect(),
        png::ColorType::GrayscaleAlpha => samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pixel| Rgba8::new(pixel[0], pixel[0], pixel[0], pixel[1]))
            .collect(),
        color_type => return Err(PngImportError::UnsupportedColorType(color_type)),
    };
    Document::from_rgba8(info.width, info.height, pixels).map_err(PngImportError::Document)
}

#[derive(Debug)]
pub enum PngImportError {
    Decode(png::DecodingError),
    Document(DocumentError),
    MissingOutputBuffer,
    UnsupportedColorType(png::ColorType),
}

impl From<png::DecodingError> for PngImportError {
    fn from(error: png::DecodingError) -> Self {
        Self::Decode(error)
    }
}

impl fmt::Display for PngImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(f, "cannot decode PNG: {error}"),
            Self::Document(error) => error.fmt(f),
            Self::MissingOutputBuffer => f.write_str("PNG output size exceeds addressable memory"),
            Self::UnsupportedColorType(color_type) => {
                write!(f, "unsupported PNG colour type: {color_type:?}")
            }
        }
    }
}

impl std::error::Error for PngImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            Self::Document(error) => Some(error),
            Self::MissingOutputBuffer | Self::UnsupportedColorType(_) => None,
        }
    }
}
