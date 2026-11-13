//! Direct printing of notes (SPEC §4.6, milestone M8): the Windows print dialog, then each
//! note on a page of its own, in its orientation (portrait or landscape), with the paper
//! (colour, template, texture, background image) or without it, like the PDF export. One note
//! per sheet, or 2 or 4 notes per sheet (Q20): the cells of a sheet are cut by Ardoise itself,
//! because the job draws the pages: the printer driver's own « pages per sheet » would act on
//! a sheet that already is a page of ours, and the driver's setting is not read.
//! Contract: `docs/design/exports-and-operations.md`, "M8: printing".
//!
//! The mechanism is GDI, through the system print dialog (`PrintDlgEx`): the dialog gives the
//! printer, its settings and the page range (in sheets); the job then draws one sheet at a time,
//! changing the orientation of the device context between two sheets (`ResetDC`). Each note is
//! rendered as an image at the printer's resolution (300 dpi at most) and stretched over its
//! cell of the sheet.
//! The Windows part is in [`windows`](crate::print_windows); what decides where and how big
//! each page is drawn is platform-free and tested here.
//! Off Windows nothing calls that logic but the tests (hence the `dead_code` allowances).

use ardoise_core::note::PageSize;
use ardoise_render::{Image, RenderOptions, render_note};
use serde::{Deserialize, Serialize};

use crate::commands::{CommandError, CommandResult};
use crate::export::{LoadedSource, render_error};

/// Error code of a print job that failed (the dialog cancelled is not a failure).
#[cfg_attr(not(windows), allow(dead_code))]
pub const PRINT_FAILED: &str = "printFailed";
/// Error code when printing is not possible (not Windows).
#[cfg_attr(windows, allow(dead_code))]
pub const PRINT_UNAVAILABLE: &str = "printUnavailable";
/// Highest resolution of a printed image, in dots per inch: ink and paper textures gain
/// nothing above, and a page at 600 dpi would take 100 MB.
#[cfg_attr(not(windows), allow(dead_code))]
pub const MAX_PRINT_DPI: f32 = 300.0;
/// Device units (10 µm) in an inch.
#[cfg_attr(not(windows), allow(dead_code))]
const UNITS_PER_INCH: f32 = 2540.0;

/// How a note is placed on the paper (`PrintFit` in `note.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrintFit {
    /// The note's real size, centred; reduced only when it is larger than the sheet.
    Actual,
    /// As large as the sheet allows, centred.
    Fit,
}

/// `PrintOptions` in `note.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintOptions {
    /// Print the paper (colour, template, texture, background image); otherwise ink on white.
    pub background: bool,
    pub fit: PrintFit,
    /// Notes per sheet: 1, 2 or 4 (anything else is taken for 1); absent in the choices an older
    /// version remembered.
    #[serde(default = "one_per_sheet")]
    pub per_sheet: u8,
}

fn one_per_sheet() -> u8 {
    1
}

impl PrintOptions {
    /// How many notes share a sheet: 1, 2 or 4.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn notes_per_sheet(&self) -> usize {
        match self.per_sheet {
            2 => 2,
            4 => 4,
            _ => 1,
        }
    }
}

/// `PrintResult` in `note.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintResult {
    /// False when the user closed the dialog without printing.
    pub printed: bool,
    /// Pages sent to the printer.
    pub pages: u32,
}

/// The orientation a sheet takes for a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    /// Landscape when the note is wider than tall, portrait otherwise (a square included).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn of(page: PageSize) -> Orientation {
        if page.width > page.height {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        }
    }
}

/// The printable area of a sheet, in device pixels, and the device's resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Sheet {
    pub width: i32,
    pub height: i32,
    pub dpi_x: i32,
    pub dpi_y: i32,
}

/// A rectangle of the printable area, in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Where a note of `page` goes on `sheet`: centred, its proportions kept.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn place(page: PageSize, sheet: Sheet, fit: PrintFit) -> Placement {
    let real_width =
        page.width.max(1) as f64 / f64::from(UNITS_PER_INCH) * f64::from(sheet.dpi_x.max(1));
    let real_height =
        page.height.max(1) as f64 / f64::from(UNITS_PER_INCH) * f64::from(sheet.dpi_y.max(1));
    let mut scale = (f64::from(sheet.width.max(1)) / real_width)
        .min(f64::from(sheet.height.max(1)) / real_height);
    if fit == PrintFit::Actual {
        scale = scale.min(1.0);
    }
    let width = ((real_width * scale).round() as i32).clamp(1, sheet.width.max(1));
    let height = ((real_height * scale).round() as i32).clamp(1, sheet.height.max(1));
    Placement {
        x: (sheet.width - width) / 2,
        y: (sheet.height - height) / 2,
        width,
        height,
    }
}

