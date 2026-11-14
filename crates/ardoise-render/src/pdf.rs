//! PDF export (SPEC §4.6): one page per note, strokes as vector paths filled with the
//! non-zero rule ([`crate::look`]), opacity and blending through ExtGState (`/ca`, `/BM`), the
//! background as for SVG (paper, template shapes, texture as a JPEG image) and the background
//! image as an image XObject: a JPEG passed through as it is (`/DCTDecode`), other pixels
//! deflated, with a soft mask (`/SMask`) for their transparency.
//!
//! Page sizes: `Actual` is the note's own size (1 device unit = 72 / 2540 pt); `A4` is
//! 210 × 297 mm in the note's orientation, the note scaled to fit and centred (the margins
//! stay white). Pages larger than PDF's 14 400 units a side get a `/UserUnit`.
//!
//! Layers: merged (the default) paints every fill in one stream; separate puts each note
//! layer's fills in marked content tied to an optional content group (PDF 1.5), one group per
//! layer name over the whole document, so that a viewer can show or hide it.
//!
//! Outline (bookmarks): [`notes_to_pdf_with_outline`] writes the document outline of the viewer's
//! bookmarks panel from a flat list of [`OutlineEntry`] (title, depth, page, and the spot of the page
//! when there is one), all entries open. The bookmarks of a note (SPEC §4.4 « Signets ») are entries
//! with a spot: [`bookmark_outline`].
//!
//! Summary: [`notes_to_pdf_with_summary`] puts the pages of a [`Summary`] (the binder's name, then each note and
//! bookmark with its page number, [`crate::summary`]) before the notes: the pages are written like the notes'
//! own, with a link over each row to the page (or the spot of the page) it names, and the outline starts
//! with an entry for the summary.
//!
//! Text frames: each layer's frames are painted over its strokes as filled paths, the glyph
//! outlines with their curves (quadratic curves written as the cubic curves they are), so the
//! page stays vector and looks the same in every viewer, with no font to embed. So that the
//! words can still be searched, selected and copied, each line of a frame is also written as
//! invisible text (rendering mode 3, as OCR layers are) over its glyphs: a standard font
//! (Times for Latin Modern, Helvetica for Andika; never drawn, so never embedded) in
//! WinAnsiEncoding, given the advance widths of our own glyphs (`/Widths`), so that each
//! character falls on the glyph it stands for; a tab or a character left out is a positioning
//! step of the `TJ` array. Characters outside WinAnsiEncoding (Greek, mathematical symbols)
//! are left out of that layer: they are drawn, not searchable.

use std::collections::HashMap;

use ardoise_core::note::Note;
use ardoise_text::{FontFamily, TextStyle, layout};
use pdf_writer::types::{ActionType, AnnotationType, BlendMode, PageMode, TextRenderingMode};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

use crate::background::{Shape, VectorBackground};
use crate::look::{Blend, UNITS_PER_INCH};
use crate::picture::{DctColor, NoteView, PlacedPicture};
use crate::summary::{self, Summary};
use crate::svg::TEXTURE_QUALITY;
use crate::text::TextFill;
use crate::vector::{LayerFills, Scene};
use crate::{LayerMode, RenderError};

/// Points per device unit.
const PT_PER_UNIT: f32 = 72.0 / UNITS_PER_INCH;
/// A4 portrait in points (210 × 297 mm).
const A4_PT: (f32, f32) = (210.0 / 25.4 * 72.0, 297.0 / 25.4 * 72.0);
/// Largest page side in user space units (PDF 32000-1, annex C.2).
const MAX_PAGE_SIDE: f32 = 14_400.0;
/// Bézier handle length of a quarter circle, relative to the radius.
const KAPPA: f32 = 0.552_284_8;
/// zlib level of the content streams.
const DEFLATE_LEVEL: u8 = 6;

/// Page size of a PDF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PdfPage {
    /// The note's own size.
    #[default]
    Actual,
    /// 210 × 297 mm in the note's orientation, the note fitted and centred.
    A4,
}

/// How to write notes as PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfOptions {
    pub page: PdfPage,
    /// Draw the paper colour, template and texture; otherwise the page stays white.
    pub background: bool,
    /// One optional content group per note layer, or one drawing.
    pub layers: LayerMode,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            page: PdfPage::Actual,
            background: true,
            layers: LayerMode::Merged,
        }
    }
}

/// One bookmark of a PDF's outline: a title, how deep it is (0 = top level; an entry one level
/// deeper than the one before it is its child), the 0-based page it goes to, and the spot of that
/// page it scrolls to (`[x, y]` in the note's device units from its top-left corner; `None`: the
/// page as a whole).
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
    pub title: String,
    pub level: usize,
    pub page: usize,
    pub at: Option<[f32; 2]>,
}

/// How far above a bookmark's box the viewer's window starts when it goes there, device units (5 mm).
const BOOKMARK_LEAD: f32 = 500.0;

/// The outline entries of `note`'s bookmarks, in reading order, all at `level`, going to page `page`
/// at the spot of each (a little above it). Empty without a bookmark.
pub fn bookmark_outline(note: &Note, page: usize, level: usize) -> Vec<OutlineEntry> {
    note.bookmark_objects()
        .into_iter()
        .map(|(object, body)| {
            let [left, top, _, _] = object.page_bounds();
            OutlineEntry {
                title: body.label.trim().to_owned(),
                level,
                page,
                at: Some([left.max(0.0), (top - BOOKMARK_LEAD).max(0.0)]),
            }
        })
        .collect()
}

/// A PDF document with one page per note, in order (`&Note`s, or [`NoteView`]s for notes
/// with a background image).
pub fn notes_to_pdf<'a, V>(notes: &[V], options: &PdfOptions) -> Result<Vec<u8>, RenderError>
where
    V: Into<NoteView<'a>> + Copy,
{
    notes_to_pdf_with_outline(notes, options, &[])
}

