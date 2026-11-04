//! Geometry and color primitives shared by every layer. Coordinates are in
//! slide units: the canvas is always 1920 wide, and height follows the aspect.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }
}

/// Straight (non-premultiplied) sRGB color with alpha, 0–255.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Color { a, ..self }
    }
}

/// Output aspect ratios offered by "Make Video".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Aspect {
    #[default]
    Wide16x9,
    Tall9x16,
    Square1x1,
}

impl Aspect {
    pub const CANVAS_WIDTH: f64 = 1920.0;

    pub fn canvas_size(self) -> (f64, f64) {
        match self {
            Aspect::Wide16x9 => (1920.0, 1080.0),
            Aspect::Tall9x16 => (1920.0, 1920.0 * 16.0 / 9.0),
            Aspect::Square1x1 => (1920.0, 1920.0),
        }
    }
}
