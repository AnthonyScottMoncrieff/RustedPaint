#![forbid(unsafe_code)]

//! The UI-independent document core for RustedPaint.

#[cfg(feature = "desktop")]
pub mod desktop;
mod editor;
mod history;
mod pdn_io;
mod png_io;
mod selection;
mod tools;

use std::fmt;

pub use editor::Editor;
pub use history::{History, HistoryError};
pub use pdn_io::PdnImportError;
pub use png_io::PngImportError;
pub use selection::{Rect, Selection, SelectionCombine, SelectionError};
pub use tools::{BucketError, PaintBucketTool, PencilError, PencilTool};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Rgba8 {
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);

    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Additive,
    ColorBurn,
    ColorDodge,
    Reflect,
    Glow,
    Overlay,
    Difference,
    Negation,
    Lighten,
    Darken,
    Screen,
    Xor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    id: LayerId,
    name: String,
    visible: bool,
    opacity: u8,
    blend_mode: BlendMode,
    pixels: Vec<Rgba8>,
}

impl Layer {
    pub fn id(&self) -> LayerId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn opacity(&self) -> u8 {
        self.opacity
    }

    pub fn set_opacity(&mut self, opacity: u8) {
        self.opacity = opacity;
    }

    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    next_layer_id: u64,
    revision: u64,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Result<Self, DocumentError> {
        if width == 0 || height == 0 {
            return Err(DocumentError::EmptyDimensions);
        }
        let pixel_count = pixel_count(width, height)?;
        Ok(Self {
            width,
            height,
            layers: vec![Layer {
                id: LayerId(1),
                name: "Background".into(),
                visible: true,
                opacity: u8::MAX,
                blend_mode: BlendMode::Normal,
                pixels: vec![Rgba8::TRANSPARENT; pixel_count],
            }],
            next_layer_id: 2,
            revision: 0,
        })
    }

    /// Creates a flat document with one Background layer from straight RGBA8
    /// pixels in row-major order.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<Rgba8>) -> Result<Self, DocumentError> {
        let mut document = Self::new(width, height)?;
        let expected = document.pixel_count();
        if pixels.len() != expected {
            return Err(DocumentError::PixelCount {
                expected,
                actual: pixels.len(),
            });
        }
        document.layers[0].pixels = pixels;
        Ok(document)
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Decodes Normal-blend PDN layers into this document model.
    pub fn read_pdn(input: impl std::io::Read) -> Result<Self, PdnImportError> {
        pdn_io::read(input)
    }

    pub fn add_layer(&mut self, name: impl Into<String>) -> LayerId {
        let id = LayerId(self.next_layer_id);
        self.next_layer_id += 1;
        self.layers.push(Layer {
            id,
            name: name.into(),
            visible: true,
            opacity: u8::MAX,
            blend_mode: BlendMode::Normal,
            pixels: vec![Rgba8::TRANSPARENT; self.pixel_count()],
        });
        self.revision += 1;
        id
    }

    pub fn rename_layer(
        &mut self,
        layer: LayerId,
        name: impl Into<String>,
    ) -> Result<(), DocumentError> {
        let name = name.into();
        let target = self.layer_mut(layer)?;
        if target.name != name {
            target.name = name;
            self.revision += 1;
        }
        Ok(())
    }

    /// Moves a layer to `destination`, where 0 is the bottom layer.
    pub fn move_layer(&mut self, layer: LayerId, destination: usize) -> Result<(), DocumentError> {
        if destination >= self.layers.len() {
            return Err(DocumentError::InvalidLayerPosition);
        }
        let source = self
            .layers
            .iter()
            .position(|candidate| candidate.id == layer)
            .ok_or(DocumentError::UnknownLayer(layer))?;
        if source != destination {
            let layer = self.layers.remove(source);
            self.layers.insert(destination, layer);
            self.revision += 1;
        }
        Ok(())
    }

    pub fn duplicate_layer(&mut self, layer: LayerId) -> Result<LayerId, DocumentError> {
        let source = self
            .layers
            .iter()
            .position(|candidate| candidate.id == layer)
            .ok_or(DocumentError::UnknownLayer(layer))?;
        let id = LayerId(self.next_layer_id);
        self.next_layer_id += 1;
        let mut duplicate = self.layers[source].clone();
        duplicate.id = id;
        duplicate.name.push_str(" copy");
        self.layers.insert(source + 1, duplicate);
        self.revision += 1;
        Ok(id)
    }