/// [`notes_to_pdf`] with an outline (the viewer's bookmarks): `outline` in reading order, each
/// entry's `level` at most one more than the entry before it (a deeper one is brought up to
/// that), the first at level 0. An entry whose page does not exist is left out. With no entry
/// the document has no outline.
pub fn notes_to_pdf_with_outline<'a, V>(
    notes: &[V],
    options: &PdfOptions,
    outline: &[OutlineEntry],
) -> Result<Vec<u8>, RenderError>
where
    V: Into<NoteView<'a>> + Copy,
{
    notes_to_pdf_with_summary(notes, options, outline, None)
}

/// A link of a page, in the page's user space: the box, the page it goes to and the spot of it.
struct Link {
    rect: Rect,
    target: usize,
    spot: Option<[f32; 2]>,
}

/// [`notes_to_pdf_with_outline`] with the pages of a summary first, when `summary` is given with the title of
/// its outline entry. `outline` stays the one of the notes (its pages count from the first note): the pages of
/// the summary are put in front of it, and the entry of the summary before its entries. The summary's pages are
/// A4 portrait with [`PdfPage::A4`], else the size of the first note.
pub fn notes_to_pdf_with_summary<'a, V>(
    notes: &[V],
    options: &PdfOptions,
    outline: &[OutlineEntry],
    summary: Option<(&Summary, &str)>,
) -> Result<Vec<u8>, RenderError>
where
    V: Into<NoteView<'a>> + Copy,
{
    if notes.is_empty() {
        return Err(RenderError::Encode("no note to put in the PDF".into()));
    }
    let views: Vec<NoteView<'a>> = notes.iter().map(|&view| view.into()).collect();
    let front = summary.map(|(summary, _)| {
        let size = summary::page_size(options.page == PdfPage::A4, views[0].note.page);
        summary::lay_out(summary, size)
    });
    let front_count = front.as_ref().map_or(0, |front| front.notes.len());
    let all: Vec<NoteView<'_>> = front
        .iter()
        .flat_map(|front| front.notes.iter())
        .map(|note| NoteView::new(note, None))
        .chain(views.iter().copied())
        .collect();
    let count = i32::try_from(all.len())
        .map_err(|_| RenderError::Encode("too many notes for one PDF".into()))?;
    // The outline of the whole document: the summary's entry, then the notes' pushed back by its pages.
    let mut shifted: Vec<OutlineEntry> = Vec::new();
    if let (Some((_, title)), true) = (summary, front_count > 0) {
        shifted.push(OutlineEntry {
            title: title.to_owned(),
            level: 0,
            page: 0,
            at: None,
        });
    }
    shifted.extend(outline.iter().map(|entry| OutlineEntry {
        page: entry.page + front_count,
        ..entry.clone()
    }));
    let outline = &shifted[..];

    let mut pdf = Pdf::new();
    let mut next = Ref::new(1);
    let catalog = next.bump();
    let tree = next.bump();
    let info = next.bump();
    let pages: Vec<Ref> = all.iter().map(|_| next.bump()).collect();
    let bookmarks = Bookmarks::of(outline, all.len(), &mut next);
    let places: Vec<Placement> = all
        .iter()
        .map(|view| {
            let page = view.note.page;
            Placement::of(page.width as f32, page.height as f32, options.page)
        })
        .collect();
    // The links of each summary page, in the page's user space.
    let links: Vec<Vec<Link>> = front.as_ref().map_or_else(Vec::new, |front| {
        front
            .links
            .iter()
            .enumerate()
            .map(|(index, page_links)| {
                let place = places[index];
                page_links
                    .iter()
                    .map(|link| {
                        let [left, top, right, bottom] = link.rect;
                        let x = |v: f32| round(place.left + v * place.scale);
                        let y = |v: f32| round(place.height - (place.top + v * place.scale));
                        let target = link.page + front_count;
                        let spot = link.at.zip(places.get(target)).map(|([x, y], target)| {
                            [
                                round(target.left + x * target.scale),
                                round(target.height - (target.top + y * target.scale)),
                            ]
                        });
                        Link {
                            rect: Rect::new(x(left), y(bottom), x(right), y(top)),
                            target,
                            spot,
                        }
                    })
                    .collect()
            })
            .collect()
    });
    let separate = options.layers == LayerMode::Separate;
    if !separate {
        let mut root = pdf.catalog(catalog);
        root.pages(tree);
        if let Some(bookmarks) = &bookmarks {
            root.outlines(bookmarks.root);
            root.page_mode(PageMode::UseOutlines);
        }
        root.finish();
    }
    if let Some(bookmarks) = &bookmarks {
        bookmarks.write(&mut pdf, &pages, &places);
    }
    pdf.pages(tree).kids(pages.iter().copied()).count(count);
    let mut groups = Groups::default();
    let mut fonts = TextFonts::default();
    for (index, (&view, &page)) in all.iter().zip(&pages).enumerate() {
        let groups = separate.then_some(&mut groups);
        let ids = Ids {
            tree,
            page,
            next: &mut next,
        };
        let page_links = links.get(index).map_or(&[][..], Vec::as_slice);
        write_page(
            &mut pdf, ids, view, options, groups, &mut fonts, page_links, &pages,
        )?;
    }
    write_fonts(&mut pdf, &fonts);
    if separate {
        // The groups are known once every page is written, so the catalog comes last.
        write_groups(&mut pdf, &groups);
        let mut root = pdf.catalog(catalog);
        root.pages(tree);
        if let Some(bookmarks) = &bookmarks {
            root.outlines(bookmarks.root);
        }
        if !groups.ids.is_empty() {
            root.page_mode(PageMode::UseOC);
            let mut properties = root.insert(Name(b"OCProperties")).dict();
            properties
                .insert(Name(b"OCGs"))
                .array()
                .items(groups.ids.iter().copied());
            let mut config = properties.insert(Name(b"D")).dict();
            config.pair(Name(b"BaseState"), Name(b"ON"));
            config
                .insert(Name(b"Order"))
                .array()
                .items(groups.ids.iter().copied());
            config.finish();
            properties.finish();
        }
        root.finish();
    }
    let mut document = pdf.document_info(info);
    document.producer(TextStr("Ardoise"));
    if let (Some((summary, _)), true) = (summary, front_count > 0) {
        document.title(TextStr(summary.title.trim()));
    } else if let [view] = &views[..] {
        let title = view.note.title.trim();
        if !title.is_empty() {
            document.title(TextStr(title));
        }
    }
    document.finish();
    Ok(pdf.finish())
}

