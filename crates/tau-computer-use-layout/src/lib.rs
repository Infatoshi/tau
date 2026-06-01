use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPoint {
    pub x: i64,
    pub y: i64,
}

impl ScreenPoint {
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenFrame {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl ScreenFrame {
    pub const fn new(x: i64, y: i64, width: i64, height: i64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn top_left(&self) -> ScreenPoint {
        ScreenPoint::new(self.x, self.y)
    }

    pub const fn center(&self) -> ScreenPoint {
        ScreenPoint::new(self.x + (self.width / 2), self.y + (self.height / 2))
    }

    pub fn contains(&self, point: ScreenPoint) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.x + self.width
            && point.y < self.y + self.height
    }

    pub fn screencapture_region(&self) -> String {
        format!("{},{},{},{}", self.x, self.y, self.width, self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutParseError {
    InvalidPartCount {
        label: String,
        raw: String,
    },
    InvalidNumber {
        label: String,
        name: &'static str,
        raw: String,
    },
}

impl fmt::Display for LayoutParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPartCount { label, raw } => {
                write!(f, "invalid {label} from accessibility: {raw}")
            }
            Self::InvalidNumber { label, name, raw } => {
                write!(f, "invalid {name} in {label} from accessibility: {raw}")
            }
        }
    }
}

impl std::error::Error for LayoutParseError {}

pub fn parse_point_csv(content: &str) -> Result<ScreenPoint, LayoutParseError> {
    let parts = parse_i64_csv(content, 2, "point")?;
    Ok(ScreenPoint::new(parts[0], parts[1]))
}

pub fn parse_frame_csv(content: &str) -> Result<ScreenFrame, LayoutParseError> {
    let parts = parse_i64_csv(content, 4, "app window frame")?;
    Ok(ScreenFrame::new(parts[0], parts[1], parts[2], parts[3]))
}

pub fn parse_i64_csv(
    content: &str,
    expected_len: usize,
    label: &str,
) -> Result<Vec<i64>, LayoutParseError> {
    let raw = content.trim().to_string();
    let parts = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() != expected_len {
        return Err(LayoutParseError::InvalidPartCount {
            label: label.to_string(),
            raw,
        });
    }
    parts
        .iter()
        .enumerate()
        .map(|(idx, value)| {
            value
                .parse::<i64>()
                .map_err(|_| LayoutParseError::InvalidNumber {
                    label: label.to_string(),
                    name: coordinate_name(expected_len, idx),
                    raw: raw.clone(),
                })
        })
        .collect()
}

fn coordinate_name(expected_len: usize, idx: usize) -> &'static str {
    match (expected_len, idx) {
        (2, 0) | (4, 0) => "x",
        (2, 1) | (4, 1) => "y",
        (4, 2) => "width",
        (4, 3) => "height",
        _ => "value",
    }
}