    pub fn delete_layer(&mut self, layer: LayerId) -> Result<(), DocumentError> {
        if self.layers.len() == 1 {
            return Err(DocumentError::LastLayer);
        }
        let index = self
            .layers
            .iter()
            .position(|candidate| candidate.id == layer)
            .ok_or(DocumentError::UnknownLayer(layer))?;
        self.layers.remove(index);
        self.revision += 1;
        Ok(())
    }

    /// Flattens `layer` into the visible layer immediately below it.
    pub fn merge_layer_down(&mut self, layer: LayerId) -> Result<LayerId, DocumentError> {
        let index = self
            .layers
            .iter()
            .position(|candidate| candidate.id == layer)
            .ok_or(DocumentError::UnknownLayer(layer))?;
        if index == 0 {
            return Err(DocumentError::BottomLayer);
        }
        let upper = self.layers.remove(index);
        let lower = &mut self.layers[index - 1];
        if upper.visible {
            for (backdrop, source) in lower.pixels.iter_mut().zip(upper.pixels) {
                *backdrop = source_over(*backdrop, source, upper.opacity, upper.blend_mode);
            }
        }
        self.revision += 1;
        Ok(lower.id)
    }

    /// Crops every layer to a rectangle in document coordinates.
    pub fn crop(&mut self, rect: Rect) -> Result<(), DocumentError> {
        if rect.x() >= self.width
            || rect.y() >= self.height
            || rect.width() > self.width - rect.x()
            || rect.height() > self.height - rect.y()
        {
            return Err(DocumentError::CropOutOfBounds);
        }
        if rect.width() == self.width
            && rect.height() == self.height
            && rect.x() == 0
            && rect.y() == 0
        {
            return Ok(());
        }
        let old_width = self.width as usize;
        for layer in &mut self.layers {
            let mut pixels = Vec::with_capacity(rect.width() as usize * rect.height() as usize);
            for y in rect.y() as usize..(rect.y() + rect.height()) as usize {
                let start = y * old_width + rect.x() as usize;
                pixels.extend_from_slice(&layer.pixels[start..start + rect.width() as usize]);
            }
            layer.pixels = pixels;
        }
        self.width = rect.width();
        self.height = rect.height();
        self.revision += 1;
        Ok(())
    }

    /// Changes whether a layer contributes to the document composite.
    pub fn set_layer_visible(
        &mut self,
        layer: LayerId,
        visible: bool,
    ) -> Result<(), DocumentError> {
        let target = self.layer_mut(layer)?;
        if target.visible != visible {
            target.visible = visible;
            self.revision += 1;
        }
        Ok(())
    }

    /// Sets the layer opacity, where 0 is transparent and 255 is opaque.
    pub fn set_layer_opacity(&mut self, layer: LayerId, opacity: u8) -> Result<(), DocumentError> {
        let target = self.layer_mut(layer)?;
        if target.opacity != opacity {
            target.opacity = opacity;
            self.revision += 1;
        }
        Ok(())
    }

    pub fn set_layer_blend_mode(
        &mut self,
        layer: LayerId,
        blend_mode: BlendMode,
    ) -> Result<(), DocumentError> {
        let target = self.layer_mut(layer)?;
        if target.blend_mode != blend_mode {
            target.blend_mode = blend_mode;
            self.revision += 1;
        }
        Ok(())
    }

    pub fn set_pixel(
        &mut self,
        layer: LayerId,
        x: u32,
        y: u32,
        color: Rgba8,
    ) -> Result<(), DocumentError> {
        let index = self.index(x, y)?;
        let target = self
            .layers
            .iter_mut()
            .find(|candidate| candidate.id == layer)
            .ok_or(DocumentError::UnknownLayer(layer))?;
        if target.pixels[index] != color {
            target.pixels[index] = color;
            self.revision += 1;
        }
        Ok(())
    }