/// The outline of a document, laid out as the tree the PDF wants (parents, siblings, children).
struct Bookmarks {
    root: Ref,
    /// Per entry, in order: its object, its title, its page, its parent (an entry's index; `None`
    /// for the top level), and its children's indexes.
    items: Vec<Bookmark>,
    top: Vec<usize>,
}

struct Bookmark {
    id: Ref,
    title: String,
    page: usize,
    at: Option<[f32; 2]>,
    parent: Option<usize>,
    children: Vec<usize>,
}

impl Bookmarks {
    /// The tree of `outline` over `pages` pages, with its object numbers taken from `next`;
    /// `None` when there is no entry to show.
    fn of(outline: &[OutlineEntry], pages: usize, next: &mut Ref) -> Option<Bookmarks> {
        let mut items: Vec<Bookmark> = Vec::new();
        let mut top = Vec::new();
        // The entries of the branch being built, from the top level down to the last one.
        let mut path: Vec<usize> = Vec::new();
        for entry in outline.iter().filter(|entry| entry.page < pages) {
            let depth = entry.level.min(path.len());
            path.truncate(depth);
            let parent = path.last().copied();
            let index = items.len();
            items.push(Bookmark {
                id: next.bump(),
                title: entry.title.clone(),
                page: entry.page,
                at: entry.at,
                parent,
                children: Vec::new(),
            });
            match parent {
                Some(parent) => items[parent].children.push(index),
                None => top.push(index),
            }
            path.push(index);
        }
        (!items.is_empty()).then(|| Bookmarks {
            root: next.bump(),
            items,
            top,
        })
    }

    /// How many entries lie below `index`, at any depth (every entry is open).
    fn below(&self, index: usize) -> i32 {
        self.items[index]
            .children
            .iter()
            .map(|child| 1 + self.below(*child))
            .sum()
    }

    fn write(&self, pdf: &mut Pdf, pages: &[Ref], places: &[Placement]) {
        let (first, last) = (self.top[0], self.top[self.top.len() - 1]);
        let mut root = pdf.outline(self.root);
        root.first(self.items[first].id)
            .last(self.items[last].id)
            .count(i32::try_from(self.items.len()).unwrap_or(i32::MAX));
        root.finish();
        for (index, item) in self.items.iter().enumerate() {
            let siblings = match item.parent {
                Some(parent) => &self.items[parent].children,
                None => &self.top,
            };
            let at = siblings.iter().position(|s| *s == index).unwrap_or(0);
            let mut dict = pdf.outline_item(item.id);
            dict.title(TextStr(&item.title));
            dict.parent(item.parent.map_or(self.root, |p| self.items[p].id));
            if at > 0 {
                dict.prev(self.items[siblings[at - 1]].id);
            }
            if let Some(next) = siblings.get(at + 1) {
                dict.next(self.items[*next].id);
            }
            if let (Some(first), Some(last)) = (item.children.first(), item.children.last()) {
                dict.first(self.items[*first].id)
                    .last(self.items[*last].id)
                    .count(self.below(index));
            }
            match (item.at, places.get(item.page)) {
                (Some([x, y]), Some(place)) => {
                    // The spot as the content stream's transform puts it, in the page's user space
                    // (y up).
                    let left = place.left + x * place.scale;
                    let top = place.height - (place.top + y * place.scale);
                    dict.dest()
                        .page(pages[item.page])
                        .xyz(round(left), round(top), None);
                }
                _ => {
                    dict.dest().page(pages[item.page]).fit();
                }
            }
            dict.finish();
        }
    }
}

/// The optional content groups of a document: one per distinct layer name, over all its pages,
/// in order of first appearance.
#[derive(Default)]
struct Groups {
    names: Vec<String>,
    ids: Vec<Ref>,
}

impl Groups {
    /// The number of the group named `name`, made when there is none yet.
    fn number(&mut self, name: &str, next: &mut Ref) -> usize {
        if let Some(k) = self.names.iter().position(|n| n == name) {
            return k;
        }
        self.names.push(name.to_owned());
        self.ids.push(next.bump());
        self.names.len() - 1
    }
}

/// The name a layer's group shows in a viewer's layers panel: the layer's name (its number when
/// the name is blank). `None` for strokes whose layer has no entry: they stay outside the groups.
fn group_name(layer: &LayerFills) -> Option<String> {
    let name = layer.name.as_deref()?;
    Some(if name.trim().is_empty() {
        (layer.index + 1).to_string()
    } else {
        name.to_owned()
    })
}

fn write_groups(pdf: &mut Pdf, groups: &Groups) {
    for (name, &id) in groups.names.iter().zip(&groups.ids) {
        let mut group = pdf.indirect(id).dict();
        group.pair(Name(b"Type"), Name(b"OCG"));
        group.pair(Name(b"Name"), TextStr(name));
        group.finish();
    }
}

/// First character code of the invisible text's fonts (the space): the codes below are control
/// codes, never written.
const FIRST_CODE: u8 = 32;
/// Smallest positioning step written in a `TJ` array, in thousandths of the text's size: below
/// it the characters stay where the widths put them (the error never builds up).
const MIN_ADJUSTMENT: f32 = 0.5;

