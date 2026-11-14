//! The summary pages of a binder's PDF (SPEC §4.6 « Exporter un classeur en PDF », option « Ajouter une
//! page de sommaire au début »): the binder's name, then the binder's sub-binders, notes and bookmarks
//! one per row, indented by their level, each with dot leaders and its page number at the right, as in the
//! table of contents of a book. The rows go to as many pages as they need, and the numbers count those pages.
//!
//! The pages are notes of their own (a text frame per piece of a row, black on white, in the sans-serif
//! family), written by the PDF writer like any other page: the glyphs are drawn as vector paths, and the
//! invisible text layer makes the summary searchable and copyable. What the writer adds is a link on each
//! row ([`SummaryLink`]) to the page, or the spot of the page, it names.
//!
//! The sizes follow the page: the body text is a fiftieth of its short side (12 pt on A4), the margins a tenth.

use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Timing};
use ardoise_core::objects::{PageObject, TextBody};
use ardoise_ink::Rgba;
use ardoise_text::{FontFamily, LINE_HEIGHT, TextStyle, layout};

/// What a row of the summary is, which decides how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryKind {
    /// A sub-binder: bold.
    Binder,
    /// A note.
    Note,
    /// A bookmark of a note: smaller and grey.
    Bookmark,
}

/// One row of the summary: what it says, how deep it is (0 = the top level), the 0-based page of the
/// notes it goes to and, for a bookmark, the spot of that page (`[x, y]` in the note's device units from
/// its top-left corner).
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryEntry {
    pub title: String,
    pub level: usize,
    pub page: usize,
    pub at: Option<[f32; 2]>,
    pub kind: SummaryKind,
}

/// The summary of a document: its title and its rows, in reading order.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub title: String,
    pub entries: Vec<SummaryEntry>,
}

/// A link of a summary page: the box of a row on the page (`[left, top, right, bottom]`, device units,
/// from the top-left corner), and where it goes: a page of the notes (0-based) and a spot of it.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryLink {
    pub rect: [f32; 4],
    pub page: usize,
    pub at: Option<[f32; 2]>,
}

/// The summary laid out: one note per page, and the links of each page.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryPages {
    pub notes: Vec<Note>,
    pub links: Vec<Vec<SummaryLink>>,
}

/// A4 portrait in device units (10 µm).
const A4: PageSize = PageSize {
    width: 21_000,
    height: 29_700,
};
/// Body text size, as a fraction of the page's short side.
const BODY: f32 = 1.0 / 50.0;
/// Margin, as a fraction of the page's short side.
const MARGIN: f32 = 0.1;
/// Row height, in body sizes.
const PITCH: f32 = 1.7;
/// Title size, in body sizes.
const TITLE: f32 = 1.8;
/// How far each level is indented, in body sizes, and the deepest indentation (the levels below look alike).
const INDENT: f32 = 2.0;
const MAX_LEVEL: usize = 6;
/// Space kept between a row's title and the numbers, in sizes of the row.
const LEADER_MIN: f32 = 3.0;
/// Space between the leaders and what they join, in sizes of the row.
const LEADER_GAP: f32 = 0.5;
/// A bookmark's text size, relative to the body's.
const SMALL: f32 = 0.85;
const GREY: [u8; 3] = [70, 70, 70];

/// The size of the pages: A4 portrait, or `page` when the notes keep their own size.
pub fn page_size(a4: bool, first: PageSize) -> PageSize {
    if a4 || first.width == 0 || first.height == 0 {
        A4
    } else {
        first
    }
}

/// How a row's pieces look.
#[derive(Clone, Copy)]
struct Look {
    size: f32,
    bold: bool,
    rgb: [u8; 3],
}

impl Look {
    fn style(self) -> TextStyle {
        TextStyle {
            family: FontFamily::Andika,
            bold: self.bold,
            size: self.size,
        }
    }

    fn width(self, text: &str) -> f32 {
        layout(text, self.style()).width
    }

    /// `text` shortened, with an ellipsis, to fit `room`.
    fn fitted(self, text: &str, room: f32) -> String {
        if self.width(text) <= room {
            return text.to_owned();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let candidate: String = chars.iter().collect::<String>().trim_end().to_owned() + "…";
            if self.width(&candidate) <= room {
                return candidate;
            }
        }
        String::from("…")
    }
}

/// How many rows fit on the first page (below the title) and on each of the others.
fn rows_per_page(height: f32, margin: f32, pitch: f32, title_block: f32) -> (usize, usize) {
    let rows = |room: f32| ((room / pitch).floor().max(1.0)) as usize;
    (
        rows(height - 2.0 * margin - title_block),
        rows(height - 2.0 * margin),
    )
}

