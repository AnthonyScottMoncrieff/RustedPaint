use crate::{BlendMode, Document, DocumentError, Layer, LayerId, Rgba8};
use std::fmt;
use std::io::Read;

pub(crate) fn read(mut input: impl Read) -> Result<Document, PdnImportError> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(PdnImportError::Io)?;
    let options = img_core::ImgDec {
        flags: img_core::IMGD_STRAIGHT,
        maxpx: 32 * 1024 * 1024,
        ..Default::default()
    };
    let document = img_pdn::decode(&bytes, Some(&options)).map_err(PdnImportError::Decode)?;
    let width = document.w;
    let height = document.h;
    let layers = document
        .layers
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            let blend_mode = blend_mode(source.blend)?;
            let name =
                String::from_utf8(source.name).map_err(|_| PdnImportError::InvalidLayer(index))?;
            let stride = usize::try_from(source.sf.stride)
                .map_err(|_| PdnImportError::InvalidLayer(index))?;
            let row_bytes = width as usize * 4;
            if stride < row_bytes || source.sf.px.len() != stride * height as usize {
                return Err(PdnImportError::InvalidLayer(index));
            }
            let mut pixels = Vec::with_capacity(width as usize * height as usize);
            for row in source.sf.px.chunks_exact(stride).take(height as usize) {
                for pixel in row[..row_bytes].as_chunks::<4>().0 {
                    pixels.push(Rgba8::new(pixel[2], pixel[1], pixel[0], pixel[3]));
                }
            }
            Ok(Layer {
                id: LayerId(index as u64 + 1),
                name,
                visible: source.visible != 0,
                opacity: source.opacity,
                blend_mode,
                pixels,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Document::imported(width, height, layers).map_err(PdnImportError::Document)
}

fn blend_mode(mode: u8) -> Result<BlendMode, PdnImportError> {
    Ok(match mode {
        0 => BlendMode::Normal,
        1 => BlendMode::Multiply,
        2 => BlendMode::Additive,
        3 => BlendMode::ColorBurn,
        4 => BlendMode::ColorDodge,
        5 => BlendMode::Reflect,
        6 => BlendMode::Glow,
        7 => BlendMode::Overlay,
        8 => BlendMode::Difference,
        9 => BlendMode::Negation,
        10 => BlendMode::Lighten,
        11 => BlendMode::Darken,
        12 => BlendMode::Screen,
        13 => BlendMode::Xor,
        _ => return Err(PdnImportError::UnsupportedBlend(mode)),
    })
}

#[derive(Debug)]
pub enum PdnImportError {
    Io(std::io::Error),
    Decode(i32),
    Document(DocumentError),
    InvalidLayer(usize),
    UnsupportedBlend(u8),
}
impl fmt::Display for PdnImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Decode(code) => write!(f, "cannot decode PDN (codec error {code})"),
            Self::Document(e) => e.fmt(f),
            Self::InvalidLayer(index) => write!(f, "invalid PDN layer {index}"),
            Self::UnsupportedBlend(mode) => write!(f, "PDN blend mode {mode} is not yet supported"),
        }
    }
}
impl std::error::Error for PdnImportError {}