/// The fonts of the invisible text of a document: one per family and weight used, over all its
/// pages, with the advance widths of our glyphs.
#[derive(Default)]
struct TextFonts {
    keys: Vec<(FontFamily, bool)>,
    ids: Vec<Ref>,
    /// Per font: the advance of each code from [`FIRST_CODE`] to 255, in thousandths of the size.
    widths: Vec<Vec<f32>>,
}

impl TextFonts {
    /// The number of the font of `family` and `bold`, made when there is none yet.
    fn number(&mut self, family: FontFamily, bold: bool, next: &mut Ref) -> usize {
        if let Some(k) = self.keys.iter().position(|&key| key == (family, bold)) {
            return k;
        }
        self.keys.push((family, bold));
        self.ids.push(next.bump());
        self.widths.push(code_widths(family, bold));
        self.keys.len() - 1
    }
}

/// The name of font `k` in the page resources.
fn font_name(k: usize) -> String {
    format!("T{k}")
}

/// The advance of each WinAnsiEncoding code from [`FIRST_CODE`] to 255 in our glyphs of
/// `family` (a character it lacks taken from the other family, as the layout does), in
/// thousandths of the size; 0 for the codes the encoding leaves unused.
fn code_widths(family: FontFamily, bold: bool) -> Vec<f32> {
    let style = TextStyle {
        family,
        bold,
        size: 1000.0,
    };
    (FIRST_CODE..=255)
        .map(|code| win_ansi_char(code).map_or(0.0, |c| layout(&c.to_string(), style).width))
        .collect()
}

/// The standard font a family's invisible text names (never drawn: only its name shows, in a
/// viewer's list of the document's fonts).
fn base_font(family: FontFamily, bold: bool) -> &'static [u8] {
    match (family, bold) {
        (FontFamily::LatinModern, false) => b"Times-Roman",
        (FontFamily::LatinModern, true) => b"Times-Bold",
        (FontFamily::Andika, false) => b"Helvetica",
        (FontFamily::Andika, true) => b"Helvetica-Bold",
    }
}

fn write_fonts(pdf: &mut Pdf, fonts: &TextFonts) {
    for ((&(family, bold), &id), widths) in fonts.keys.iter().zip(&fonts.ids).zip(&fonts.widths) {
        let mut font = pdf.type1_font(id);
        font.base_font(Name(base_font(family, bold)));
        font.encoding_predefined(Name(b"WinAnsiEncoding"));
        font.first_char(FIRST_CODE);
        font.last_char(255);
        font.widths(widths.iter().copied());
        font.finish();
    }
}

/// The characters of the codes 0x80 to 0x9F of WinAnsiEncoding (PDF 32000-1, annex D.2; the
/// codes of Windows code page 1252), `'\0'` for the five it leaves unused. The other codes from
/// 0x20 are those of Unicode (Latin-1): 0x20 to 0x7E and 0xA0 to 0xFF.
const WIN_ANSI_80: [char; 32] = [
    '\u{20ac}', '\0', '\u{201a}', '\u{0192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02c6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\0', '\u{017d}', '\0', '\0',
    '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{02dc}',
    '\u{2122}', '\u{0161}', '\u{203a}', '\u{0153}', '\0', '\u{017e}', '\u{0178}',
];

/// The WinAnsiEncoding code of `c`; `None` for a character the encoding lacks.
fn win_ansi(c: char) -> Option<u8> {
    match u32::from(c) {
        code @ (0x20..=0x7e | 0xa0..=0xff) => u8::try_from(code).ok(),
        _ if c == '\0' => None,
        _ => WIN_ANSI_80
            .iter()
            .position(|&high| high == c)
            .and_then(|i| u8::try_from(0x80 + i).ok()),
    }
}

/// The character of WinAnsiEncoding code `code`; `None` for a control code or an unused one.
fn win_ansi_char(code: u8) -> Option<char> {
    match code {
        0x20..=0x7e | 0xa0..=0xff => Some(char::from(code)),
        0x80..=0x9f => WIN_ANSI_80
            .get(usize::from(code - 0x80))
            .copied()
            .filter(|&c| c != '\0'),
        _ => None,
    }
}

/// A piece of a `TJ` array.
#[derive(Debug, Clone, PartialEq)]
enum Shown {
    Bytes(Vec<u8>),
    /// Thousandths of the size, subtracted from the position (negative moves on).
    Adjust(f32),
}

/// Writes the lines of `text` as invisible text in font `font` (whose code widths are
/// `widths`), each over its glyphs: one text object per line, so that extractors end each line.
fn invisible_text(content: &mut Content, text: &TextFill, font: usize, widths: &[f32]) {
    let size = text.size();
    let Some(layout) = text.layout.as_ref() else {
        return;
    };
    if !(size.is_finite() && size > 0.0) {
        return;
    }
    // The character at each UTF-16 offset of the text (the glyphs' `index`).
    let mut chars: Vec<Option<char>> = Vec::with_capacity(text.text.len());
    for c in text.text.chars() {
        chars.push(Some(c));
        chars.extend(std::iter::repeat_n(None, c.len_utf16() - 1));
    }
    let name = font_name(font);
    let [a, b, c, d, _, _] = text.placement;
    for line in &layout.lines {
        let mut shown: Vec<Shown> = Vec::new();
        // Where the first character shown starts on the line, and where the next one will go.
        let mut start: Option<f32> = None;
        let mut pen = 0.0_f32;
        for glyph in &line.glyphs {
            let Some(code) = chars.get(glyph.index).copied().flatten().and_then(win_ansi) else {
                continue;
            };
            if start.is_none() {
                start = Some(glyph.x);
                pen = glyph.x;
            }
            let adjust = -(glyph.x - pen) / size * 1000.0;
            if adjust.abs() >= MIN_ADJUSTMENT {
                shown.push(Shown::Adjust(adjust));
                pen = glyph.x;
            }
            match shown.last_mut() {
                Some(Shown::Bytes(bytes)) => bytes.push(code),
                _ => shown.push(Shown::Bytes(vec![code])),
            }
            let width = widths
                .get(usize::from(code.saturating_sub(FIRST_CODE)))
                .copied()
                .unwrap_or(0.0);
            pen += width * size / 1000.0;
        }
        let Some(x0) = start else {
            continue;
        };
        // Text space: x along the line, y up the glyphs (the page is y down), one unit = the size.
        let [e, f] = text.page_point([x0, line.baseline]);
        content.begin_text();
        content.set_font(Name(name.as_bytes()), 1.0);
        content.set_text_rendering_mode(TextRenderingMode::Invisible);
        content.set_text_matrix([a * size, b * size, -c * size, -d * size, round(e), round(f)]);
        {
            let mut positioned = content.show_positioned();
            let mut items = positioned.items();
            for piece in &shown {
                match piece {
                    Shown::Bytes(bytes) => {
                        items.show(Str(bytes));
                    }
                    Shown::Adjust(amount) => {
                        items.adjust(*amount);
                    }
                }
            }
        }
        content.end_text();
    }
}