/// Lays `summary` out on pages of `size`. No entry: no page. The numbers shown are those of the pages of
/// the notes counted after the summary's own pages.
pub fn lay_out(summary: &Summary, size: PageSize) -> SummaryPages {
    if summary.entries.is_empty() {
        return SummaryPages {
            notes: Vec::new(),
            links: Vec::new(),
        };
    }
    let (w, h) = (size.width as f32, size.height as f32);
    let short = w.min(h);
    let margin = short * MARGIN;
    let body = short * BODY;
    let pitch = body * PITCH;
    let title_look = Look {
        size: body * TITLE,
        bold: true,
        rgb: [0, 0, 0],
    };
    let title_block = title_look.size * LINE_HEIGHT + body * 1.5;
    let (first_rows, other_rows) = rows_per_page(h, margin, pitch, title_block);

    // How many pages the rows take: the numbers depend on it, the layout of a row does not.
    let rest = summary.entries.len().saturating_sub(first_rows);
    let pages = 1 + rest.div_ceil(other_rows);

    let mut notes = Vec::with_capacity(pages);
    let mut links = Vec::with_capacity(pages);
    let mut entries = summary.entries.iter().peekable();
    for page in 0..pages {
        let mut objects: Vec<PageObject> = Vec::new();
        let mut page_links = Vec::new();
        let mut top = margin;
        let rows = if page == 0 {
            let title = title_look.fitted(&summary.title, w - 2.0 * margin);
            objects.push(frame(&objects, &title, margin, top, title_look));
            top += title_block;
            first_rows
        } else {
            other_rows
        };
        for _ in 0..rows {
            let Some(entry) = entries.next() else {
                break;
            };
            row(
                entry,
                pages,
                [w, margin, body, pitch],
                top,
                &mut objects,
                &mut page_links,
            );
            top += pitch;
        }
        notes.push(Note {
            id: format!("summary-{}", page + 1),
            title: summary.title.clone(),
            created: 0,
            modified: 0,
            page: size,
            background: Background::default(),
            background_image: None,
            tags: Vec::new(),
            favorite: false,
            layers: vec![Layer {
                name: String::new(),
                visible: true,
            }],
            strokes: Vec::new(),
            objects,
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        });
        links.push(page_links);
    }
    SummaryPages { notes, links }
}

/// A text frame whose line box starts at `(x, y)`.
fn frame(done: &[PageObject], text: &str, x: f32, y: f32, look: Look) -> PageObject {
    PageObject::text(
        format!("s{}", done.len()),
        0,
        0,
        x,
        y,
        TextBody {
            text: text.to_owned(),
            font: FontFamily::Andika,
            bold: look.bold,
            size: look.size,
            color: Rgba::rgb(look.rgb[0], look.rgb[1], look.rgb[2]),
        },
    )
}

