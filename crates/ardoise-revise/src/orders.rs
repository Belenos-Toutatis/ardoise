//! The orders, as an assistant writes them in JSON.
//!
//! The doc comments of these types become the JSON schema the assistant reads
//! (`schemars::JsonSchema`, through the tools of `ardoise-mcp`): they are written for that reader.
//! Everything is in millimetres from the top left of the page, y pointing down.

use ardoise_ink::Tool;
use schemars::JsonSchema;
use serde::Deserialize;

/// Which strokes an order acts on. It is read on the note as it was given (a stroke made by an earlier order: as it was when made), whatever earlier orders did to it since, so reason on the picture you saw. `ids` and `rect` add up; with neither, every stroke is a candidate; the other fields then narrow. The picture does not show the strokes of hidden layers: `rect`, or no place at all, only takes drawn strokes, unless `layer` names a layer; `ids` takes what it names. The report warns when a hidden stroke is touched. Check `selected` in the report: a selection of the wrong size is the usual mistake.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Select {
    /// Stroke numbers: a stroke's index in the note, or the number the report gave a stroke made by an earlier order in this list. An unknown number is refused.
    pub ids: Option<Vec<usize>>,
    /// `[x, y, w, h]`: the drawn strokes whose centre (the centre of their box) is inside this rectangle.
    pub rect: Option<[f32; 4]>,
    /// Keeps the strokes of this colour, `#rrggbb` (any opacity) or `#rrggbbaa` (exactly), as `get_note` writes it. Useful when two colours overlap in a rectangle.
    pub color: Option<String>,
    /// Keeps the strokes of this layer (index in the note's layers), hidden or not.
    pub layer: Option<u32>,
    /// Keeps the strokes whose box has a larger side of at least this many millimetres.
    pub min_size: Option<f32>,
    /// Keeps the strokes whose box has a larger side of at most this many millimetres.
    pub max_size: Option<f32>,
    /// Stroke numbers to leave out.
    pub not: Option<Vec<usize>>,
}

/// A pen tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PenTool {
    /// The tablet's own pen.
    Ballpoint,
    FountainPen,
    FeltTip,
    Pencil,
    Brush,
    /// Wide and translucent: colours the paper and leaves dark ink readable.
    Highlighter,
    /// For a blackboard background.
    Chalk,
}

impl From<PenTool> for Tool {
    fn from(tool: PenTool) -> Self {
        match tool {
            PenTool::Ballpoint => Tool::Ballpoint,
            PenTool::FountainPen => Tool::FountainPen,
            PenTool::FeltTip => Tool::FeltTip,
            PenTool::Pencil => Tool::Pencil,
            PenTool::Brush => Tool::Brush,
            PenTool::Highlighter => Tool::Highlighter,
            PenTool::Chalk => Tool::Chalk,
        }
    }
}

/// A drawn shape, in millimetres from the top left of the page. Each is one stroke, except `dashed` (one stroke per dash, and one per arrow head).
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Shape {
    /// A straight line.
    Line {
        /// Start, `[x, y]`.
        from: [f32; 2],
        /// End, `[x, y]`.
        to: [f32; 2],
    },
    /// Straight segments through these points.
    Polyline {
        /// At least 2 points `[x, y]`, at most 100,000.
        points: Vec<[f32; 2]>,
        /// Comes back to the first point.
        #[serde(default)]
        closed: bool,
    },
    /// A rectangle with sides parallel to the page.
    Rect {
        /// Left.
        x: f32,
        /// Top.
        y: f32,
        /// Width.
        w: f32,
        /// Height.
        h: f32,
    },
    /// A circle.
    Circle {
        /// Centre, `[x, y]`.
        c: [f32; 2],
        /// Radius, above 0.
        r: f32,
    },
    /// An ellipse.
    Ellipse {
        /// Centre, `[x, y]`.
        c: [f32; 2],
        /// Half-axis along x before turning, above 0.
        rx: f32,
        /// Half-axis along y before turning, above 0.
        ry: f32,
        /// Degrees, clockwise as seen on the page.
        #[serde(default)]
        angle: f32,
    },
    /// An arrow in one stroke: tail, tip, one barb, tip, other barb, as the editor's straightening draws it.
    Arrow {
        /// Tail, `[x, y]`.
        from: [f32; 2],
        /// Tip, `[x, y]`.
        to: [f32; 2],
        /// Length of the barbs (default 3.5).
        head: Option<f32>,
        /// A head at both ends.
        #[serde(default)]
        both: bool,
    },
    /// A dashed line: an axis, a construction line.
    Dashed {
        /// Start, `[x, y]`.
        from: [f32; 2],
        /// End, `[x, y]`.
        to: [f32; 2],
        /// Length of a dash, at least 0.2.
        dash: f32,
        /// Space between dashes, at least 0.2.
        gap: f32,
        /// When given, arrow heads of this length at both ends.
        head: Option<f32>,
    },
    /// A smooth curve (Catmull-Rom) through these points.
    Curve {
        /// At least 2 points `[x, y]`, at most 100,000.
        points: Vec<[f32; 2]>,
    },
}