    pub fn pixel(&self, layer: LayerId, x: u32, y: u32) -> Result<Rgba8, DocumentError> {
        let index = self.index(x, y)?;
        self.layers
            .iter()
            .find(|candidate| candidate.id == layer)
            .map(|target| target.pixels[index])
            .ok_or(DocumentError::UnknownLayer(layer))
    }

    /// Renders visible layers from bottom to top using Normal source-over
    /// compositing. The returned pixels are straight RGBA8.
    pub fn composite(&self) -> Vec<Rgba8> {
        let mut output = vec![Rgba8::TRANSPARENT; self.pixel_count()];
        for layer in self.layers.iter().filter(|layer| layer.visible) {
            for (backdrop, source) in output.iter_mut().zip(&layer.pixels) {
                *backdrop = source_over(*backdrop, *source, layer.opacity, layer.blend_mode);
            }
        }
        output
    }

    /// Writes the current flattened Normal composite as an RGBA8 PNG.
    pub fn write_png(&self, output: impl std::io::Write) -> Result<(), png::EncodingError> {
        png_io::write(self, output)
    }

    /// Decodes one flat PNG image as a single Background layer.
    pub fn read_png(input: impl std::io::BufRead + std::io::Seek) -> Result<Self, PngImportError> {
        png_io::read(input)
    }

    fn index(&self, x: u32, y: u32) -> Result<usize, DocumentError> {
        if x >= self.width || y >= self.height {
            return Err(DocumentError::OutOfBounds { x, y });
        }
        Ok(y as usize * self.width as usize + x as usize)
    }

    fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    fn layer_mut(&mut self, layer: LayerId) -> Result<&mut Layer, DocumentError> {
        self.layers
            .iter_mut()
            .find(|candidate| candidate.id == layer)
            .ok_or(DocumentError::UnknownLayer(layer))
    }

    pub(crate) fn imported(
        width: u32,
        height: u32,
        layers: Vec<Layer>,
    ) -> Result<Self, DocumentError> {
        if layers.is_empty() {
            return Err(DocumentError::EmptyDimensions);
        }
        let expected = pixel_count(width, height)?;
        if layers.iter().any(|layer| layer.pixels.len() != expected) {
            return Err(DocumentError::PixelCount {
                expected,
                actual: 0,
            });
        }
        Ok(Self {
            width,
            height,
            next_layer_id: layers.len() as u64 + 1,
            layers,
            revision: 0,
        })
    }
}

fn source_over(backdrop: Rgba8, source: Rgba8, opacity: u8, mode: BlendMode) -> Rgba8 {
    let source_alpha = div_round(u32::from(source.alpha) * u32::from(opacity), 255);
    if source_alpha == 0 {
        return backdrop;
    }
    let backdrop_alpha = u32::from(backdrop.alpha);
    if backdrop_alpha == 0 {
        return Rgba8::new(source.red, source.green, source.blue, source_alpha as u8);
    }

    let inverse_source_alpha = 255 - source_alpha;
    let output_alpha = source_alpha + div_round(backdrop_alpha * inverse_source_alpha, 255);
    let channel = |source: u8, backdrop: u8| {
        let blended = blend_channel(mode, backdrop, source);
        let numerator = u32::from(blended) * source_alpha * 255
            + u32::from(backdrop) * backdrop_alpha * inverse_source_alpha;
        div_round(numerator, output_alpha * 255) as u8
    };
    Rgba8::new(
        channel(source.red, backdrop.red),
        channel(source.green, backdrop.green),
        channel(source.blue, backdrop.blue),
        output_alpha as u8,
    )
}