/// Pixels per device unit for rendering a note of `page` that is drawn `placement.width`
/// device pixels wide: one image pixel per device pixel, at most [`MAX_PRINT_DPI`].
#[cfg_attr(not(windows), allow(dead_code))]
pub fn render_scale(page: PageSize, placement: &Placement) -> f32 {
    let wanted = placement.width as f32 / page.width.max(1) as f32;
    wanted.min(MAX_PRINT_DPI / UNITS_PER_INCH)
}

/// The notes (0-based) a print job prints, from the page ranges (1-based, inclusive) the print
/// dialog returned; no range means every note. Ranges outside `1..=count` are cut, a reversed
/// one is read the other way round, and the notes come in the order of the ranges.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn selected_notes(count: usize, ranges: &[(u32, u32)]) -> Vec<usize> {
    if ranges.is_empty() {
        return (0..count).collect();
    }
    let mut out = Vec::new();
    for &(from, to) in ranges {
        let (low, high) = (from.min(to).max(1) as usize, from.max(to) as usize);
        out.extend((low..=high.min(count)).map(|page| page - 1));
    }
    out
}

/// How many sheets `notes` notes take at `per_sheet` notes per sheet.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn sheet_count(notes: usize, per_sheet: usize) -> usize {
    notes.div_ceil(per_sheet.max(1))
}

/// The sheets a print job prints, each as the notes (0-based) it holds, from the page ranges the
/// print dialog returned: the pages of the dialog are the sheets ([`selected_notes`]'s rule, over
/// the sheets), and the last sheet may hold fewer notes. The notes of a sheet are consecutive.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn selected_sheets(notes: usize, per_sheet: usize, ranges: &[(u32, u32)]) -> Vec<Vec<usize>> {
    let per_sheet = per_sheet.max(1);
    selected_notes(sheet_count(notes, per_sheet), ranges)
        .into_iter()
        .map(|sheet| (sheet * per_sheet..((sheet + 1) * per_sheet).min(notes)).collect())
        .collect()
}

/// The orientation of a sheet whose first note is `first`: that note's own for one or four notes
/// (a 2 x 2 sheet keeps the shape of its notes), and the other one for two (two portrait notes
/// stand side by side on a landscape sheet, two landscape ones one above the other on a
/// portrait sheet).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn sheet_orientation(first: PageSize, per_sheet: usize) -> Orientation {
    match (Orientation::of(first), per_sheet) {
        (Orientation::Portrait, 2) => Orientation::Landscape,
        (Orientation::Landscape, 2) => Orientation::Portrait,
        (orientation, _) => orientation,
    }
}

/// The cells of `sheet` for `per_sheet` notes, in reading order, each a rectangle of the
/// printable area: the whole sheet for one; two columns for two on a landscape sheet (two rows on
/// a portrait one); two by two for four. About 4 mm separate the cells.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn cells(sheet: Sheet, per_sheet: usize) -> Vec<Placement> {
    let (columns, rows) = match per_sheet {
        4 => (2, 2),
        2 if sheet.width >= sheet.height => (2, 1),
        2 => (1, 2),
        _ => (1, 1),
    };
    let gap_x = (sheet.dpi_x / 6).max(2);
    let gap_y = (sheet.dpi_y / 6).max(2);
    let width = ((sheet.width - gap_x * (columns - 1)) / columns).max(1);
    let height = ((sheet.height - gap_y * (rows - 1)) / rows).max(1);
    (0..rows)
        .flat_map(|row| {
            (0..columns).map(move |column| Placement {
                x: column * (width + gap_x),
                y: row * (height + gap_y),
                width,
                height,
            })
        })
        .collect()
}