/// Where the note goes on its page.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    /// Page size in user space units.
    width: f32,
    height: f32,
    /// Size of a user space unit in points (`/UserUnit`), 1 normally.
    user_unit: f32,
    /// User space units per device unit.
    scale: f32,
    /// Offset of the note's top-left corner from the page's top-left corner.
    left: f32,
    top: f32,
}

impl Placement {
    fn of(width: f32, height: f32, page: PdfPage) -> Placement {
        let (page_w, page_h, scale) = match page {
            PdfPage::Actual => (width * PT_PER_UNIT, height * PT_PER_UNIT, PT_PER_UNIT),
            PdfPage::A4 => {
                let (w, h) = if width > height {
                    (A4_PT.1, A4_PT.0)
                } else {
                    A4_PT
                };
                (w, h, (w / width).min(h / height))
            }
        };
        let user_unit = (page_w.max(page_h) / MAX_PAGE_SIDE).ceil().max(1.0);
        let (page_w, page_h, scale) = (page_w / user_unit, page_h / user_unit, scale / user_unit);
        Placement {
            width: page_w,
            height: page_h,
            user_unit,
            scale,
            left: (page_w - width * scale) / 2.0,
            top: (page_h - height * scale) / 2.0,
        }
    }
}

/// Rounds page coordinates to 0.01 device unit (0.1 µm): shorter numbers, same drawing.
fn round(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

/// The content stream of one page, keeping track of the graphics state it set.
struct Painter {
    content: Content,
    /// ExtGState names by (opacity bits, blend).
    states: HashMap<(u32, Blend), usize>,
    state: (u32, Blend),
    rgb: Option<[u8; 3]>,
}

impl Painter {
    fn new() -> Self {
        Self {
            content: Content::new(),
            states: HashMap::new(),
            state: (1.0f32.to_bits(), Blend::Normal),
            rgb: None,
        }
    }

    /// Sets the fill colour, opacity and blending for the next fill.
    fn style(&mut self, rgb: [u8; 3], opacity: f32, blend: Blend) {
        self.alpha(opacity, blend);
        if self.rgb != Some(rgb) {
            let [r, g, b] = rgb.map(|c| f32::from(c) / 255.0);
            self.content.set_fill_rgb(r, g, b);
            self.rgb = Some(rgb);
        }
    }

    /// Sets the opacity and blending (of fills and images) for what is painted next.
    fn alpha(&mut self, opacity: f32, blend: Blend) {
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let key = (opacity.to_bits(), blend);
        if key != self.state {
            let next = self.states.len();
            let index = *self.states.entry(key).or_insert(next);
            self.content
                .set_parameters(Name(format!("G{index}").as_bytes()));
            self.state = key;
        }
    }

    /// The background image (the XObject `/Im1`), with its opacity, at its place. The page is
    /// already clipped to the note.
    fn picture(&mut self, placed: &PlacedPicture<'_>) {
        // Outside the saved state, so that the state this painter tracks stays the real one.
        self.alpha(placed.opacity, Blend::Normal);
        let (w, h) = (
            placed.picture.width() as f32,
            placed.picture.height() as f32,
        );
        let [a, b, c, d, e, f] = placed.matrix;
        // An image fills the unit square with its first row at the top: (s, t) is the pixel
        // (s w, (1 - t) h), which the image's own map then puts on the page.
        self.content.save_state();
        self.content
            .transform([a * w, b * w, -c * h, -d * h, c * h + e, d * h + f]);
        self.content.x_object(Name(b"Im1"));
        self.content.restore_state();
    }

    fn shape(&mut self, shape: Shape) {
        match shape {
            Shape::Rect {
                x,
                y,
                width,
                height,
            } => {
                self.content
                    .rect(round(x), round(y), round(width), round(height));
            }
            Shape::Disc { cx, cy, r } => {
                let k = KAPPA * r;
                let c = &mut self.content;
                c.move_to(round(cx + r), round(cy));
                c.cubic_to(
                    round(cx + r),
                    round(cy + k),
                    round(cx + k),
                    round(cy + r),
                    round(cx),
                    round(cy + r),
                );
                c.cubic_to(
                    round(cx - k),
                    round(cy + r),
                    round(cx - r),
                    round(cy + k),
                    round(cx - r),
                    round(cy),
                );
                c.cubic_to(
                    round(cx - r),
                    round(cy - k),
                    round(cx - k),
                    round(cy - r),
                    round(cx),
                    round(cy - r),
                );
                c.cubic_to(
                    round(cx + k),
                    round(cy - r),
                    round(cx + r),
                    round(cy - k),
                    round(cx + r),
                    round(cy),
                );
                c.close_path();
            }
        }
        self.content.fill_nonzero();
    }

    /// A text frame's glyphs: one path of cubic curves, filled with the non-zero rule.
    fn text(&mut self, text: &TextFill) {
        use ardoise_text::Segment;
        self.style(text.rgb, text.opacity, Blend::Normal);
        let c = &mut self.content;
        for segment in &text.path.to_cubic().segments {
            match *segment {
                Segment::MoveTo([x, y]) => {
                    c.move_to(round(x), round(y));
                }
                Segment::LineTo([x, y]) => {
                    c.line_to(round(x), round(y));
                }
                Segment::CubicTo([ax, ay], [bx, by], [x, y]) => {
                    c.cubic_to(
                        round(ax),
                        round(ay),
                        round(bx),
                        round(by),
                        round(x),
                        round(y),
                    );
                }
                // `to_cubic` leaves none.
                Segment::QuadTo(_, [x, y]) => {
                    c.line_to(round(x), round(y));
                }
                Segment::Close => {
                    c.close_path();
                }
            }
        }
        c.fill_nonzero();
    }

    fn background(&mut self, background: &VectorBackground, width: f32, height: f32) {
        let paper = background.paper;
        self.style([paper.r, paper.g, paper.b], 1.0, Blend::Normal);
        self.shape(Shape::Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        });
        if background.texture.is_some() {
            // The image's first row is at the top: map the unit square upside down.
            self.content.save_state();
            self.content
                .transform([width, 0.0, 0.0, -height, 0.0, height]);
            self.content.x_object(Name(b"Im0"));
            self.content.restore_state();
        }
        for mark in &background.marks {
            self.style(mark.rgb, mark.alpha, Blend::Normal);
            self.shape(mark.shape);
        }
    }
}

