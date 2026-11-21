use std::fmt;

/// A rectangular region in document coordinates, with exclusive right and
/// bottom edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Result<Self, SelectionError> {
        if width == 0 || height == 0 {
            return Err(SelectionError::EmptyRectangle);
        }
        x.checked_add(width)
            .zip(y.checked_add(height))
            .ok_or(SelectionError::OutOfRange)?;
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    pub fn x(&self) -> u32 {
        self.x
    }
    pub fn y(&self) -> u32 {
        self.y
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn from_inclusive_points(
        first: (u32, u32),
        second: (u32, u32),
        canvas: (u32, u32),
    ) -> Option<Self> {
        if canvas.0 == 0 || canvas.1 == 0 {
            return None;
        }
        let left = first.0.min(second.0).min(canvas.0 - 1);
        let top = first.1.min(second.1).min(canvas.1 - 1);
        let right = first.0.max(second.0).min(canvas.0 - 1);
        let bottom = first.1.max(second.1).min(canvas.1 - 1);
        Self::new(left, top, right - left + 1, bottom - top + 1).ok()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Selection {
    #[default]
    All,
    Rectangle(Rect),
    Mask {
        width: u32,
        height: u32,
        coverage: Vec<bool>,
    },
}

impl Selection {
    pub fn rectangle(rectangle: Rect) -> Self {
        Self::Rectangle(rectangle)
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        match self {
            Self::All => true,
            Self::Rectangle(rectangle) => rectangle.contains(x, y),
            Self::Mask {
                width,
                height,
                coverage,
            } => x < *width && y < *height && coverage[y as usize * *width as usize + x as usize],
        }
    }

    /// Combines two selections inside the given document bounds. Resulting
    /// coverage is binary; antialiased masks are a later rendering layer.
    pub fn combine(
        &self,
        other: &Self,
        width: u32,
        height: u32,
        mode: SelectionCombine,
    ) -> Result<Self, SelectionError> {
        let count = pixel_count(width, height)?;
        let mut coverage = Vec::with_capacity(count);
        for y in 0..height {
            for x in 0..width {
                let left = self.contains(x, y);
                let right = other.contains(x, y);
                coverage.push(match mode {
                    SelectionCombine::Replace => right,
                    SelectionCombine::Add => left || right,
                    SelectionCombine::Subtract => left && !right,
                    SelectionCombine::Intersect => left && right,
                    SelectionCombine::Xor => left != right,
                });
            }
        }
        Ok(Self::Mask {
            width,
            height,
            coverage,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionCombine {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
    Xor,
}

fn pixel_count(width: u32, height: u32) -> Result<usize, SelectionError> {
    if width == 0 || height == 0 {
        return Err(SelectionError::EmptyCanvas);
    }
    usize::try_from(u64::from(width) * u64::from(height)).map_err(|_| SelectionError::OutOfRange)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionError {
    EmptyRectangle,
    EmptyCanvas,
    OutOfRange,
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRectangle => f.write_str("selection rectangle must be non-empty"),
            Self::EmptyCanvas => f.write_str("selection requires non-zero document dimensions"),
            Self::OutOfRange => f.write_str("selection rectangle exceeds document coordinates"),
        }
    }
}

impl std::error::Error for SelectionError {}