/// A rendered page in the order GDI wants it: top-down 32-bit BGRA rows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct PageBitmap {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// `image` with red and blue swapped and the alpha dropped (the page is opaque: it has its
/// paper, or the white page under the ink).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn bitmap_of(image: &Image) -> PageBitmap {
    let mut bgra = Vec::with_capacity(image.pixels.len());
    for pixel in image.pixels.as_chunks::<4>().0 {
        bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    PageBitmap {
        width: image.width,
        height: image.height,
        bgra,
    }
}

/// The page of `source` to send to the printer when it is drawn in `placement`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn render_page(
    source: &LoadedSource,
    placement: &Placement,
    options: &PrintOptions,
) -> CommandResult<PageBitmap> {
    let render = RenderOptions {
        scale: render_scale(source.note.page, placement),
        background: options.background,
    };
    let image = render_note(source.view(), &render).map_err(render_error)?;
    Ok(bitmap_of(&image))
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn print_error(message: impl ToString) -> CommandError {
    CommandError::new(PRINT_FAILED, message)
}

/// Opens the Windows print dialog for `sources` and prints each note on its own page.
/// `printUnavailable` outside Windows.
#[tauri::command]
pub async fn print_notes(
    window: tauri::WebviewWindow,
    context: tauri::State<'_, crate::AppContext>,
    sources: Vec<crate::export::ExportSource>,
    options: PrintOptions,
) -> CommandResult<PrintResult> {
    #[cfg(windows)]
    {
        let library = crate::export::library_for(&context, &sources)?;
        let owner = window.hwnd().map_or(0, |hwnd| hwnd.0 as usize);
        tauri::async_runtime::spawn_blocking(move || {
            let loaded = crate::export::load_sources(library.as_ref(), sources)?;
            crate::print_windows::print_with_dialog(owner, &loaded, &options)
        })
        .await
        .map_err(print_error)?
    }
    #[cfg(not(windows))]
    {
        let _ = (window, context, sources, options);
        Err(CommandError::new(
            PRINT_UNAVAILABLE,
            "printing is only available on Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A4_PORTRAIT: PageSize = PageSize {
        width: 21_000,
        height: 29_700,
    };
    const A4_LANDSCAPE: PageSize = PageSize {
        width: 29_700,
        height: 21_000,
    };
    /// An A4 sheet at 600 dpi with 4 mm of unprintable margin on each side.
    fn sheet() -> Sheet {
        Sheet {
            width: 4960 - 190,
            height: 7016 - 190,
            dpi_x: 600,
            dpi_y: 600,
        }
    }

    #[test]
    fn the_orientation_follows_the_note() {
        assert_eq!(Orientation::of(A4_PORTRAIT), Orientation::Portrait);
        assert_eq!(Orientation::of(A4_LANDSCAPE), Orientation::Landscape);
        let square = PageSize {
            width: 100,
            height: 100,
        };
        assert_eq!(Orientation::of(square), Orientation::Portrait);
    }

    #[test]
    fn a_note_is_centred_and_keeps_its_proportions() {
        // An A4 note on an A4 sheet: real size is slightly too large for the printable area,
        // so it is reduced a little; fitting gives the same.
        let actual = place(A4_PORTRAIT, sheet(), PrintFit::Actual);
        let fit = place(A4_PORTRAIT, sheet(), PrintFit::Fit);
        assert_eq!(actual, fit);
        assert!(actual.width <= 4770 && actual.height <= 6826);
        assert_eq!(actual.x, (4770 - actual.width) / 2);
        assert_eq!(actual.y, (6826 - actual.height) / 2);
        let ratio = f64::from(actual.width) / f64::from(actual.height);
        assert!((ratio - 21.0 / 29.7).abs() < 0.001, "{ratio}");
    }

    #[test]
    fn a_small_note_keeps_its_real_size_unless_fitting_is_asked() {
        // 10 x 5 cm at 600 dpi: 2362 x 1181 px.
        let small = PageSize {
            width: 10_000,
            height: 5_000,
        };
        let actual = place(small, sheet(), PrintFit::Actual);
        assert_eq!((actual.width, actual.height), (2362, 1181));
        assert_eq!(actual.x, (4770 - 2362) / 2);
        let fit = place(small, sheet(), PrintFit::Fit);
        assert_eq!(fit.width, 4770);
        assert_eq!(fit.height, 2385);
        assert_eq!(fit.x, 0);
    }

    #[test]
    fn a_long_page_is_reduced_to_the_sheet_and_unequal_resolutions_are_honoured() {
        let tall = PageSize {
            width: 21_000,
            height: 90_000,
        };
        let placed = place(tall, sheet(), PrintFit::Actual);
        assert_eq!(placed.height, 6826);
        assert!(placed.width < 1600);
        // A printer with 600 x 300 dots per inch: the same physical shape.
        let anamorphic = Sheet {
            width: 4770,
            height: 3400,
            dpi_x: 600,
            dpi_y: 300,
        };
        let placed = place(A4_PORTRAIT, anamorphic, PrintFit::Fit);
        let physical = (f64::from(placed.width) / 600.0) / (f64::from(placed.height) / 300.0);
        assert!((physical - 21.0 / 29.7).abs() < 0.01, "{physical}");
        // Degenerate sizes still give a visible rectangle inside the sheet.
        let empty = PageSize {
            width: 0,
            height: 0,
        };
        let p = place(empty, sheet(), PrintFit::Actual);
        assert!(p.width >= 1 && p.height >= 1 && p.x >= 0 && p.y >= 0);
    }

    #[test]
    fn the_image_is_rendered_at_the_sheet_resolution_but_never_above_300_dpi() {
        let placed = place(A4_PORTRAIT, sheet(), PrintFit::Fit);
        let scale = render_scale(A4_PORTRAIT, &placed);
        assert!((scale - 300.0 / 2540.0).abs() < 1e-6, "{scale}");
        // A low-resolution printer: one pixel per dot.
        let draft = Sheet {
            width: 800,
            height: 1100,
            dpi_x: 96,
            dpi_y: 96,
        };
        let placed = place(A4_PORTRAIT, draft, PrintFit::Fit);
        let scale = render_scale(A4_PORTRAIT, &placed);
        let width = (21_000.0 * scale).round() as i32;
        assert!(
            (width - placed.width).abs() <= 1,
            "{width} vs {}",
            placed.width
        );
    }

    #[test]
    fn the_page_ranges_choose_the_notes() {
        assert_eq!(selected_notes(3, &[]), [0, 1, 2]);
        assert_eq!(selected_notes(5, &[(2, 3)]), [1, 2]);
        assert_eq!(selected_notes(5, &[(4, 9)]), [3, 4]);
        assert_eq!(selected_notes(5, &[(3, 3), (1, 1)]), [2, 0]);
        assert_eq!(selected_notes(5, &[(3, 2)]), [1, 2]);
        assert_eq!(selected_notes(2, &[(0, 1)]), [0]);
        assert!(selected_notes(2, &[(7, 9)]).is_empty());
    }

    #[test]
    fn a_sheet_holds_one_two_or_four_notes_and_the_dialog_counts_sheets() {
        let per = |n: u8| {
            PrintOptions {
                background: true,
                fit: PrintFit::Fit,
                per_sheet: n,
            }
            .notes_per_sheet()
        };
        assert_eq!(
            [per(1), per(2), per(4), per(0), per(3), per(9)],
            [1, 2, 4, 1, 1, 1]
        );
        assert_eq!(sheet_count(5, 1), 5);
        assert_eq!(sheet_count(5, 2), 3);
        assert_eq!(sheet_count(8, 4), 2);
        assert_eq!(sheet_count(0, 4), 0);
        // One per sheet: the notes themselves.
        assert_eq!(selected_sheets(3, 1, &[]), [[0], [1], [2]]);
        // Five notes, two per sheet: three sheets, the last with one note.
        assert_eq!(
            selected_sheets(5, 2, &[]),
            [vec![0, 1], vec![2, 3], vec![4]]
        );
        // The page ranges of the dialog are sheets: "2" is notes 3 and 4.
        assert_eq!(selected_sheets(5, 2, &[(2, 2)]), [vec![2, 3]]);
        assert_eq!(
            selected_sheets(9, 4, &[(3, 3), (1, 1)]),
            [vec![8], vec![0, 1, 2, 3]]
        );
        assert!(selected_sheets(5, 2, &[(4, 9)]).is_empty());
        assert!(selected_sheets(0, 2, &[]).is_empty());
    }

    #[test]
    fn the_sheet_turns_so_that_notes_fit_side_by_side_when_two_share_it() {
        assert_eq!(sheet_orientation(A4_PORTRAIT, 1), Orientation::Portrait);
        assert_eq!(sheet_orientation(A4_LANDSCAPE, 1), Orientation::Landscape);
        assert_eq!(sheet_orientation(A4_PORTRAIT, 2), Orientation::Landscape);
        assert_eq!(sheet_orientation(A4_LANDSCAPE, 2), Orientation::Portrait);
        assert_eq!(sheet_orientation(A4_PORTRAIT, 4), Orientation::Portrait);
        assert_eq!(sheet_orientation(A4_LANDSCAPE, 4), Orientation::Landscape);
    }

    #[test]
    fn a_sheet_is_cut_in_cells_that_do_not_touch_and_stay_inside_it() {
        let portrait = sheet();
        let landscape = Sheet {
            width: portrait.height,
            height: portrait.width,
            ..portrait
        };
        // One note: the whole sheet, as before.
        assert_eq!(
            cells(portrait, 1),
            [Placement {
                x: 0,
                y: 0,
                width: portrait.width,
                height: portrait.height
            }]
        );
        // Two on a landscape sheet: side by side; on a portrait sheet: one above the other.
        let side = cells(landscape, 2);
        assert_eq!(side.len(), 2);
        assert_eq!(side[0].y, side[1].y);
        assert!(side[0].x + side[0].width < side[1].x);
        let stacked = cells(portrait, 2);
        assert_eq!(stacked[0].x, stacked[1].x);
        assert!(stacked[0].y + stacked[0].height < stacked[1].y);
        // Four: two by two, in reading order, the same size, all inside the sheet.
        for sheet in [portrait, landscape] {
            let four = cells(sheet, 4);
            assert_eq!(four.len(), 4);
            assert!(
                four.iter()
                    .all(|c| c.width == four[0].width && c.height == four[0].height)
            );
            assert_eq!((four[0].y, four[1].y, four[2].x), (0, 0, 0));
            assert!(four[1].x > four[0].x && four[2].y > four[0].y);
            for cell in &four {
                assert!(cell.x >= 0 && cell.y >= 0);
                assert!(cell.x + cell.width <= sheet.width && cell.y + cell.height <= sheet.height);
            }
            let (last_x, last_y) = (four[3].x + four[3].width, four[3].y + four[3].height);
            assert!(
                sheet.width - last_x < 2 && sheet.height - last_y < 2,
                "the sheet is used up to its edge"
            );
        }
        // A note fits its cell with its proportions kept.
        let cell = cells(landscape, 2)[0];
        let in_cell = Sheet {
            width: cell.width,
            height: cell.height,
            ..landscape
        };
        let placed = place(A4_PORTRAIT, in_cell, PrintFit::Fit);
        assert!(placed.width <= cell.width && placed.height <= cell.height);
        let ratio = f64::from(placed.width) / f64::from(placed.height);
        assert!((ratio - 21.0 / 29.7).abs() < 0.002, "{ratio}");
    }

    #[test]
    fn pixels_go_to_the_printer_as_opaque_bgra() {
        let image = Image {
            width: 2,
            height: 1,
            pixels: vec![10, 20, 30, 255, 1, 2, 3, 128],
        };
        let bitmap = bitmap_of(&image);
        assert_eq!((bitmap.width, bitmap.height), (2, 1));
        assert_eq!(bitmap.bgra, [30, 20, 10, 255, 3, 2, 1, 255]);
    }

    #[test]
    fn options_and_results_have_the_interface_shape() {
        let options: PrintOptions =
            serde_json::from_value(serde_json::json!({ "background": false, "fit": "fit" }))
                .unwrap();
        assert_eq!(
            options,
            PrintOptions {
                background: false,
                fit: PrintFit::Fit,
                per_sheet: 1
            }
        );
        let options: PrintOptions = serde_json::from_value(
            serde_json::json!({ "background": true, "fit": "actual", "perSheet": 4 }),
        )
        .unwrap();
        assert_eq!(options.notes_per_sheet(), 4);
        let json = serde_json::to_value(PrintResult {
            printed: true,
            pages: 3,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "printed": true, "pages": 3 }));
    }
}