/// How a piece of a `glyph` is mirrored or turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Flip {
    /// Mirrored left-right.
    H,
    /// Mirrored top-bottom.
    V,
    /// Half a turn (an u turned gives an n).
    Rot,
}

/// A piece of a `glyph`: a stroke as it is at this point of the list, or some of its points, mirrored or turned, resized, then placed. A piece's duration in the replay is the one this piece was written in, not its whole stroke's: this is the way to reuse a letter of a word written in one stroke.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Part {
    /// The stroke's number. A letter only comes from the note being revised: only its strokes can be named here, and the points of another note are never to be retyped in a shape.
    pub stroke: usize,
    /// First point kept (index from `get_strokes`; default the first).
    pub from: Option<usize>,
    /// Last point kept, included (default the last).
    pub to: Option<usize>,
    /// `h` mirrors left-right, `v` top-bottom, `rot` turns half a turn.
    pub flip: Option<Flip>,
    /// Size across, 1 keeps it; 0.05 to 20.
    pub sx: Option<f32>,
    /// Size up and down, 1 keeps it; 0.05 to 20.
    pub sy: Option<f32>,
    /// Where the left side of this piece's box goes.
    pub x: f32,
    /// Where the bottom of this piece's box goes.
    pub y: f32,
}

/// How `tidy_line` treats its strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TidyMode {
    /// Cut into words, each one straightened, put on the baseline and placed in turn.
    #[default]
    Words,
    /// One block, only straightened and moved: for formulas (an exponent must stay above). Give the `slope` you see: exponents fool the fit.
    Rigid,
}

/// What `tidy_line` does with the spaces between words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TidyGaps {
    /// Brought closer to the line's usual space.
    #[default]
    Even,
    /// Kept as they are: for an intentional blank, such as a name left out.
    Keep,
}

