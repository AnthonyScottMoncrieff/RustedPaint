use crate::{Document, DocumentError, LayerId, Rgba8};
use std::collections::HashSet;
use std::fmt;

/// Reversible, document-local history for committed pixel edits.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PixelEdit {
    layer: LayerId,
    x: u32,
    y: u32,
    before: Rgba8,
    after: Rgba8,
}

#[derive(Debug)]
enum HistoryEntry {
    Pixel(PixelEdit),
    Pixels(Vec<PixelEdit>),
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Commits a pixel edit and invalidates any redo history. Setting a pixel
    /// to its current colour is a no-op and does not create a history entry.
    pub fn set_pixel(
        &mut self,
        document: &mut Document,
        layer: LayerId,
        x: u32,
        y: u32,
        after: Rgba8,
    ) -> Result<bool, HistoryError> {
        let before = document.pixel(layer, x, y)?;
        if before == after {
            return Ok(false);
        }
        document.set_pixel(layer, x, y, after)?;
        self.undo.push(HistoryEntry::Pixel(PixelEdit {
            layer,
            x,
            y,
            before,
            after,
        }));
        self.redo.clear();
        Ok(true)
    }

    /// Applies a group of changes as one undoable history entry. A coordinate
    /// may appear at most once, which keeps undo and redo atomic.
    pub fn set_pixels(
        &mut self,
        document: &mut Document,
        layer: LayerId,
        changes: &[(u32, u32, Rgba8)],
    ) -> Result<bool, HistoryError> {
        let mut seen = HashSet::with_capacity(changes.len());
        let mut edits = Vec::with_capacity(changes.len());
        for &(x, y, after) in changes {
            if !seen.insert((x, y)) {
                return Err(HistoryError::DuplicatePixel { x, y });
            }
            let before = document.pixel(layer, x, y)?;
            if before != after {
                edits.push(PixelEdit {
                    layer,
                    x,
                    y,
                    before,
                    after,
                });
            }
        }
        if edits.is_empty() {
            return Ok(false);
        }
        for edit in &edits {
            document.set_pixel(edit.layer, edit.x, edit.y, edit.after)?;
        }
        self.undo.push(HistoryEntry::Pixels(edits));
        self.redo.clear();
        Ok(true)
    }

    pub fn undo(&mut self, document: &mut Document) -> Result<bool, HistoryError> {
        let Some(entry) = self.undo.pop() else {
            return Ok(false);
        };
        if let Err(error) = entry.undo(document) {
            self.undo.push(entry);
            return Err(error);
        }
        self.redo.push(entry);
        Ok(true)
    }

    pub fn redo(&mut self, document: &mut Document) -> Result<bool, HistoryError> {
        let Some(entry) = self.redo.pop() else {
            return Ok(false);
        };
        if let Err(error) = entry.redo(document) {
            self.redo.push(entry);
            return Err(error);
        }
        self.undo.push(entry);
        Ok(true)
    }
}

impl HistoryEntry {
    fn undo(&self, document: &mut Document) -> Result<(), HistoryError> {
        let edits = self.edits();
        validate_expected(document, edits.iter().rev(), |edit| edit.after)?;
        for edit in edits.iter().rev() {
            document.set_pixel(edit.layer, edit.x, edit.y, edit.before)?;
        }
        Ok(())
    }

    fn redo(&self, document: &mut Document) -> Result<(), HistoryError> {
        let edits = self.edits();
        validate_expected(document, edits.iter(), |edit| edit.before)?;
        for edit in edits {
            document.set_pixel(edit.layer, edit.x, edit.y, edit.after)?;
        }
        Ok(())
    }

    fn edits(&self) -> &[PixelEdit] {
        match self {
            Self::Pixel(edit) => std::slice::from_ref(edit),
            Self::Pixels(edits) => edits,
        }
    }
}

fn validate_expected<'a>(
    document: &Document,
    edits: impl Iterator<Item = &'a PixelEdit>,
    expected: impl Fn(&PixelEdit) -> Rgba8,
) -> Result<(), HistoryError> {
    for edit in edits {
        if document.pixel(edit.layer, edit.x, edit.y)? != expected(edit) {
            return Err(HistoryError::Diverged {
                layer: edit.layer,
                x: edit.x,
                y: edit.y,
            });
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum HistoryError {
    Document(DocumentError),
    Diverged { layer: LayerId, x: u32, y: u32 },
    DuplicatePixel { x: u32, y: u32 },
}

impl From<DocumentError> for HistoryError {
    fn from(error: DocumentError) -> Self {
        Self::Document(error)
    }
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(error) => error.fmt(f),
            Self::Diverged { layer, x, y } => write!(
                f,
                "cannot apply history at pixel ({x}, {y}) in layer {layer:?}: document changed"
            ),
            Self::DuplicatePixel { x, y } => {
                write!(
                    f,
                    "pixel ({x}, {y}) appears more than once in one history entry"
                )
            }
        }
    }
}

impl std::error::Error for HistoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Document(error) => Some(error),
            Self::Diverged { .. } | Self::DuplicatePixel { .. } => None,
        }
    }
}
