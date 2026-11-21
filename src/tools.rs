use crate::{Document, History, HistoryError, LayerId, Rgba8, Selection};
use std::collections::VecDeque;
use std::fmt;

/// Pixel-aligned pencil operations.
pub struct PencilTool;

/// Exact-colour, 4-connected flood filling for an individual layer.
pub struct PaintBucketTool;

impl PencilTool {
    /// Draws an inclusive line from `start` to `end`, clipping it to the
    /// document bounds and active selection. The whole stroke is one undo step.
    pub fn draw_line(
        document: &mut Document,
        history: &mut History,
        layer: LayerId,
        start: (u32, u32),
        end: (u32, u32),
        color: Rgba8,
        selection: &Selection,
    ) -> Result<bool, PencilError> {
        let mut points = Vec::new();
        raster_line(start, end, |x, y| {
            if x < document.width() && y < document.height() && selection.contains(x, y) {
                points.push((x, y, color));
            }
        });
        history
            .set_pixels(document, layer, &points)
            .map_err(PencilError::History)
    }
}

impl PaintBucketTool {
    /// Fills the contiguous source-colour region containing `start`. Pixels
    /// outside the selection do not participate in connectivity or filling.
    pub fn fill(
        document: &mut Document,
        history: &mut History,
        layer: LayerId,
        start: (u32, u32),
        color: Rgba8,
        selection: &Selection,
    ) -> Result<bool, BucketError> {
        let source = document
            .pixel(layer, start.0, start.1)
            .map_err(HistoryError::from)
            .map_err(BucketError::History)?;
        if source == color || !selection.contains(start.0, start.1) {
            return Ok(false);
        }

        let width = document.width();
        let height = document.height();
        let mut queued = vec![false; width as usize * height as usize];
        let mut queue = VecDeque::from([start]);
        queued[index(width, start.0, start.1)] = true;
        let mut changes = Vec::new();

        while let Some((x, y)) = queue.pop_front() {
            if !selection.contains(x, y)
                || document
                    .pixel(layer, x, y)
                    .map_err(HistoryError::from)
                    .map_err(BucketError::History)?
                    != source
            {
                continue;
            }
            changes.push((x, y, color));
            for (next_x, next_y) in neighbours(x, y, width, height) {
                let next_index = index(width, next_x, next_y);
                if !queued[next_index] {
                    queued[next_index] = true;
                    queue.push_back((next_x, next_y));
                }
            }
        }
        history
            .set_pixels(document, layer, &changes)
            .map_err(BucketError::History)
    }
}

fn index(width: u32, x: u32, y: u32) -> usize {
    y as usize * width as usize + x as usize
}

fn neighbours(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    [
        x.checked_sub(1).map(|next_x| (next_x, y)),
        y.checked_sub(1).map(|next_y| (x, next_y)),
        (x + 1 < width).then_some((x + 1, y)),
        (y + 1 < height).then_some((x, y + 1)),
    ]
    .into_iter()
    .flatten()
}

fn raster_line(start: (u32, u32), end: (u32, u32), mut point: impl FnMut(u32, u32)) {
    let (mut x, mut y) = (i64::from(start.0), i64::from(start.1));
    let (end_x, end_y) = (i64::from(end.0), i64::from(end.1));
    let delta_x = (end_x - x).abs();
    let delta_y = -(end_y - y).abs();
    let step_x = if x < end_x { 1 } else { -1 };
    let step_y = if y < end_y { 1 } else { -1 };
    let mut error = delta_x + delta_y;

    loop {
        point(x as u32, y as u32);
        if x == end_x && y == end_y {
            break;
        }
        let doubled_error = 2 * error;
        if doubled_error >= delta_y {
            error += delta_y;
            x += step_x;
        }
        if doubled_error <= delta_x {
            error += delta_x;
            y += step_y;
        }
    }
}

#[derive(Debug)]
pub enum PencilError {
    History(HistoryError),
}

impl fmt::Display for PencilError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::History(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PencilError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::History(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub enum BucketError {
    History(HistoryError),
}

impl fmt::Display for BucketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::History(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for BucketError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::History(error) => Some(error),
        }
    }
}
