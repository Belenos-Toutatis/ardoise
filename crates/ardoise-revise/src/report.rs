//! What came of a list of orders: errors (nothing was done) and the report (what was done). Both
//! go back to the assistant, so they are written for it.

use serde::Serialize;
use thiserror::Error;

use crate::{MAX_ORDERS, geom::round2};

/// A list of orders refused: nothing was done. The message goes back to the assistant as it is.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReviseError {
    #[error("{count} orders: at most {MAX_ORDERS} are accepted in one list")]
    TooManyOrders { count: usize },
    /// `order` is the order's index in the list, from 0.
    #[error("order {order} ({op}): {message}")]
    Order {
        order: usize,
        op: &'static str,
        message: String,
    },
}

/// An order and its place in the list, to say which one an error is about.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ctx {
    pub index: usize,
    pub op: &'static str,
}

impl Ctx {
    pub fn err(self, message: impl Into<String>) -> ReviseError {
        ReviseError::Order {
            order: self.index,
            op: self.op,
            message: message.into(),
        }
    }
}

/// Where the slope of a tidied line comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeFrom {
    /// The order gave it.
    Given,
    /// Fitted through the lower outline of the strokes.
    Fitted,
    /// Taken as level: the line is under 40 mm or of few letters, where a fit goes wrong.
    ShortLine,
}

/// What `tidy_line` found and did: the assistant checks it against the picture.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LineReport {
    pub strokes: usize,
    pub words: usize,
    /// The slope taken away, in degrees (positive goes down to the right).
    pub slope: f32,
    pub slope_from: SlopeFrom,
    /// Height of the body of the letters, millimetres.
    pub body: f32,
    /// New y of the baseline.
    pub baseline: f32,
    /// New x of the start of the line.
    pub left: f32,
    /// The blank that separated the words, millimetres.
    pub word_gap: f32,
}

impl LineReport {
    pub(crate) fn rounded(mut self) -> Self {
        self.slope = round2(self.slope);
        self.body = round2(self.body);
        self.baseline = round2(self.baseline);
        self.left = round2(self.left);
        self.word_gap = round2(self.word_gap);
        self
    }
}

/// Where in time an order put its strokes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimeReport {
    /// The stroke they come after; absent when they went to the end of the note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<usize>,
    /// When the first of them starts, milliseconds on the clock of the note.
    pub start_ms: u32,
    /// When the last of them ends.
    pub end_ms: u32,
    /// How far what follows was pushed (0: it fit, or the note has a narration).
    pub pushed_ms: u32,
    /// How many strokes were pushed.
    pub pushed_strokes: usize,
    /// 1: drawn in their own time. Below 1: drawn in this fraction of it, because the note has a narration and nothing else may move.
    pub pace: f32,
}

/// What one order did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OrderReport {
    /// The order's name.
    pub op: &'static str,
    /// How many strokes its selection found.
    pub selected: usize,
    /// The numbers of the strokes it made, for later orders.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub new_strokes: Vec<usize>,
    /// What looks wrong, such as an empty selection.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Where in time its strokes went (`copy`, `glyph`, `add`, `retime`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<TimeReport>,
    /// What it found (`tidy_line`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<LineReport>,
}

impl OrderReport {
    pub(crate) fn new(op: &'static str) -> Self {
        Self {
            op,
            selected: 0,
            new_strokes: Vec::new(),
            warnings: Vec::new(),
            time: None,
            line: None,
        }
    }
}

/// What a list of orders did.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// Strokes of the note as given.
    pub strokes_before: usize,
    /// Strokes of the result.
    pub strokes_after: usize,
    /// Strokes left without any point, removed.
    pub empty_removed: usize,
    /// Numbers of the strokes moved or made that end up beyond the page: usually a mistake.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub off_page: Vec<usize>,
    /// Strokes of the note pushed later in time by the orders (each counted once).
    pub strokes_pushed: usize,
    /// How far they were pushed, in total, milliseconds.
    pub pushed_ms: u64,
    /// Strokes drawn faster because the note has a narration (each counted once).
    pub strokes_faster: usize,
    /// One entry per order, in the order of the list.
    pub orders: Vec<OrderReport>,
}
