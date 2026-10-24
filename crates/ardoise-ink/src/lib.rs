//! Stroke model and geometry (pressure-dependent outlines), shared by the display and the
//! exports, so that the screen, PNG, SVG, PDF and video all show exactly the same strokes.
//!
//! - [`model`]: points, strokes, tools, colours (the data stored in notes).
//! - [`geometry`]: the line width of each tool ([`Tool::width_at`] and its inverse) and the
//!   filled outline of a stroke ([`outline`]).

pub mod geometry;
pub mod model;

pub use geometry::{Bounds, MAX_COORD, Outline, outline};
pub use model::{Point, Rgba, Stroke, Tool};