/// The object numbers a page is written with.
struct Ids<'a> {
    /// The page tree.
    tree: Ref,
    /// The page itself.
    page: Ref,
    /// The next free object number.
    next: &'a mut Ref,
}

fn write_page(
    pdf: &mut Pdf,
    ids: Ids<'_>,
    view: NoteView<'_>,
    options: &PdfOptions,
    mut groups: Option<&mut Groups>,
    fonts: &mut TextFonts,
    links: &[Link],
    pages: &[Ref],
) -> Result<(), RenderError> {
    let Ids {
        tree,
        page: page_id,
        next,
    } = ids;
    let (width, height) = (view.note.page.width, view.note.page.height);
    if width == 0 || height == 0 {
        return Err(RenderError::Size { width, height });
    }
    let scene = Scene::of(view, options.background);
    let place = Placement::of(scene.width, scene.height, options.page);

    let mut painter = Painter::new();
    painter.content.save_state();
    // Device units, origin at the note's top-left corner, y down.
    painter.content.transform([
        place.scale,
        0.0,
        0.0,
        -place.scale,
        place.left,
        place.height - place.top,
    ]);
    painter.content.rect(0.0, 0.0, scene.width, scene.height);
    painter.content.clip_nonzero();
    painter.content.end_path();
    if let Some(background) = &scene.background {
        painter.background(background, scene.width, scene.height);
    }
    if let Some(placed) = &scene.picture {
        painter.picture(placed);
    }
    // The group of each layer of the page (separate layers). Numbered top layer first, the order
    // a viewer lists them in; the fills are still painted bottom first.
    let mut layer_groups: Vec<Option<usize>> = vec![None; scene.layers.len()];
    // The fonts of the invisible text on this page.
    let mut used_fonts: Vec<usize> = Vec::new();
    if let Some(groups) = groups.as_deref_mut() {
        for (layer, slot) in scene.layers.iter().zip(&mut layer_groups).rev() {
            *slot = group_name(layer).map(|name| groups.number(&name, next));
        }
    }
    for (layer, group) in scene.layers.iter().zip(&layer_groups) {
        // Marked content is not a saved state: what the painter tracks stays true.
        let tag = group.map(|k| format!("L{k}"));
        if let Some(tag) = &tag {
            painter
                .content
                .begin_marked_content_with_properties(Name(b"OC"))
                .properties_named(Name(tag.as_bytes()));
        }
        for fill in &layer.fills {
            painter.style(fill.rgb, fill.flat_opacity(), fill.blend);
            for contour in &fill.outline.contours {
                let mut points = contour.iter();
                let Some(&[x, y]) = points.next() else {
                    continue;
                };
                painter.content.move_to(round(x), round(y));
                for &[x, y] in points {
                    painter.content.line_to(round(x), round(y));
                }
                painter.content.close_path();
            }
            painter.content.fill_nonzero();
        }
        for text in &layer.texts {
            painter.text(text);
            // An equation is its drawing alone: no invisible text (its source is LaTeX).
            let Some(style) = text.layout.as_ref().map(|layout| layout.style) else {
                continue;
            };
            let font = fonts.number(style.family, style.bold, next);
            if !used_fonts.contains(&font) {
                used_fonts.push(font);
            }
            invisible_text(&mut painter.content, text, font, &fonts.widths[font]);
        }
        if tag.is_some() {
            painter.content.end_marked_content();
        }
    }
    painter.content.restore_state();

    let content_id = next.bump();
    let stream = miniz_oxide::deflate::compress_to_vec_zlib(
        painter.content.finish().as_slice(),
        DEFLATE_LEVEL,
    );
    pdf.stream(content_id, &stream).filter(Filter::FlateDecode);

    let mut states: Vec<((u32, Blend), usize)> = painter.states.into_iter().collect();
    states.sort_by_key(|&(_, index)| index);
    let mut state_ids = Vec::with_capacity(states.len());
    for ((opacity, blend), index) in states {
        let id = next.bump();
        let mut state = pdf.ext_graphics(id);
        state.non_stroking_alpha(f32::from_bits(opacity));
        state.blend_mode(match blend {
            Blend::Normal => BlendMode::Normal,
            Blend::Multiply => BlendMode::Multiply,
            Blend::Screen => BlendMode::Screen,
        });
        state.finish();
        state_ids.push((format!("G{index}"), id));
    }

    let texture = match scene.background.as_ref().and_then(|b| b.texture.as_ref()) {
        Some(texture) => {
            let jpeg = crate::jpeg::encode_image(texture, TEXTURE_QUALITY, None)?;
            let id = next.bump();
            let side = |v: u32| {
                i32::try_from(v).map_err(|_| RenderError::Size {
                    width: texture.width,
                    height: texture.height,
                })
            };
            let mut image = pdf.image_xobject(id, &jpeg);
            image.filter(Filter::DctDecode);
            image.width(side(texture.width)?);
            image.height(side(texture.height)?);
            image.color_space().device_rgb();
            image.bits_per_component(8);
            image.finish();
            Some(id)
        }
        None => None,
    };
    let picture = match &scene.picture {
        Some(placed) => Some(write_picture(pdf, next, placed)?),
        None => None,
    };

    // The links of a summary page: a GoTo to the page of a note, at its spot when it has one.
    let mut annotations = Vec::with_capacity(links.len());
    for link in links {
        let id = next.bump();
        let mut annotation = pdf.annotation(id);
        annotation
            .subtype(AnnotationType::Link)
            .rect(link.rect)
            .border(0.0, 0.0, 0.0, None);
        let mut action = annotation.action();
        action.action_type(ActionType::GoTo);
        match link.spot {
            Some([x, y]) => {
                action
                    .destination()
                    .page(pages[link.target])
                    .xyz(x, y, None);
            }
            None => {
                action.destination().page(pages[link.target]).fit();
            }
        }
        action.finish();
        annotation.finish();
        annotations.push(id);
    }

    let mut page = pdf.page(page_id);
    page.parent(tree)
        .media_box(Rect::new(0.0, 0.0, place.width, place.height))
        .contents(content_id);
    if place.user_unit > 1.0 {
        page.user_unit(place.user_unit);
    }
    if !annotations.is_empty() {
        page.annotations(annotations.iter().copied());
    }
    page.group().transparency().color_space().device_rgb();
    let mut resources = page.resources();
    if !state_ids.is_empty() {
        let mut dict = resources.ext_g_states();
        for (name, id) in &state_ids {
            dict.pair(Name(name.as_bytes()), *id);
        }
        dict.finish();
    }
    if !used_fonts.is_empty() {
        used_fonts.sort_unstable();
        let mut dict = resources.fonts();
        for k in used_fonts {
            dict.pair(Name(font_name(k).as_bytes()), fonts.ids[k]);
        }
        dict.finish();
    }
    if texture.is_some() || picture.is_some() {
        let mut objects = resources.x_objects();
        if let Some(id) = texture {
            objects.pair(Name(b"Im0"), id);
        }
        if let Some(id) = picture {
            objects.pair(Name(b"Im1"), id);
        }
        objects.finish();
    }
    if let Some(groups) = groups {
        let mut used: Vec<usize> = layer_groups.iter().flatten().copied().collect();
        used.sort_unstable();
        used.dedup();
        if !used.is_empty() {
            let mut properties = resources.insert(Name(b"Properties")).dict();
            for k in used {
                properties.pair(Name(format!("L{k}").as_bytes()), groups.ids[k]);
            }
            properties.finish();
        }
    }
    resources.finish();
    page.finish();
    Ok(())
}