/// One row: the title, the leaders and the number, and the link over it. `[w, margin, body, pitch]` are the
/// page's width and the sizes the page gives; `top` is where the row starts; `pages` the pages of the summary.
fn row(
    entry: &SummaryEntry,
    pages: usize,
    [w, margin, body, pitch]: [f32; 4],
    top: f32,
    objects: &mut Vec<PageObject>,
    links: &mut Vec<SummaryLink>,
) {
    let look = match entry.kind {
        SummaryKind::Binder => Look {
            size: body,
            bold: true,
            rgb: [0, 0, 0],
        },
        SummaryKind::Note => Look {
            size: body,
            bold: false,
            rgb: [0, 0, 0],
        },
        SummaryKind::Bookmark => Look {
            size: body * SMALL,
            bold: false,
            rgb: GREY,
        },
    };
    // The text's box is centred in the row.
    let y = top + (pitch - look.size * LINE_HEIGHT) / 2.0;
    let x0 = margin + entry.level.min(MAX_LEVEL) as f32 * INDENT * body;
    let number = (entry.page + 1 + pages).to_string();
    let right = w - margin;
    let number_x = right - look.width(&number);
    let room = (number_x - x0 - LEADER_MIN * look.size).max(4.0 * look.size);
    let title = look.fitted(&entry.title, room);
    objects.push(frame(objects, &title, x0, y, look));

    // The dots sit on a grid shared by every row of the page, so that they line up like a book's.
    let step = look.width(". ");
    if step > 0.0 {
        let from = x0 + look.width(&title) + LEADER_GAP * look.size;
        let first = margin + ((from - margin) / step).ceil() * step;
        let count = ((number_x - LEADER_GAP * look.size - first) / step).floor();
        if count >= 1.0 {
            let dots = ". ".repeat(count as usize);
            objects.push(frame(objects, dots.trim_end(), first, y, look));
        }
    }
    objects.push(frame(objects, &number, number_x, y, look));
    links.push(SummaryLink {
        rect: [margin, top, right, top + pitch],
        page: entry.page,
        at: entry.at,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, level: usize, page: usize, kind: SummaryKind) -> SummaryEntry {
        SummaryEntry {
            title: title.to_owned(),
            level,
            page,
            at: None,
            kind,
        }
    }

    fn texts(note: &Note) -> Vec<String> {
        note.objects
            .iter()
            .filter_map(|object| object.as_text().map(|body| body.text.clone()))
            .collect()
    }

    #[test]
    fn a_short_summary_is_one_page_with_a_title_and_a_row_per_entry() {
        let summary = Summary {
            title: "Cours".to_owned(),
            entries: vec![
                entry("Chapitre 1", 0, 0, SummaryKind::Note),
                entry("Loi d'Ohm", 1, 0, SummaryKind::Bookmark),
                entry("Chapitre 2", 0, 1, SummaryKind::Note),
            ],
        };
        let pages = lay_out(&summary, A4);
        assert_eq!((pages.notes.len(), pages.links.len()), (1, 1));
        let texts = texts(&pages.notes[0]);
        assert_eq!(texts[0], "Cours");
        // One summary page: the notes start at page 2.
        assert!(texts.contains(&"Chapitre 1".to_owned()));
        assert!(texts.contains(&"2".to_owned()) && texts.contains(&"3".to_owned()));
        assert!(texts.iter().any(|t| t.starts_with(". .")), "dot leaders");
        // A link per row, over the row, to the page of the notes it names.
        assert_eq!(pages.links[0].len(), 3);
        assert_eq!(
            pages.links[0].iter().map(|l| l.page).collect::<Vec<_>>(),
            [0, 0, 1]
        );
        let [left, top, right, bottom] = pages.links[0][0].rect;
        assert!(left < right && top < bottom && bottom <= 29_700.0);
        // Every frame stays inside the page.
        for object in &pages.notes[0].objects {
            let [x0, y0, x1, y1] = object.page_bounds();
            assert!(x0 >= 0.0 && y0 >= 0.0 && x1 <= 21_000.0 && y1 <= 29_700.0, "{object:?}");
        }
    }

    #[test]
    fn a_long_summary_takes_several_pages_and_its_numbers_count_them() {
        let entries: Vec<SummaryEntry> = (0..100)
            .map(|i| entry(&format!("Note {i}"), 0, i, SummaryKind::Note))
            .collect();
        let pages = lay_out(
            &Summary {
                title: "Long".to_owned(),
                entries,
            },
            A4,
        );
        assert!(pages.notes.len() >= 3, "{}", pages.notes.len());
        let rows: usize = pages.links.iter().map(Vec::len).sum();
        assert_eq!(rows, 100);
        // The first note is on page (summary pages + 1): that is what its row says.
        let first = texts(&pages.notes[0]);
        assert!(first.contains(&(1 + pages.notes.len()).to_string()));
        // The last row says (summary pages + 100).
        let last = texts(pages.notes.last().unwrap());
        assert!(last.contains(&(100 + pages.notes.len()).to_string()));
        // The title is on the first page only.
        assert!(first.contains(&"Long".to_owned()));
        assert!(!last.contains(&"Long".to_owned()));
    }

    #[test]
    fn a_title_too_long_for_its_row_is_cut_with_an_ellipsis() {
        let long = "Un titre de note vraiment très long qui ne tiendrait jamais sur une seule ligne de la page ".repeat(3);
        let pages = lay_out(
            &Summary {
                title: long.clone(),
                entries: vec![entry(&long, 5, 0, SummaryKind::Note)],
            },
            A4,
        );
        let texts = texts(&pages.notes[0]);
        assert!(texts[0].ends_with('…') && texts[0].len() < long.len());
        assert!(texts[1].ends_with('…'));
        for object in &pages.notes[0].objects {
            let [_, _, x1, _] = object.page_bounds();
            assert!(x1 <= 21_000.0);
        }
    }

    #[test]
    fn no_entry_no_page_and_the_page_size_is_a4_or_the_first_notes() {
        let none = lay_out(
            &Summary {
                title: "x".to_owned(),
                entries: Vec::new(),
            },
            A4,
        );
        assert!(none.notes.is_empty());
        let slate = PageSize {
            width: 29_700,
            height: 21_600,
        };
        assert_eq!(page_size(true, slate), A4);
        assert_eq!(page_size(false, slate), slate);
        assert_eq!(
            page_size(
                false,
                PageSize {
                    width: 0,
                    height: 0
                }
            ),
            A4
        );
    }
}
