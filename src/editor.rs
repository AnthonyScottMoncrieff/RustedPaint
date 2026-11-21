use crate::{BlendMode, Document, DocumentError, LayerId, Rect, Rgba8};
use std::collections::VecDeque;
#[derive(Clone)]
struct Snapshot {
    document: Document,
    selection: Option<Rect>,
    active_layer: LayerId,
}
#[derive(Clone)]
struct Clipboard {
    width: u32,
    height: u32,
    pixels: Vec<Rgba8>,
}
pub struct Editor {
    document: Document,
    selection: Option<Rect>,
    active_layer: LayerId,
    move_preview: Option<(Rect, (i32, i32), Vec<Rgba8>)>,
    brush_stroke: Option<(Snapshot, bool)>,
    clipboard: Option<Clipboard>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}
impl Editor {
    pub fn new(document: Document) -> Self {
        let active_layer = document.layers()[0].id();
        Self {
            document,
            selection: None,
            active_layer,
            move_preview: None,
            brush_stroke: None,
            clipboard: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn replace_document(&mut self, document: Document) {
        self.active_layer = document.layers()[0].id();
        self.document = document;
        self.selection = None;
        self.move_preview = None;
        self.brush_stroke = None;
        self.undo.clear();
        self.redo.clear();
    }
    pub fn selection(&self) -> Option<Rect> {
        self.selection
    }
    pub fn active_layer(&self) -> LayerId {
        self.active_layer
    }
    pub fn set_active_layer(&mut self, layer: LayerId) -> bool {
        if self.move_preview.is_some()
            || self.active_layer == layer
            || !self.document.layers().iter().any(|item| item.id() == layer)
        {
            return false;
        }
        self.active_layer = layer;
        true
    }
    pub fn select_rectangle(&mut self, first: (u32, u32), second: (u32, u32)) -> bool {
        self.set_selection(Rect::from_inclusive_points(
            first,
            second,
            (self.document.width(), self.document.height()),
        ))
    }
    pub fn fill_selection(&mut self, color: Rgba8) -> Result<bool, DocumentError> {
        if self.move_preview.is_some() {
            return Ok(false);
        }
        let rect = self
            .selection
            .unwrap_or(Rect::new(0, 0, self.document.width(), self.document.height()).unwrap());
        self.commit();
        for y in rect.y()..rect.y() + rect.height() {
            for x in rect.x()..rect.x() + rect.width() {
                self.document.set_pixel(self.active_layer, x, y, color)?;
            }
        }
        Ok(true)
    }
    /// Copies the selected area of the active layer into the editor clipboard.
    pub fn copy_selection(&mut self) -> Result<bool, DocumentError> {
        if self.move_preview.is_some() || self.brush_stroke.is_some() {
            return Ok(false);
        }
        let rect = self
            .selection
            .unwrap_or(Rect::new(0, 0, self.document.width(), self.document.height()).unwrap());
        let layer = self
            .document
            .layers
            .iter()
            .find(|layer| layer.id() == self.active_layer)
            .ok_or(DocumentError::UnknownLayer(self.active_layer))?;
        let mut pixels = Vec::with_capacity((rect.width() * rect.height()) as usize);
        let canvas_width = self.document.width() as usize;
        for y in rect.y()..rect.y() + rect.height() {
            for x in rect.x()..rect.x() + rect.width() {
                pixels.push(layer.pixels[y as usize * canvas_width + x as usize]);
            }
        }
        self.clipboard = Some(Clipboard {
            width: rect.width(),
            height: rect.height(),
            pixels,
        });
        Ok(true)
    }
    /// Copies and clears the selected active-layer pixels as one undoable edit.
    pub fn cut_selection(&mut self) -> Result<bool, DocumentError> {
        if !self.copy_selection()? {
            return Ok(false);
        }
        let rect = self
            .selection
            .unwrap_or(Rect::new(0, 0, self.document.width(), self.document.height()).unwrap());
        self.commit();
        let canvas_width = self.document.width() as usize;
        let layer = self
            .document
            .layers
            .iter_mut()
            .find(|layer| layer.id() == self.active_layer)
            .ok_or(DocumentError::UnknownLayer(self.active_layer))?;
        for y in rect.y()..rect.y() + rect.height() {
            for x in rect.x()..rect.x() + rect.width() {
                layer.pixels[y as usize * canvas_width + x as usize] = Rgba8::TRANSPARENT;
            }
        }
        self.document.revision += 1;
        Ok(true)
    }
    /// Pastes the clipboard to a new layer at the current selection origin.
    pub fn paste_clipboard(&mut self) -> Result<bool, DocumentError> {
        if self.move_preview.is_some() || self.brush_stroke.is_some() {
            return Ok(false);
        }
        let Some(clipboard) = self.clipboard.clone() else {
            return Ok(false);
        };
        let (origin_x, origin_y) = self.selection.map_or((0, 0), |rect| (rect.x(), rect.y()));
        self.commit();
        let layer_id = self.document.add_layer("Pasted");
        let canvas_width = self.document.width() as usize;
        let canvas_height = self.document.height();
        let layer = self
            .document
            .layers
            .iter_mut()
            .find(|layer| layer.id() == layer_id)
            .ok_or(DocumentError::UnknownLayer(layer_id))?;
        for y in 0..clipboard.height {
            for x in 0..clipboard.width {
                let destination_x = origin_x + x;
                let destination_y = origin_y + y;
                if destination_x < canvas_width as u32 && destination_y < canvas_height {
                    layer.pixels[destination_y as usize * canvas_width + destination_x as usize] =
                        clipboard.pixels[y as usize * clipboard.width as usize + x as usize];
                }
            }
        }
        self.active_layer = layer_id;
        Ok(true)
    }
    /// Draws one pixel-aligned pencil stroke. A complete stroke is one undo step.
    pub fn draw_line(
        &mut self,
        start: (u32, u32),
        end: (u32, u32),
        color: Rgba8,
    ) -> Result<bool, DocumentError> {
        self.draw_brush_line(start, end, color, 1)
    }
    /// Draws a square pixel brush stroke. `diameter` is clamped to at least one.
    pub fn draw_brush_line(
        &mut self,
        start: (u32, u32),
        end: (u32, u32),
        color: Rgba8,
        diameter: u32,
    ) -> Result<bool, DocumentError> {
        if self.move_preview.is_some() || self.brush_stroke.is_some() {
            return Ok(false);
        }
        let snapshot = self.snapshot();
        let changed = self.paint_brush_line(start, end, color, diameter)?;
        if changed {
            self.undo.push(snapshot);
            self.redo.clear();
        }
        Ok(changed)
    }
    /// Begins a live brush stroke. Call `finish_brush_stroke` to make its
    /// accumulated edits one undoable action, or `cancel_brush_stroke` to revert.
    pub fn begin_brush_stroke(&mut self) -> bool {
        if self.move_preview.is_some() || self.brush_stroke.is_some() {
            return false;
        }
        self.brush_stroke = Some((self.snapshot(), false));
        true
    }
    pub fn draw_brush_segment(
        &mut self,
        start: (u32, u32),
        end: (u32, u32),
        color: Rgba8,
        diameter: u32,
    ) -> Result<bool, DocumentError> {
        if self.brush_stroke.is_none() {
            return Ok(false);
        }
        let changed = self.paint_brush_line(start, end, color, diameter)?;
        if changed {
            if let Some((_, stroke_changed)) = &mut self.brush_stroke {
                *stroke_changed = true;
            }
        }
        Ok(changed)
    }
    pub fn finish_brush_stroke(&mut self) -> bool {
        let Some((snapshot, changed)) = self.brush_stroke.take() else {
            return false;
        };
        if changed {
            self.undo.push(snapshot);
            self.redo.clear();
        }
        changed
    }
    pub fn cancel_brush_stroke(&mut self) -> bool {
        let Some((snapshot, _)) = self.brush_stroke.take() else {
            return false;
        };
        self.restore(snapshot);
        true
    }
    fn paint_brush_line(
        &mut self,
        start: (u32, u32),
        end: (u32, u32),
        color: Rgba8,
        diameter: u32,
    ) -> Result<bool, DocumentError> {
        let diameter = diameter.max(1);
        let before = (diameter - 1) / 2;
        let after = diameter / 2;
        let mut points = Vec::new();
        raster_line(start, end, |x, y| {
            let right = x.saturating_add(after).min(self.document.width() - 1);
            let bottom = y.saturating_add(after).min(self.document.height() - 1);
            for brush_y in y.saturating_sub(before)..=bottom {
                for brush_x in x.saturating_sub(before)..=right {
                    if self
                        .selection
                        .is_none_or(|selection| selection.contains(brush_x, brush_y))
                    {
                        points.push((brush_x, brush_y));
                    }
                }
            }
        });
        if points.is_empty() {
            return Ok(false);
        }
        for (x, y) in points {
            self.document.set_pixel(self.active_layer, x, y, color)?;
        }
        Ok(true)
    }
    /// Flood-fills the contiguous same-colour region under `start`.
    pub fn bucket_fill(&mut self, start: (u32, u32), color: Rgba8) -> Result<bool, DocumentError> {
        self.bucket_fill_with_tolerance(start, color, 0)
    }
    /// Flood-fills contiguous pixels whose per-channel difference from the
    /// source pixel is at most `tolerance`.
    pub fn bucket_fill_with_tolerance(
        &mut self,
        start: (u32, u32),
        color: Rgba8,
        tolerance: u8,
    ) -> Result<bool, DocumentError> {
        if self.move_preview.is_some()
            || start.0 >= self.document.width()
            || start.1 >= self.document.height()
            || self
                .selection
                .is_some_and(|selection| !selection.contains(start.0, start.1))
        {
            return Ok(false);
        }
        let source = self.document.pixel(self.active_layer, start.0, start.1)?;
        if source == color {
            return Ok(false);
        }
        let width = self.document.width();
        let height = self.document.height();
        let mut queued = vec![false; width as usize * height as usize];
        let mut queue = VecDeque::from([start]);
        let mut points = Vec::new();
        queued[start.1 as usize * width as usize + start.0 as usize] = true;
        while let Some((x, y)) = queue.pop_front() {
            if self
                .selection
                .is_some_and(|selection| !selection.contains(x, y))
                || !within_tolerance(
                    self.document.pixel(self.active_layer, x, y)?,
                    source,
                    tolerance,
                )
            {
                continue;
            }
            points.push((x, y));
            for (next_x, next_y) in [
                x.checked_sub(1).map(|next| (next, y)),
                y.checked_sub(1).map(|next| (x, next)),
                (x + 1 < width).then_some((x + 1, y)),
                (y + 1 < height).then_some((x, y + 1)),
            ]
            .into_iter()
            .flatten()
            {
                let index = next_y as usize * width as usize + next_x as usize;
                if !queued[index] {
                    queued[index] = true;
                    queue.push_back((next_x, next_y));
                }
            }
        }
        if points.is_empty() {
            return Ok(false);
        }
        self.commit();
        for (x, y) in points {
            self.document.set_pixel(self.active_layer, x, y, color)?;
        }
        Ok(true)
    }
    pub fn add_layer(&mut self, name: impl Into<String>) -> LayerId {
        self.commit();
        let layer = self.document.add_layer(name);
        self.active_layer = layer;
        layer
    }
    pub fn delete_active_layer(&mut self) -> Result<bool, DocumentError> {
        self.commit();
        if let Err(error) = self.document.delete_layer(self.active_layer) {
            self.undo.pop();
            return Err(error);
        }
        self.active_layer = self.document.layers()[0].id();
        Ok(true)
    }
    pub fn duplicate_active_layer(&mut self) -> Result<LayerId, DocumentError> {
        self.commit();
        let layer = self.document.duplicate_layer(self.active_layer)?;
        self.active_layer = layer;
        Ok(layer)
    }
    pub fn move_active_layer(&mut self, destination: usize) -> Result<bool, DocumentError> {
        let source = self
            .document
            .layers()
            .iter()
            .position(|layer| layer.id() == self.active_layer)
            .ok_or(DocumentError::UnknownLayer(self.active_layer))?;
        if source == destination {
            return Ok(false);
        }
        self.commit();
        if let Err(error) = self.document.move_layer(self.active_layer, destination) {
            self.undo.pop();
            return Err(error);
        }
        Ok(true)
    }
    pub fn rename_active_layer(&mut self, name: String) -> Result<bool, DocumentError> {
        let current = self
            .document
            .layers()
            .iter()
            .find(|item| item.id() == self.active_layer)
            .ok_or(DocumentError::UnknownLayer(self.active_layer))?
            .name()
            .to_owned();
        if current == name {
            return Ok(false);
        }
        self.commit();
        self.document.rename_layer(self.active_layer, name)?;
        Ok(true)
    }
    pub fn merge_active_layer_down(&mut self) -> Result<bool, DocumentError> {
        self.commit();
        match self.document.merge_layer_down(self.active_layer) {
            Ok(layer) => {
                self.active_layer = layer;
                Ok(true)
            }
            Err(error) => {
                self.undo.pop();
                Err(error)
            }
        }
    }
    pub fn set_layer_visible(
        &mut self,
        layer: LayerId,
        visible: bool,
    ) -> Result<bool, DocumentError> {
        let current = self
            .document
            .layers()
            .iter()
            .find(|item| item.id() == layer)
            .ok_or(DocumentError::UnknownLayer(layer))?
            .is_visible();
        if current == visible {
            return Ok(false);
        }
        self.commit();
        self.document.set_layer_visible(layer, visible)?;
        Ok(true)
    }
    pub fn set_layer_opacity(
        &mut self,
        layer: LayerId,
        opacity: u8,
    ) -> Result<bool, DocumentError> {
        let current = self
            .document
            .layers()
            .iter()
            .find(|item| item.id() == layer)
            .ok_or(DocumentError::UnknownLayer(layer))?
            .opacity();
        if current == opacity {
            return Ok(false);
        }
        self.commit();
        self.document.set_layer_opacity(layer, opacity)?;
        Ok(true)
    }
    pub fn set_layer_blend_mode(
        &mut self,
        layer: LayerId,
        mode: BlendMode,
    ) -> Result<bool, DocumentError> {
        let current = self
            .document
            .layers()
            .iter()
            .find(|item| item.id() == layer)
            .ok_or(DocumentError::UnknownLayer(layer))?
            .blend_mode();
        if current == mode {
            return Ok(false);
        }
        self.commit();
        self.document.set_layer_blend_mode(layer, mode)?;
        Ok(true)
    }
    pub fn has_move_preview(&self) -> bool {
        self.move_preview.is_some()
    }
    /// Freezes the active selection as the source of an interactive move.
    pub fn begin_move_pixels(&mut self) -> Result<bool, DocumentError> {
        if self.move_preview.is_some() || self.brush_stroke.is_some() {
            return Ok(false);
        }
        let rect = self
            .selection
            .unwrap_or(Rect::new(0, 0, self.document.width(), self.document.height()).unwrap());
        let layer = self
            .document
            .layers
            .iter()
            .find(|layer| layer.id() == self.active_layer)
            .ok_or(DocumentError::UnknownLayer(self.active_layer))?;
        self.move_preview = Some((rect, (0, 0), layer.pixels.clone()));
        Ok(true)
    }
    /// Updates the offset of a staged move without changing the document.
    pub fn update_move_pixels(&mut self, offset: (i32, i32)) -> bool {
        let Some((_, current, _)) = &mut self.move_preview else {
            return false;
        };
        if *current == offset {
            return false;
        }
        *current = offset;
        true
    }
    pub fn stage_move_pixels(&mut self, offset: (i32, i32)) -> Result<bool, DocumentError> {
        if offset == (0, 0) {
            return Ok(false);
        }
        if !self.begin_move_pixels()? {
            return Ok(false);
        }
        Ok(self.update_move_pixels(offset))
    }
    pub fn cancel_move_pixels(&mut self) -> bool {
        self.move_preview.take().is_some()
    }
    /// Returns the composite with a staged move applied, without committing it.
    pub fn preview_composite(&self) -> Vec<Rgba8> {
        let Some((rect, offset, source)) = &self.move_preview else {
            return self.document.composite();
        };
        let mut preview = self.document.clone();
        let width = preview.width() as usize;
        let height = preview.height() as usize;
        if let Some(layer) = preview
            .layers
            .iter_mut()
            .find(|layer| layer.id() == self.active_layer)
        {
            for y in rect.y() as usize..(rect.y() + rect.height()) as usize {
                for x in rect.x() as usize..(rect.x() + rect.width()) as usize {
                    layer.pixels[y * width + x] = Rgba8::TRANSPARENT;
                }
            }
            for y in rect.y() as usize..(rect.y() + rect.height()) as usize {
                for x in rect.x() as usize..(rect.x() + rect.width()) as usize {
                    let dx = x as i32 + offset.0;
                    let dy = y as i32 + offset.1;
                    if dx >= 0 && dy >= 0 && (dx as usize) < width && (dy as usize) < height {
                        layer.pixels[dy as usize * width + dx as usize] = source[y * width + x];
                    }
                }
            }
        }
        preview.composite()
    }
    pub fn finish_move_pixels(&mut self) -> Result<bool, DocumentError> {
        let Some((rect, offset, source)) = self.move_preview.take() else {
            return Ok(false);
        };
        if offset == (0, 0) {
            return Ok(false);
        }
        self.commit();
        let width = self.document.width() as usize;
        let height = self.document.height() as usize;
        let layer = self
            .document
            .layers
            .iter_mut()
            .find(|layer| layer.id() == self.active_layer)
            .ok_or(DocumentError::UnknownLayer(self.active_layer))?;
        for y in rect.y() as usize..(rect.y() + rect.height()) as usize {
            for x in rect.x() as usize..(rect.x() + rect.width()) as usize {
                layer.pixels[y * width + x] = Rgba8::TRANSPARENT;
            }
        }
        for y in rect.y() as usize..(rect.y() + rect.height()) as usize {
            for x in rect.x() as usize..(rect.x() + rect.width()) as usize {
                let dx = x as i32 + offset.0;
                let dy = y as i32 + offset.1;
                if dx >= 0 && dy >= 0 && (dx as usize) < width && (dy as usize) < height {
                    layer.pixels[dy as usize * width + dx as usize] = source[y * width + x];
                }
            }
        }
        self.document.revision += 1;
        Ok(true)
    }
    pub fn set_selection(&mut self, selection: Option<Rect>) -> bool {
        if selection == self.selection {
            return false;
        }
        self.commit();
        self.selection = selection;
        true
    }
    pub fn crop_to_selection(&mut self) -> Result<bool, DocumentError> {
        let Some(rect) = self.selection else {
            return Ok(false);
        };
        self.commit();
        if let Err(error) = self.document.crop(rect) {
            self.undo.pop();
            return Err(error);
        }
        self.selection = None;
        Ok(true)
    }
    pub fn undo(&mut self) -> bool {
        if self.move_preview.is_some() || self.brush_stroke.is_some() {
            return false;
        }
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(previous);
        true
    }
    pub fn redo(&mut self) -> bool {
        if self.move_preview.is_some() || self.brush_stroke.is_some() {
            return false;
        }
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(next);
        true
    }
    fn commit(&mut self) {
        self.undo.push(self.snapshot());
        self.redo.clear();
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            document: self.document.clone(),
            selection: self.selection,
            active_layer: self.active_layer,
        }
    }
    fn restore(&mut self, snapshot: Snapshot) {
        self.document = snapshot.document;
        self.selection = snapshot.selection;
        self.active_layer = snapshot.active_layer;
    }
}

fn raster_line(start: (u32, u32), end: (u32, u32), mut visit: impl FnMut(u32, u32)) {
    let (mut x, mut y) = (i64::from(start.0), i64::from(start.1));
    let (end_x, end_y) = (i64::from(end.0), i64::from(end.1));
    let delta_x = (end_x - x).abs();
    let delta_y = -(end_y - y).abs();
    let step_x = if x < end_x { 1 } else { -1 };
    let step_y = if y < end_y { 1 } else { -1 };
    let mut error = delta_x + delta_y;
    loop {
        visit(x as u32, y as u32);
        if x == end_x && y == end_y {
            break;
        }
        let doubled = 2 * error;
        if doubled >= delta_y {
            error += delta_y;
            x += step_x;
        }
        if doubled <= delta_x {
            error += delta_x;
            y += step_y;
        }
    }
}

fn within_tolerance(left: Rgba8, right: Rgba8, tolerance: u8) -> bool {
    [
        left.red.abs_diff(right.red),
        left.green.abs_diff(right.green),
        left.blue.abs_diff(right.blue),
        left.alpha.abs_diff(right.alpha),
    ]
    .into_iter()
    .all(|difference| difference <= tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_preserves_overlapping_source_pixels() {
        let mut document = Document::new(3, 1).unwrap();
        let layer = document.layers()[0].id();
        document
            .set_pixel(layer, 0, 0, Rgba8::new(1, 0, 0, 255))
            .unwrap();
        document
            .set_pixel(layer, 1, 0, Rgba8::new(2, 0, 0, 255))
            .unwrap();
        let mut editor = Editor::new(document);
        editor.set_selection(Some(Rect::new(0, 0, 2, 1).unwrap()));
        editor.stage_move_pixels((1, 0)).unwrap();
        editor.finish_move_pixels().unwrap();
        assert_eq!(
            editor.document().pixel(layer, 1, 0),
            Ok(Rgba8::new(1, 0, 0, 255))
        );
        assert_eq!(
            editor.document().pixel(layer, 2, 0),
            Ok(Rgba8::new(2, 0, 0, 255))
        );
    }

    #[test]
    fn move_cancel_and_undo_restore_the_committed_document() {
        let mut document = Document::new(2, 1).unwrap();
        let layer = document.layers()[0].id();
        document
            .set_pixel(layer, 0, 0, Rgba8::new(4, 5, 6, 255))
            .unwrap();
        let mut editor = Editor::new(document);
        editor.stage_move_pixels((1, 0)).unwrap();
        assert!(editor.cancel_move_pixels());
        assert_eq!(
            editor.document().pixel(layer, 0, 0),
            Ok(Rgba8::new(4, 5, 6, 255))
        );
        editor.stage_move_pixels((1, 0)).unwrap();
        editor.finish_move_pixels().unwrap();
        assert!(editor.undo());
        assert_eq!(
            editor.document().pixel(layer, 0, 0),
            Ok(Rgba8::new(4, 5, 6, 255))
        );
        assert!(editor.redo());
        assert_eq!(
            editor.document().pixel(layer, 1, 0),
            Ok(Rgba8::new(4, 5, 6, 255))
        );
    }

    #[test]
    fn interactive_move_updates_its_preview_before_finishing() {
        let mut document = Document::new(2, 1).unwrap();
        let layer = document.layers()[0].id();
        document
            .set_pixel(layer, 0, 0, Rgba8::new(1, 2, 3, 255))
            .unwrap();
        let mut editor = Editor::new(document);
        assert!(editor.begin_move_pixels().unwrap());
        assert!(editor.update_move_pixels((1, 0)));
        assert_eq!(editor.preview_composite()[1], Rgba8::new(1, 2, 3, 255));
        assert!(editor.finish_move_pixels().unwrap());
        assert_eq!(
            editor.document().pixel(layer, 1, 0),
            Ok(Rgba8::new(1, 2, 3, 255))
        );
    }

    #[test]
    fn fill_selection_is_undoable() {
        let document = Document::new(2, 1).unwrap();
        let layer = document.layers()[0].id();
        let mut editor = Editor::new(document);
        editor.set_selection(Some(Rect::new(1, 0, 1, 1).unwrap()));
        editor.fill_selection(Rgba8::new(20, 30, 40, 255)).unwrap();
        assert_eq!(
            editor.document().pixel(layer, 1, 0),
            Ok(Rgba8::new(20, 30, 40, 255))
        );
        assert!(editor.undo());
        assert_eq!(editor.document().pixel(layer, 1, 0), Ok(Rgba8::TRANSPARENT));
    }

    #[test]
    fn moving_active_layer_is_undoable() {
        let mut editor = Editor::new(Document::new(1, 1).unwrap());
        let bottom = editor.active_layer();
        let top = editor.add_layer("Top");
        assert!(editor.move_active_layer(0).unwrap());
        assert_eq!(editor.document().layers()[0].id(), top);
        assert!(editor.undo());
        assert_eq!(editor.document().layers()[0].id(), bottom);
    }

    #[test]
    fn pencil_line_is_selection_aware_and_undoable() {
        let mut editor = Editor::new(Document::new(3, 1).unwrap());
        let layer = editor.active_layer();
        editor.set_selection(Some(Rect::new(1, 0, 1, 1).unwrap()));
        editor
            .draw_line((0, 0), (2, 0), Rgba8::new(10, 20, 30, 255))
            .unwrap();
        assert_eq!(editor.document().pixel(layer, 0, 0), Ok(Rgba8::TRANSPARENT));
        assert_eq!(
            editor.document().pixel(layer, 1, 0),
            Ok(Rgba8::new(10, 20, 30, 255))
        );
        assert!(editor.undo());
        assert_eq!(editor.document().pixel(layer, 1, 0), Ok(Rgba8::TRANSPARENT));
    }

    #[test]
    fn brush_size_expands_the_stroke() {
        let mut editor = Editor::new(Document::new(5, 5).unwrap());
        let layer = editor.active_layer();
        editor
            .draw_brush_line((2, 2), (2, 2), Rgba8::new(1, 2, 3, 255), 3)
            .unwrap();
        assert_eq!(
            editor.document().pixel(layer, 1, 1),
            Ok(Rgba8::new(1, 2, 3, 255))
        );
        assert_eq!(
            editor.document().pixel(layer, 3, 3),
            Ok(Rgba8::new(1, 2, 3, 255))
        );
        assert_eq!(editor.document().pixel(layer, 0, 0), Ok(Rgba8::TRANSPARENT));
    }

    #[test]
    fn live_brush_stroke_has_one_undo_step_and_can_be_cancelled() {
        let mut editor = Editor::new(Document::new(4, 1).unwrap());
        let layer = editor.active_layer();
        assert!(editor.begin_brush_stroke());
        editor
            .draw_brush_segment((0, 0), (1, 0), Rgba8::new(3, 4, 5, 255), 1)
            .unwrap();
        editor
            .draw_brush_segment((1, 0), (2, 0), Rgba8::new(3, 4, 5, 255), 1)
            .unwrap();
        assert!(editor.finish_brush_stroke());
        assert!(editor.undo());
        assert_eq!(editor.document().pixel(layer, 2, 0), Ok(Rgba8::TRANSPARENT));
        assert!(editor.begin_brush_stroke());
        editor
            .draw_brush_segment((0, 0), (1, 0), Rgba8::new(9, 9, 9, 255), 1)
            .unwrap();
        assert!(editor.cancel_brush_stroke());
        assert_eq!(editor.document().pixel(layer, 1, 0), Ok(Rgba8::TRANSPARENT));
    }

    #[test]
    fn cut_and_paste_use_a_new_layer_and_undo_cleanly() {
        let mut editor = Editor::new(Document::new(2, 1).unwrap());
        let base = editor.active_layer();
        editor
            .document
            .set_pixel(base, 0, 0, Rgba8::new(1, 2, 3, 255))
            .unwrap();
        editor.set_selection(Some(Rect::new(0, 0, 1, 1).unwrap()));
        assert!(editor.cut_selection().unwrap());
        assert_eq!(editor.document().pixel(base, 0, 0), Ok(Rgba8::TRANSPARENT));
        assert!(editor.paste_clipboard().unwrap());
        assert_eq!(editor.document().layers().len(), 2);
        assert_eq!(
            editor.document().pixel(editor.active_layer(), 0, 0),
            Ok(Rgba8::new(1, 2, 3, 255))
        );
        assert!(editor.undo());
        assert_eq!(editor.document().layers().len(), 1);
        assert_eq!(editor.active_layer(), base);
    }

    #[test]
    fn bucket_fill_is_contiguous_and_undoable() {
        let mut editor = Editor::new(Document::new(3, 1).unwrap());
        let layer = editor.active_layer();
        editor
            .document
            .set_pixel(layer, 2, 0, Rgba8::new(1, 1, 1, 255))
            .unwrap();
        editor
            .bucket_fill((0, 0), Rgba8::new(9, 8, 7, 255))
            .unwrap();
        assert_eq!(
            editor.document().pixel(layer, 1, 0),
            Ok(Rgba8::new(9, 8, 7, 255))
        );
        assert_eq!(
            editor.document().pixel(layer, 2, 0),
            Ok(Rgba8::new(1, 1, 1, 255))
        );
        assert!(editor.undo());
        assert_eq!(editor.document().pixel(layer, 0, 0), Ok(Rgba8::TRANSPARENT));
    }

    #[test]
    fn bucket_tolerance_includes_nearby_colours_only_when_requested() {
        let mut document = Document::new(2, 1).unwrap();
        let layer = document.layers()[0].id();
        document
            .set_pixel(layer, 0, 0, Rgba8::new(0, 0, 0, 255))
            .unwrap();
        document
            .set_pixel(layer, 1, 0, Rgba8::new(3, 0, 0, 255))
            .unwrap();
        let mut editor = Editor::new(document);
        editor
            .bucket_fill((0, 0), Rgba8::new(9, 0, 0, 255))
            .unwrap();
        assert_eq!(
            editor.document().pixel(layer, 1, 0),
            Ok(Rgba8::new(3, 0, 0, 255))
        );
        assert!(editor.undo());
        editor
            .bucket_fill_with_tolerance((0, 0), Rgba8::new(9, 0, 0, 255), 3)
            .unwrap();
        assert_eq!(
            editor.document().pixel(layer, 1, 0),
            Ok(Rgba8::new(9, 0, 0, 255))
        );
    }
}
