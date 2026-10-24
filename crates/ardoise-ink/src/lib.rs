//! Stroke model and geometry (pressure-dependent outlines), shared by the display and the
//! exports, so that the screen, PNG, SVG, PDF and video all show exactly the same strokes.
//!
//! - [`model`]: points, strokes, tools, colours (the data stored in notes).

pub mod model;

pub use model::{Point, Rgba, Stroke, Tool};