/// Writes the background image's XObject (and its soft mask); returns its reference. A JPEG
/// in grey or colour goes in as it is; anything else as deflated RGB samples, with an 8-bit
/// soft mask when some pixels are not opaque.
fn write_picture(
    pdf: &mut Pdf,
    next: &mut Ref,
    placed: &PlacedPicture<'_>,
) -> Result<Ref, RenderError> {
    let picture = placed.picture;
    let (width, height) = (picture.width(), picture.height());
    let side = |v: u32| i32::try_from(v).map_err(|_| RenderError::Size { width, height });
    let (w, h) = (side(width)?, side(height)?);
    let id = next.bump();
    if let Some(color) = picture.dct() {
        let mut image = pdf.image_xobject(id, picture.bytes());
        image.filter(Filter::DctDecode);
        image.width(w);
        image.height(h);
        match color {
            DctColor::Gray => image.color_space().device_gray(),
            DctColor::Rgb => image.color_space().device_rgb(),
        }
        image.bits_per_component(8);
        image.interpolate(true);
        image.finish();
        return Ok(id);
    }
    let pixels = picture.image().pixels.as_chunks::<4>().0;
    let rgb: Vec<u8> = pixels.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mask = if pixels.iter().any(|p| p[3] != 255) {
        let alpha: Vec<u8> = pixels.iter().map(|p| p[3]).collect();
        let mask_id = next.bump();
        let data = miniz_oxide::deflate::compress_to_vec_zlib(&alpha, DEFLATE_LEVEL);
        let mut mask = pdf.image_xobject(mask_id, &data);
        mask.filter(Filter::FlateDecode);
        mask.width(w);
        mask.height(h);
        mask.color_space().device_gray();
        mask.bits_per_component(8);
        mask.interpolate(true);
        mask.finish();
        Some(mask_id)
    } else {
        None
    };
    let data = miniz_oxide::deflate::compress_to_vec_zlib(&rgb, DEFLATE_LEVEL);
    let mut image = pdf.image_xobject(id, &data);
    image.filter(Filter::FlateDecode);
    image.width(w);
    image.height(h);
    image.color_space().device_rgb();
    image.bits_per_component(8);
    image.interpolate(true);
    if let Some(mask_id) = mask {
        image.s_mask(mask_id);
    }
    image.finish();
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_pages_have_the_note_size() {
        let place = Placement::of(29_700.0, 21_600.0, PdfPage::Actual);
        // 297 × 216 mm.
        assert!((place.width - 841.89).abs() < 0.01, "{place:?}");
        assert!((place.height - 612.28).abs() < 0.01, "{place:?}");
        assert_eq!(place.user_unit, 1.0);
        assert_eq!((place.left, place.top), (0.0, 0.0));
    }

    #[test]
    fn a4_pages_follow_the_note_orientation_and_centre_it() {
        let landscape = Placement::of(29_700.0, 21_600.0, PdfPage::A4);
        assert!((landscape.width - 841.89).abs() < 0.01);
        assert!((landscape.height - 595.28).abs() < 0.01);
        // 216 mm high in a 210 mm page: scaled down, centred horizontally.
        assert!((landscape.scale * 21_600.0 - landscape.height).abs() < 0.01);
        assert!(landscape.left > 0.0 && landscape.top.abs() < 0.01);

        let portrait = Placement::of(14_800.0, 21_000.0, PdfPage::A4);
        assert!((portrait.width - 595.28).abs() < 0.01);
        assert!((portrait.height - 841.89).abs() < 0.01);
        assert!(portrait.scale * 14_800.0 <= portrait.width + 0.01);
    }

    #[test]
    fn huge_pages_get_a_user_unit() {
        // 10 m long: 28 346 pt, above 14 400.
        let place = Placement::of(29_700.0, 1_000_000.0, PdfPage::Actual);
        assert_eq!(place.user_unit, 2.0);
        assert!(place.height <= MAX_PAGE_SIDE);
        assert!((place.height * place.user_unit - 1_000_000.0 * PT_PER_UNIT).abs() < 1.0);
    }

    #[test]
    fn no_note_no_pdf() {
        assert!(matches!(
            notes_to_pdf::<NoteView<'_>>(&[], &PdfOptions::default()),
            Err(RenderError::Encode(_))
        ));
    }

    fn entry(title: &str, level: usize, page: usize) -> OutlineEntry {
        OutlineEntry {
            title: title.to_owned(),
            level,
            page,
            at: None,
        }
    }

    #[test]
    fn the_outline_is_a_tree_of_the_levels_in_order() {
        let outline = [
            entry("A", 0, 0),
            entry("S", 0, 1),
            entry("S1", 1, 1),
            entry("S2", 1, 2),
            // Too deep (two levels below the entry before it): brought up to its child.
            entry("S2x", 3, 3),
            entry("B", 0, 4),
            // Past the last page: left out.
            entry("lost", 0, 9),
        ];
        let mut next = Ref::new(10);
        let tree = Bookmarks::of(&outline, 5, &mut next).unwrap();
        assert_eq!(tree.items.len(), 6);
        assert_eq!(tree.top, [0, 1, 5]);
        assert_eq!(tree.items[1].children, [2, 3]);
        assert_eq!(tree.items[3].children, [4]);
        assert_eq!(tree.items[4].parent, Some(3));
        assert_eq!(tree.below(1), 3);
        assert_eq!(tree.below(0), 0);
        // Seven objects were taken: six entries and the root.
        assert_eq!(next, Ref::new(17));
        assert!(Bookmarks::of(&[], 5, &mut Ref::new(1)).is_none());
        assert!(Bookmarks::of(&[entry("x", 0, 5)], 5, &mut Ref::new(1)).is_none());
    }

    #[test]
    fn win_ansi_codes_round_trip_and_other_characters_have_none() {
        let mut defined = 0;
        for code in 0..=255u8 {
            if let Some(c) = win_ansi_char(code) {
                defined += 1;
                assert_eq!(win_ansi(c), Some(code), "{code:#x} {c:?}");
            }
        }
        // 0x20 to 0xFF but DEL and the five unused codes of 0x80 to 0x9F.
        assert_eq!(defined, 224 - 1 - 5);
        assert_eq!(win_ansi('é'), Some(0xe9));
        assert_eq!(win_ansi('€'), Some(0x80));
        assert_eq!(win_ansi('œ'), Some(0x9c));
        assert_eq!(win_ansi('’'), Some(0x92));
        for c in ['α', '\u{1d400}', '\n', '\t', '\0', '\u{81}', '\u{7f}', '中'] {
            assert_eq!(win_ansi(c), None, "{c:?}");
        }
    }

    #[test]
    fn the_widths_are_the_advances_of_our_glyphs() {
        for family in FontFamily::ALL {
            for bold in [false, true] {
                let widths = code_widths(family, bold);
                assert_eq!(widths.len(), 224);
                let style = TextStyle {
                    family,
                    bold,
                    size: 1000.0,
                };
                let line = layout("Wé€ «a»", style).lines.remove(0);
                for glyph in &line.glyphs {
                    let c = "Wé€ «a»".encode_utf16().nth(glyph.index).unwrap();
                    let code = win_ansi(char::from_u32(u32::from(c)).unwrap()).unwrap();
                    let width = widths[usize::from(code - FIRST_CODE)];
                    assert!((width - glyph.advance).abs() < 1e-3, "{family:?} {code:#x}");
                }
                assert_eq!(widths[usize::from(0x81 - FIRST_CODE)], 0.0);
            }
        }
    }

    #[test]
    fn groups_are_numbered_by_name_in_order_of_first_appearance() {
        let mut next = Ref::new(10);
        let mut groups = Groups::default();
        assert_eq!(groups.number("B", &mut next), 0);
        assert_eq!(groups.number("A", &mut next), 1);
        assert_eq!(groups.number("B", &mut next), 0);
        assert_eq!(groups.ids, [Ref::new(10), Ref::new(11)]);
        assert_eq!(next, Ref::new(12));
    }

    #[test]
    fn a_group_takes_the_layer_name_or_its_number_and_a_layer_without_entry_has_none() {
        let layer = |index, name: Option<&str>| LayerFills {
            index,
            name: name.map(str::to_owned),
            fills: vec![],
            texts: vec![],
        };
        assert_eq!(
            group_name(&layer(0, Some("Calque 1"))).as_deref(),
            Some("Calque 1")
        );
        assert_eq!(group_name(&layer(2, Some(" "))).as_deref(), Some("3"));
        assert_eq!(group_name(&layer(9, None)), None);
    }
}