/// One retouch. `op` says which. Orders run in the order of the list, and the whole list is sent again each time: always from the note as it is.
///
/// **Stroke numbers.** A stroke keeps its number for the whole list: its index in the note, whatever is done to it. A stroke an order makes (`copy`, `glyph`, `add`, `replace`) takes the next free number, starting after the note's last stroke, given in `new_strokes` in the report, and later orders can name it.
///
/// **Time.** `after` (on `copy`, `glyph`, `add`, `retime`) names the stroke the new or moved strokes come right after in the replay. Without `after`, new strokes come at the end of the note. They are written one after the other, each with its own duration, 40 ms apart. When they do not fit before what follows, what follows moves on; when the note has a narration, nothing else may move, and they are drawn faster instead.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Order {
    /// Changes the colour of the selected strokes.
    Recolor {
        /// The strokes.
        select: Select,
        /// New colour, `#rrggbb` (the stroke keeps its opacity) or `#rrggbbaa`.
        color: String,
    },
    /// Changes the pen of the selected strokes. Give `width`, `tool` or both.
    Restyle {
        /// The strokes.
        select: Select,
        /// New nominal pen width in millimetres (the tablet's pen: 0.5).
        width: Option<f32>,
        /// New tool.
        tool: Option<PenTool>,
    },
    /// Removes the selected strokes. Their numbers are not given to other strokes.
    Delete {
        /// The strokes.
        select: Select,
    },
    /// Moves the selected strokes.
    Move {
        /// The strokes.
        select: Select,
        /// Millimetres to the right.
        dx: f32,
        /// Millimetres down.
        dy: f32,
    },
    /// Resizes the selected strokes around `origin`. Give `factor`, or `sx` and `sy`; each from 0.05 to 20. The pen's width does not change.
    Scale {
        /// The strokes.
        select: Select,
        /// Same size change both ways.
        factor: Option<f32>,
        /// Size change across.
        sx: Option<f32>,
        /// Size change up and down.
        sy: Option<f32>,
        /// The point that stays where it is, `[x, y]`.
        origin: [f32; 2],
    },
    /// Turns the selected strokes around `origin`: a tilted block.
    Rotate {
        /// The strokes.
        select: Select,
        /// Degrees, clockwise as seen on the page.
        degrees: f32,
        /// The point that stays where it is, `[x, y]`.
        origin: [f32; 2],
    },
    /// Removes the tremble: a Gaussian along the stroke, whose ends stay where they are. A list can smooth at most 5,000,000 samples in all.
    Smooth {
        /// The strokes.
        select: Select,
        /// Millimetres along the stroke; 0.2 is light, at most 5.
        sigma: f32,
    },
    /// Keeps only these points of a stroke, which stays where it is in time. To reuse a letter of a word written in one stroke, use `glyph` with `from` and `to`: it times the piece only. A stroke an earlier order placed in time (`copy`, `glyph`, `add`, `retime`) is refused: its place was made for the whole stroke. Points are numbered as in `get_strokes`, and from 0 again after an earlier `trim` of the same stroke.
    Trim {
        /// The stroke's number.
        stroke: usize,
        /// First point kept.
        from: usize,
        /// Last point kept, included (default the last).
        to: Option<usize>,
    },
    /// Redraws: the shapes take the place of the selected strokes (at the place of the first of them in the drawing order, on its layer; the first on a visible layer when there is one) with its tool, colour and width, the strokes' median pressure and their time span. The selection must not be empty.
    Replace {
        /// The strokes replaced.
        select: Select,
        /// The new shapes, at least one.
        with: Vec<Shape>,
        /// Colour instead of the replaced strokes' one, `#rrggbb`.
        color: Option<String>,
        /// Pen width in millimetres instead of the replaced strokes' one.
        width: Option<f32>,
    },
    /// New strokes.
    Add {
        /// The shapes, at least one.
        shapes: Vec<Shape>,
        /// Their colour, `#rrggbb`.
        color: String,
        /// Pen width in millimetres (default: the width of the note's writing, or the tool's usual width when `tool` is another one).
        width: Option<f32>,
        /// Tool (default: the one of the note's writing). `highlighter` to mark something, usually with `behind`.
        tool: Option<PenTool>,
        /// Layer (default: the layer of the note's writing that the picture shows).
        layer: Option<u32>,
        /// Under the other strokes of the layer: a highlight.
        #[serde(default)]
        behind: bool,
        /// The stroke they come right after in the replay (default: at the end of the note).
        after: Option<usize>,
    },
    /// A copy of the selected strokes as they are at this point of the list, each keeping its tool, width and layer: moves a letter of the same handwriting where one is missing, when it is a stroke of its own. Give `after`, so that the replay writes it in its place. For part of a stroke, use `glyph` with `from` and `to`.
    Copy {
        /// The strokes copied: chosen on the note as given, copied as they are by now.
        select: Select,
        /// Where the left side of the copied strokes' box goes. Instead of `dx`.
        to_x: Option<f32>,
        /// Millimetres to the right. Instead of `to_x`.
        dx: Option<f32>,
        /// Millimetres down.
        dy: Option<f32>,
        /// Size of the copy (0.05 to 20), around the bottom left of what is copied.
        factor: Option<f32>,
        /// Colour instead of each stroke's own, `#rrggbb`.
        color: Option<String>,
        /// The stroke it comes right after in the replay (default: at the end of the note).
        after: Option<usize>,
    },
    /// A letter made of pieces of other strokes, each a new stroke (q + tail of j = g, bigger w = W, u turned = n), or one letter taken out of a word written in one stroke (`from`, `to`). Say in the account that it was made.
    Glyph {
        /// The pieces, at least one, in the order they are written.
        parts: Vec<Part>,
        /// Colour instead of each piece's own, `#rrggbb`.
        color: Option<String>,
        /// The stroke it comes right after in the replay (default: at the end of the note).
        after: Option<usize>,
    },
    /// These strokes come right after `after` in the replay, in the order they were written: a word moved into its sentence. Their old place in time stays as a pause (the replay can cut pauses). The selection must not be empty.
    Retime {
        /// The strokes.
        select: Select,
        /// The stroke they come right after. Not one of them.
        after: usize,
    },
    /// Lays out one written line: the same handwriting, better laid out. Letter shapes do not change, except for `smooth`. Give it the `strokes` of one line of `ink_lines`. The report gives what it found (strokes, words, slope, body height): check them.
    TidyLine {
        /// The strokes of one line.
        select: Select,
        /// New y of the baseline (default: where the line sits at its left end). The lines of ruled paper are in `background.ruling` of `get_note`.
        baseline: Option<f32>,
        /// New x of the start of the line (default: where it starts).
        left: Option<f32>,
        /// `words` (default) or `rigid`.
        mode: Option<TidyMode>,
        /// Size change (0.05 to 20), around the start of each word and the baseline. Ruled paper may need 0.9 when the writer's own line pitch was wider than the rules.
        scale: Option<f32>,
        /// Slope of the written line in degrees (positive goes down to the right, at most 45 either way), when you see it better than the fit. Without it, a line under 40 mm or of few letters is taken as level.
        slope: Option<f32>,
        /// `even` (default) or `keep`.
        gaps: Option<TidyGaps>,
        /// A blank wider than this many millimetres separates two words (default: 1.2 × body height, at least 2.5).
        word_gap: Option<f32>,
        /// Also smooths each stroke, as `smooth` does: 0.2 is light.
        smooth: Option<f32>,
    },
}

impl Order {
    /// The name of this order in JSON (`op`).
    pub fn op(&self) -> &'static str {
        match self {
            Order::Recolor { .. } => "recolor",
            Order::Restyle { .. } => "restyle",
            Order::Delete { .. } => "delete",
            Order::Move { .. } => "move",
            Order::Scale { .. } => "scale",
            Order::Rotate { .. } => "rotate",
            Order::Smooth { .. } => "smooth",
            Order::Trim { .. } => "trim",
            Order::Replace { .. } => "replace",
            Order::Add { .. } => "add",
            Order::Copy { .. } => "copy",
            Order::Glyph { .. } => "glyph",
            Order::Retime { .. } => "retime",
            Order::TidyLine { .. } => "tidy_line",
        }
    }
}
