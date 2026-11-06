//! Retouching a copy of a note (SPEC §4.9, `docs/design/revision.md`): an assistant looks at the
//! page and decides, this crate carries out its list of [`Order`]s on the strokes, and finds the
//! written lines from the ink ([`ink_lines`]).
//!
//! Pure: no I/O, no library. Its interface is in millimetres from the top left of the page; device
//! units (10 µm) stay inside. The source is only borrowed: it is never written.
//!
//! A stroke keeps its number for the whole list of orders: its index in [`Input::strokes`],
//! whatever is done to it. A stroke an order makes takes the next free number. Selections read a
//! stroke as it was given (or made), and geometry changes accumulate.

mod geom;
mod lines;
mod orders;
mod report;
mod shapes;
mod tidy;
mod time;
mod validate;
mod work;

use ardoise_ink::Stroke;

pub use lines::{InkLine, InkLines, InkWord, ink_lines};
pub use orders::{Flip, Order, Part, PenTool, Select, Shape, TidyGaps, TidyMode};
pub use report::{LineReport, OrderReport, Report, ReviseError, SlopeFrom, TimeReport};

use report::Ctx;
use work::Work;

/// Most orders in one list.
pub const MAX_ORDERS: usize = 5_000;
/// Most points the orders of one list can make. The note's own points do not count: a dense note
/// can still be retouched.
pub const MAX_POINTS: usize = 1_000_000;
/// Largest shape, millimetres.
pub const MAX_SHAPE_MM: f32 = 5_000.0;
/// Shortest dash or gap, millimetres.
pub const MIN_DASH_MM: f32 = 0.2;
/// Largest smoothing, millimetres.
pub const MAX_SIGMA_MM: f32 = 5.0;
/// Smallest size change.
pub const MIN_FACTOR: f32 = 0.05;
/// Largest size change.
pub const MAX_FACTOR: f32 = 20.0;
/// No place or distance is further than this, millimetres: the arithmetic stays finite, and the
/// file format can hold the result.
pub const MAX_COORD_MM: f32 = 100_000.0;
/// Widest pen, millimetres.
pub const MAX_WIDTH_MM: f32 = 100.0;
/// Steepest written line, degrees.
pub const MAX_SLOPE_DEG: f32 = 45.0;

/// The note the orders act on. It is only read.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    pub strokes: &'a [Stroke],
    /// Size of the page, millimetres.
    pub page_mm: [f32; 2],
    /// How many layers the note has.
    pub layers: u32,
    /// For each layer, whether the note shows it. A layer the list does not reach is shown, as the
    /// app draws a stroke whose layer has no entry. What a picture of the note does not show is not
    /// taken by a selection that does not ask for it, and new strokes do not go there by default.
    pub visible_layers: &'a [bool],
    /// A narration exists: nothing may move in time.
    pub narration: bool,
}

impl Input<'_> {
    /// Whether the note shows this layer.
    pub(crate) fn shows(&self, layer: u32) -> bool {
        self.visible_layers
            .get(layer as usize)
            .copied()
            .unwrap_or(true)
    }
}

/// What came out of a list of orders.
#[derive(Debug, Clone, PartialEq)]
pub struct Revised {
    /// The strokes, in the order of the note's list.
    pub strokes: Vec<Stroke>,
    /// For each of [`Revised::strokes`], its number in the orders. A picture of a preview must label
    /// strokes with these: once saved, a note's strokes are numbered by their place again.
    pub numbers: Vec<usize>,
    pub report: Report,
}

/// Carries out the list of orders on a copy of the strokes. Nothing comes out of a list with an
/// error: the message says which order is wrong, and why.
pub fn apply(input: &Input, orders: &[Order]) -> Result<Revised, ReviseError> {
    validate::check(input, orders)?;
    let mut work = Work::new(input);
    let mut reports = Vec::with_capacity(orders.len());
    for (index, order) in orders.iter().enumerate() {
        let ctx = Ctx {
            index,
            op: order.op(),
        };
        reports.push(work.run(ctx, order)?);
    }
    Ok(work.finish(reports))
}

#[cfg(test)]
mod tests;