fn blend_channel(mode: BlendMode, backdrop: u8, source: u8) -> u8 {
    let b = u32::from(backdrop);
    let s = u32::from(source);
    match mode {
        BlendMode::Normal => source,
        BlendMode::Multiply => div_round(b * s, 255) as u8,
        BlendMode::Additive => backdrop.saturating_add(source),
        BlendMode::ColorBurn if source == 0 => 0,
        BlendMode::ColorBurn => (255 - div_round((255 - b) * 255, s).min(255)) as u8,
        BlendMode::ColorDodge if source == u8::MAX => u8::MAX,
        BlendMode::ColorDodge => div_round(b * 255, 255 - s).min(255) as u8,
        BlendMode::Reflect if source == u8::MAX => u8::MAX,
        BlendMode::Reflect => div_round(b * b, 255 - s).min(255) as u8,
        BlendMode::Glow if backdrop == u8::MAX => u8::MAX,
        BlendMode::Glow => div_round(s * s, 255 - b).min(255) as u8,
        BlendMode::Overlay if backdrop < 128 => div_round(2 * b * s, 255) as u8,
        BlendMode::Overlay => (255 - div_round(2 * (255 - b) * (255 - s), 255)) as u8,
        BlendMode::Difference => backdrop.abs_diff(source),
        BlendMode::Negation => 255 - (255 - b as i32 - s as i32).unsigned_abs() as u8,
        BlendMode::Lighten => backdrop.max(source),
        BlendMode::Darken => backdrop.min(source),
        BlendMode::Screen => (255 - div_round((255 - b) * (255 - s), 255)) as u8,
        BlendMode::Xor => backdrop ^ source,
    }
}

fn div_round(numerator: u32, denominator: u32) -> u32 {
    (numerator + denominator / 2) / denominator
}

fn pixel_count(width: u32, height: u32) -> Result<usize, DocumentError> {
    usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| DocumentError::TooLarge)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentError {
    EmptyDimensions,
    TooLarge,
    OutOfBounds { x: u32, y: u32 },
    UnknownLayer(LayerId),
    PixelCount { expected: usize, actual: usize },
    InvalidLayerPosition,
    LastLayer,
    BottomLayer,
    CropOutOfBounds,
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDimensions => f.write_str("document dimensions must be non-zero"),
            Self::TooLarge => f.write_str("document dimensions exceed addressable memory"),
            Self::OutOfBounds { x, y } => write!(f, "pixel ({x}, {y}) is outside the document"),
            Self::UnknownLayer(id) => write!(f, "unknown layer {}", id.0),
            Self::PixelCount { expected, actual } => {
                write!(f, "expected {expected} pixels but received {actual}")
            }
            Self::InvalidLayerPosition => f.write_str("layer position is outside the stack"),
            Self::LastLayer => f.write_str("a document must retain one layer"),
            Self::BottomLayer => f.write_str("the bottom layer cannot be merged down"),
            Self::CropOutOfBounds => f.write_str("crop rectangle is outside the document"),
        }
    }
}

impl std::error::Error for DocumentError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_has_one_transparent_background_layer() {
        let document = Document::new(2, 3).unwrap();
        assert_eq!(document.layers().len(), 1);
        assert_eq!(document.layers()[0].name(), "Background");
        assert_eq!(
            document.pixel(document.layers()[0].id(), 1, 2),
            Ok(Rgba8::TRANSPARENT)
        );
    }

    #[test]
    fn modifying_a_pixel_advances_the_revision_once() {
        let mut document = Document::new(1, 1).unwrap();
        let background = document.layers()[0].id();
        document
            .set_pixel(background, 0, 0, Rgba8::new(30, 60, 90, 128))
            .unwrap();
        assert_eq!(document.revision(), 1);
        document
            .set_pixel(background, 0, 0, Rgba8::new(30, 60, 90, 128))
            .unwrap();
        assert_eq!(document.revision(), 1);
    }

    #[test]
    fn layer_ids_remain_stable_when_more_layers_are_added() {
        let mut document = Document::new(1, 1).unwrap();
        let background = document.layers()[0].id();
        let paint = document.add_layer("Paint");
        assert_ne!(background, paint);
        assert_eq!(document.layers()[1].name(), "Paint");
    }

    #[test]
    fn composite_respects_layer_order_and_partial_alpha() {
        let mut document = Document::new(1, 1).unwrap();
        let background = document.layers()[0].id();
        let paint = document.add_layer("Paint");
        document
            .set_pixel(background, 0, 0, Rgba8::new(255, 0, 0, 255))
            .unwrap();
        document
            .set_pixel(paint, 0, 0, Rgba8::new(0, 0, 255, 128))
            .unwrap();

        assert_eq!(document.composite(), vec![Rgba8::new(127, 0, 128, 255)]);
    }

    #[test]
    fn composite_applies_opacity_and_skips_hidden_layers() {
        let mut document = Document::new(1, 1).unwrap();
        let background = document.layers()[0].id();
        let paint = document.add_layer("Paint");
        document
            .set_pixel(background, 0, 0, Rgba8::new(255, 0, 0, 255))
            .unwrap();
        document
            .set_pixel(paint, 0, 0, Rgba8::new(0, 255, 0, 255))
            .unwrap();
        document.set_layer_opacity(paint, 128).unwrap();
        assert_eq!(document.composite(), vec![Rgba8::new(127, 128, 0, 255)]);

        document.set_layer_visible(paint, false).unwrap();
        assert_eq!(document.composite(), vec![Rgba8::new(255, 0, 0, 255)]);
    }

    #[test]
    fn additive_blend_clamps_each_channel_before_source_over() {
        let mut document = Document::new(1, 1).unwrap();
        let bottom = document.layers()[0].id();
        let top = document.add_layer("Additive");
        document
            .set_pixel(bottom, 0, 0, Rgba8::new(180, 90, 40, 255))
            .unwrap();
        document
            .set_pixel(top, 0, 0, Rgba8::new(100, 200, 20, 255))
            .unwrap();
        document
            .set_layer_blend_mode(top, BlendMode::Additive)
            .unwrap();
        assert_eq!(document.composite(), vec![Rgba8::new(255, 255, 60, 255)]);
    }

    #[test]
    fn layer_stack_commands_preserve_ids_and_layer_order() {
        let mut document = Document::new(1, 1).unwrap();
        let background = document.layers()[0].id();
        let paint = document.add_layer("Paint");
        document.rename_layer(paint, "Ink").unwrap();
        let copy = document.duplicate_layer(paint).unwrap();
        document.move_layer(copy, 0).unwrap();
        assert_eq!(document.layers()[0].id(), copy);
        assert_eq!(document.layers()[1].id(), background);
        assert_eq!(document.layers()[2].name(), "Ink");
        document.delete_layer(copy).unwrap();
        assert_eq!(document.layers().len(), 2);
    }

    #[test]
    fn merge_down_uses_the_upper_layer_blend_and_removes_it() {
        let mut document = Document::new(1, 1).unwrap();
        let bottom = document.layers()[0].id();
        let top = document.add_layer("Top");
        document
            .set_pixel(bottom, 0, 0, Rgba8::new(100, 0, 0, 255))
            .unwrap();
        document
            .set_pixel(top, 0, 0, Rgba8::new(0, 50, 0, 255))
            .unwrap();
        document
            .set_layer_blend_mode(top, BlendMode::Additive)
            .unwrap();
        assert_eq!(document.merge_layer_down(top), Ok(bottom));
        assert_eq!(document.layers().len(), 1);
        assert_eq!(
            document.pixel(bottom, 0, 0),
            Ok(Rgba8::new(100, 50, 0, 255))
        );
        assert_eq!(
            document.merge_layer_down(bottom),
            Err(DocumentError::BottomLayer)
        );
    }

    #[test]
    fn transparent_source_does_not_replace_backdrop_rgb() {
        let mut document = Document::new(1, 1).unwrap();
        let background = document.layers()[0].id();
        let paint = document.add_layer("Paint");
        document
            .set_pixel(background, 0, 0, Rgba8::new(20, 40, 60, 255))
            .unwrap();
        document
            .set_pixel(paint, 0, 0, Rgba8::new(255, 255, 255, 0))
            .unwrap();

        assert_eq!(document.composite(), vec![Rgba8::new(20, 40, 60, 255)]);
        assert_eq!(
            document.pixel(paint, 0, 0),
            Ok(Rgba8::new(255, 255, 255, 0))
        );
    }

    #[test]
    fn pixel_history_undoes_and_redoes_a_committed_edit() {
        let mut document = Document::new(1, 1).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();

        assert!(
            history
                .set_pixel(&mut document, layer, 0, 0, Rgba8::new(5, 10, 15, 20))
                .unwrap()
        );
        assert!(history.can_undo());
        assert!(!history.can_redo());
        assert!(history.undo(&mut document).unwrap());
        assert_eq!(document.pixel(layer, 0, 0), Ok(Rgba8::TRANSPARENT));
        assert!(history.redo(&mut document).unwrap());
        assert_eq!(document.pixel(layer, 0, 0), Ok(Rgba8::new(5, 10, 15, 20)));
    }

    #[test]
    fn new_history_edit_invalidates_redo() {
        let mut document = Document::new(1, 1).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();
        history
            .set_pixel(&mut document, layer, 0, 0, Rgba8::new(1, 2, 3, 4))
            .unwrap();
        history.undo(&mut document).unwrap();
        history
            .set_pixel(&mut document, layer, 0, 0, Rgba8::new(9, 8, 7, 6))
            .unwrap();

        assert!(!history.can_redo());
        assert!(!history.redo(&mut document).unwrap());
    }

    #[test]
    fn history_rejects_an_edit_when_the_document_has_diverged() {
        let mut document = Document::new(1, 1).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();
        history
            .set_pixel(&mut document, layer, 0, 0, Rgba8::new(1, 2, 3, 4))
            .unwrap();
        document
            .set_pixel(layer, 0, 0, Rgba8::new(9, 8, 7, 6))
            .unwrap();

        assert!(matches!(
            history.undo(&mut document),
            Err(HistoryError::Diverged { .. })
        ));
        assert!(history.can_undo());
        assert_eq!(document.pixel(layer, 0, 0), Ok(Rgba8::new(9, 8, 7, 6)));
    }

    #[test]
    fn grouped_pixel_edits_undo_and_redo_as_one_entry() {
        let mut document = Document::new(2, 1).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();

        assert!(
            history
                .set_pixels(
                    &mut document,
                    layer,
                    &[
                        (0, 0, Rgba8::new(10, 0, 0, 255)),
                        (1, 0, Rgba8::new(0, 20, 0, 255)),
                    ],
                )
                .unwrap()
        );
        assert!(history.undo(&mut document).unwrap());
        assert_eq!(document.composite(), vec![Rgba8::TRANSPARENT; 2]);
        assert!(!history.undo(&mut document).unwrap());
        assert!(history.redo(&mut document).unwrap());
        assert_eq!(
            document.composite(),
            vec![Rgba8::new(10, 0, 0, 255), Rgba8::new(0, 20, 0, 255)]
        );
    }

    #[test]
    fn grouped_edits_reject_duplicate_pixels_without_modifying_document() {
        let mut document = Document::new(1, 1).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();

        assert!(matches!(
            history.set_pixels(
                &mut document,
                layer,
                &[
                    (0, 0, Rgba8::new(1, 2, 3, 4)),
                    (0, 0, Rgba8::new(5, 6, 7, 8)),
                ],
            ),
            Err(HistoryError::DuplicatePixel { x: 0, y: 0 })
        ));
        assert_eq!(document.pixel(layer, 0, 0), Ok(Rgba8::TRANSPARENT));
        assert!(!history.can_undo());
    }

    #[test]
    fn pencil_draws_a_contiguous_diagonal_as_one_history_entry() {
        let mut document = Document::new(4, 4).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();
        let color = Rgba8::new(40, 50, 60, 255);

        assert!(
            PencilTool::draw_line(
                &mut document,
                &mut history,
                layer,
                (0, 0),
                (3, 3),
                color,
                &Selection::All,
            )
            .unwrap()
        );
        for coordinate in 0..4 {
            assert_eq!(document.pixel(layer, coordinate, coordinate), Ok(color));
        }
        assert!(history.undo(&mut document).unwrap());
        assert_eq!(document.composite(), vec![Rgba8::TRANSPARENT; 16]);
    }

    #[test]
    fn pencil_clips_to_the_active_selection_and_canvas() {
        let mut document = Document::new(4, 1).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();
        let selection = Selection::rectangle(Rect::new(1, 0, 2, 1).unwrap());
        let color = Rgba8::new(200, 100, 50, 128);

        PencilTool::draw_line(
            &mut document,
            &mut history,
            layer,
            (0, 0),
            (5, 0),
            color,
            &selection,
        )
        .unwrap();
        assert_eq!(
            document.composite(),
            vec![Rgba8::TRANSPARENT, color, color, Rgba8::TRANSPARENT]
        );
    }

    #[test]
    fn selection_combine_modes_produce_expected_binary_coverage() {
        let left = Selection::rectangle(Rect::new(0, 0, 2, 1).unwrap());
        let right = Selection::rectangle(Rect::new(1, 0, 2, 1).unwrap());

        let add = left.combine(&right, 3, 1, SelectionCombine::Add).unwrap();
        assert_eq!([0, 1, 2].map(|x| add.contains(x, 0)), [true, true, true]);
        let subtract = left
            .combine(&right, 3, 1, SelectionCombine::Subtract)
            .unwrap();
        assert_eq!(
            [0, 1, 2].map(|x| subtract.contains(x, 0)),
            [true, false, false]
        );
        let intersect = left
            .combine(&right, 3, 1, SelectionCombine::Intersect)
            .unwrap();
        assert_eq!(
            [0, 1, 2].map(|x| intersect.contains(x, 0)),
            [false, true, false]
        );
        let xor = left.combine(&right, 3, 1, SelectionCombine::Xor).unwrap();
        assert_eq!([0, 1, 2].map(|x| xor.contains(x, 0)), [true, false, true]);
    }

    #[test]
    fn png_export_has_document_dimensions_and_composite_pixels() {
        let mut document = Document::new(2, 1).unwrap();
        let layer = document.layers()[0].id();
        document
            .set_pixel(layer, 0, 0, Rgba8::new(1, 2, 3, 4))
            .unwrap();
        let mut encoded = Vec::new();
        document.write_png(&mut encoded).unwrap();

        let decoder = png::Decoder::new(std::io::Cursor::new(encoded));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&pixels[..info.buffer_size()], &[1, 2, 3, 4, 0, 0, 0, 0]);
    }

    #[test]
    fn paint_bucket_fills_the_connected_region_in_one_history_entry() {
        let mut document = Document::new(4, 2).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();
        let wall = Rgba8::new(9, 9, 9, 255);
        for y in 0..2 {
            document.set_pixel(layer, 2, y, wall).unwrap();
        }

        assert!(
            PaintBucketTool::fill(
                &mut document,
                &mut history,
                layer,
                (0, 0),
                Rgba8::new(30, 60, 90, 255),
                &Selection::All,
            )
            .unwrap()
        );
        assert_eq!(document.pixel(layer, 0, 1), Ok(Rgba8::new(30, 60, 90, 255)));
        assert_eq!(document.pixel(layer, 1, 0), Ok(Rgba8::new(30, 60, 90, 255)));
        assert_eq!(document.pixel(layer, 2, 0), Ok(wall));
        assert_eq!(document.pixel(layer, 3, 0), Ok(Rgba8::TRANSPARENT));
        history.undo(&mut document).unwrap();
        assert_eq!(document.pixel(layer, 0, 0), Ok(Rgba8::TRANSPARENT));
    }

    #[test]
    fn paint_bucket_does_not_cross_unselected_pixels() {
        let mut document = Document::new(3, 1).unwrap();
        let layer = document.layers()[0].id();
        let mut history = History::new();
        let selection = Selection::rectangle(Rect::new(0, 0, 1, 1).unwrap());

        PaintBucketTool::fill(
            &mut document,
            &mut history,
            layer,
            (0, 0),
            Rgba8::new(255, 0, 0, 255),
            &selection,
        )
        .unwrap();
        assert_eq!(
            document.composite(),
            vec![
                Rgba8::new(255, 0, 0, 255),
                Rgba8::TRANSPARENT,
                Rgba8::TRANSPARENT
            ]
        );
    }

    #[test]
    fn png_round_trip_imports_the_exported_flat_composite() {
        let mut document = Document::new(2, 1).unwrap();
        let layer = document.layers()[0].id();
        document
            .set_pixel(layer, 1, 0, Rgba8::new(70, 80, 90, 100))
            .unwrap();
        let mut encoded = Vec::new();
        document.write_png(&mut encoded).unwrap();

        let imported = Document::read_png(std::io::Cursor::new(encoded)).unwrap();
        assert_eq!((imported.width(), imported.height()), (2, 1));
        assert_eq!(imported.layers().len(), 1);
        assert_eq!(
            imported.composite(),
            vec![Rgba8::TRANSPARENT, Rgba8::new(70, 80, 90, 100)]
        );
    }
}
