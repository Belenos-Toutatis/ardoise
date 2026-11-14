//! The summary pages of a binder's PDF (`notes_to_pdf_with_summary`), read back with an independent reader
//! (`lopdf`): the pages come first, the numbers they show count them, each row links to the page (or the spot of the
//! page) it names, the outline starts with the summary's entry, and the text is there to search.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Paper, Template, Timing};
use ardoise_render::{
    OutlineEntry, PdfOptions, PdfPage, Summary, SummaryEntry, SummaryKind,
    notes_to_pdf_with_outline, notes_to_pdf_with_summary,
};
use lopdf::{Document, Object, ObjectId};

const SLATE: PageSize = PageSize {
    width: 29_700,
    height: 21_600,
};

fn note(title: &str) -> Note {
    Note {
        id: "00000000-0000-4000-8000-0000000000cc".into(),
        title: title.into(),
        created: 0,
        modified: 0,
        page: SLATE,
        background: Background {
            paper: Paper::White,
            template: Template::Blank,
            texture: false,
        },
        background_image: None,
        tags: vec![],
        favorite: false,
        layers: vec![Layer {
            name: "Calque 1".into(),
            visible: true,
        }],
        strokes: vec![],
        objects: vec![],
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

fn entry(title: &str, level: usize, page: usize, kind: SummaryKind) -> SummaryEntry {
    SummaryEntry {
        title: title.into(),
        level,
        page,
        at: None,
        kind,
    }
}

fn outline(title: &str, level: usize, page: usize) -> OutlineEntry {
    OutlineEntry {
        title: title.into(),
        level,
        page,
        at: None,
    }
}

/// The links of every page: for each, the page it goes to (0-based) and its spot when it has one.
fn links_of(document: &Document) -> Vec<Vec<(usize, Option<[f32; 2]>)>> {
    let ids: Vec<ObjectId> = document.get_pages().values().copied().collect();
    ids.iter()
        .map(|&id| {
            let page = document.get_dictionary(id).unwrap();
            let Ok(annots) = page.get(b"Annots") else {
                return Vec::new();
            };
            annots
                .as_array()
                .unwrap()
                .iter()
                .map(|annot| {
                    let annot = document
                        .get_dictionary(annot.as_reference().unwrap())
                        .unwrap();
                    assert_eq!(annot.get(b"Subtype").unwrap().as_name().unwrap(), b"Link");
                    let Object::Dictionary(action) = annot.get(b"A").unwrap() else {
                        panic!("action")
                    };
                    assert_eq!(action.get(b"S").unwrap().as_name().unwrap(), b"GoTo");
                    let dest = action.get(b"D").unwrap().as_array().unwrap();
                    let target = ids
                        .iter()
                        .position(|p| *p == dest[0].as_reference().unwrap())
                        .unwrap();
                    let at = match dest[1].as_name().unwrap() {
                        b"Fit" => None,
                        b"XYZ" => Some([dest[2].as_float().unwrap(), dest[3].as_float().unwrap()]),
                        other => panic!("destination {other:?}"),
                    };
                    (target, at)
                })
                .collect()
        })
        .collect()
}

/// The title and page (0-based) of each outline entry, in reading order, whatever the depth.
fn outline_pages(document: &Document) -> Vec<(String, usize)> {
    let ids: Vec<ObjectId> = document.get_pages().values().copied().collect();
    let root = document
        .catalog()
        .unwrap()
        .get(b"Outlines")
        .unwrap()
        .as_reference()
        .unwrap();
    let mut out = Vec::new();
    let mut stack = vec![
        document
            .get_dictionary(root)
            .unwrap()
            .get(b"First")
            .unwrap()
            .as_reference()
            .unwrap(),
    ];
    // Depth first: an entry, then its children, then its next sibling.
    fn visit(
        document: &Document,
        ids: &[ObjectId],
        first: ObjectId,
        out: &mut Vec<(String, usize)>,
    ) {
        let mut current = Some(first);
        while let Some(id) = current {
            let item = document.get_dictionary(id).unwrap();
            let Object::String(title, _) = item.get(b"Title").unwrap() else {
                panic!("title")
            };
            let title = match title.as_slice() {
                [0xfe, 0xff, rest @ ..] => String::from_utf16(
                    &rest
                        .chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
                plain => String::from_utf8_lossy(plain).into_owned(),
            };
            let dest = item.get(b"Dest").unwrap().as_array().unwrap();
            let page = ids
                .iter()
                .position(|p| *p == dest[0].as_reference().unwrap())
                .unwrap();
            out.push((title, page));
            if let Ok(child) = item.get(b"First") {
                visit(document, ids, child.as_reference().unwrap(), out);
            }
            current = item.get(b"Next").ok().map(|n| n.as_reference().unwrap());
        }
    }
    visit(document, &ids, stack.pop().unwrap(), &mut out);
    out
}

fn media_box(document: &Document, page: usize) -> [f32; 2] {
    let id = *document.get_pages().values().nth(page).unwrap();
    let rect = document
        .get_dictionary(id)
        .unwrap()
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap();
    [rect[2].as_float().unwrap(), rect[3].as_float().unwrap()]
}

fn options(page: PdfPage) -> PdfOptions {
    PdfOptions {
        page,
        ..PdfOptions::default()
    }
}

#[test]
fn the_summary_pages_come_first_and_every_row_links_to_its_page() {
    let (a, b) = (note("Cours"), note("TP"));
    let summary = Summary {
        title: "Physique".into(),
        entries: vec![
            entry("Cours", 0, 0, SummaryKind::Note),
            SummaryEntry {
                at: Some([500.0, 1500.0]),
                ..entry("Loi d'Ohm", 1, 0, SummaryKind::Bookmark)
            },
            entry("3e B", 0, 1, SummaryKind::Binder),
            entry("TP", 1, 1, SummaryKind::Note),
        ],
    };
    let notes_outline = [
        outline("Cours", 0, 0),
        outline("3e B", 0, 1),
        outline("TP", 1, 1),
    ];
    let pdf = notes_to_pdf_with_summary(
        &[&a, &b],
        &options(PdfPage::A4),
        &notes_outline,
        Some((&summary, "Sommaire")),
    )
    .unwrap();
    let document = Document::load_mem(&pdf).unwrap();
    // One summary page, then the two notes.
    assert_eq!(document.get_pages().len(), 3);
    // A4 portrait for the summary (the notes are fitted on A4 landscape).
    let [w, h] = media_box(&document, 0);
    assert!(
        (w - 595.28).abs() < 0.01 && (h - 841.89).abs() < 0.01,
        "{w} {h}"
    );
    // The links: the row of each entry goes to the page of its note, a bookmark to its spot.
    let links = links_of(&document);
    assert_eq!(links[0].len(), 4);
    assert_eq!(
        links[0].iter().map(|(page, _)| *page).collect::<Vec<_>>(),
        [1, 1, 2, 2]
    );
    assert_eq!(links[0][0].1, None);
    let [x, y] = links[0][1].1.expect("the bookmark has a spot");
    assert!(x > 0.0 && y > 0.0);
    assert!(links[1].is_empty() && links[2].is_empty());
    // The outline: the summary first, then the notes' entries moved back by the summary's page.
    assert_eq!(
        outline_pages(&document),
        [
            ("Sommaire".to_owned(), 0),
            ("Cours".to_owned(), 1),
            ("3e B".to_owned(), 2),
            ("TP".to_owned(), 2),
        ]
    );
    // The text is there to search: the title, a note's name, and its page number (2: the summary is page 1).
    let text = document.extract_text(&[1]).unwrap();
    assert!(text.contains("Physique"), "{text:?}");
    assert!(text.contains("Cours") && text.contains("TP"), "{text:?}");
    assert!(text.contains('2') && text.contains('3'), "{text:?}");
}

#[test]
fn a_long_summary_takes_several_pages_and_the_links_count_them() {
    let notes: Vec<Note> = (0..5).map(|i| note(&format!("Note {i}"))).collect();
    let refs: Vec<&Note> = notes.iter().collect();
    // 120 rows: a bookmark-heavy binder needs more than one A4 page.
    let entries: Vec<SummaryEntry> = (0..120)
        .map(|i| entry(&format!("Ligne {i}"), i % 3, i % 5, SummaryKind::Note))
        .collect();
    let summary = Summary {
        title: "Long".into(),
        entries,
    };
    let pdf = notes_to_pdf_with_summary(
        &refs,
        &options(PdfPage::A4),
        &[outline("Note 0", 0, 0)],
        Some((&summary, "Sommaire")),
    )
    .unwrap();
    let document = Document::load_mem(&pdf).unwrap();
    let pages = document.get_pages().len();
    let front = pages - 5;
    assert!(front >= 3, "{front} summary pages");
    let links = links_of(&document);
    assert_eq!(links.iter().take(front).map(Vec::len).sum::<usize>(), 120);
    assert!(links.iter().skip(front).all(Vec::is_empty));
    // Row i goes to note i % 5, which is page front + i % 5; the rows are in order over the pages.
    let flat: Vec<usize> = links
        .iter()
        .take(front)
        .flatten()
        .map(|(page, _)| *page)
        .collect();
    assert_eq!(flat, (0..120).map(|i| front + i % 5).collect::<Vec<_>>());
    // The numbers shown count the summary's pages: the last page's first row says `front + 1 + ...`.
    let text = document.extract_text(&[front as u32]).unwrap();
    assert!(text.contains(&(front + 5).to_string()), "{text:?}");
    // The outline's entry of the first note is after the summary pages.
    assert_eq!(
        outline_pages(&document),
        [("Sommaire".to_owned(), 0), ("Note 0".to_owned(), front)]
    );
}

#[test]
fn the_pages_of_the_notes_size_follow_the_first_note_without_a4() {
    let a = note("a");
    let summary = Summary {
        title: "t".into(),
        entries: vec![entry("a", 0, 0, SummaryKind::Note)],
    };
    let pdf = notes_to_pdf_with_summary(
        &[&a],
        &options(PdfPage::Actual),
        &[],
        Some((&summary, "Sommaire")),
    )
    .unwrap();
    let document = Document::load_mem(&pdf).unwrap();
    let [w, h] = media_box(&document, 0);
    let [nw, nh] = media_box(&document, 1);
    assert!(
        (w - nw).abs() < 0.01 && (h - nh).abs() < 0.01,
        "{w}x{h} {nw}x{nh}"
    );
}

#[test]
fn without_a_summary_the_document_is_the_one_of_the_outline_alone() {
    let (a, b) = (note("a"), note("b"));
    let entries = [outline("a", 0, 0), outline("b", 0, 1)];
    let plain = notes_to_pdf_with_outline(&[&a, &b], &PdfOptions::default(), &entries).unwrap();
    let none =
        notes_to_pdf_with_summary(&[&a, &b], &PdfOptions::default(), &entries, None).unwrap();
    assert_eq!(plain, none);
    // A summary with no row adds no page.
    let empty = Summary {
        title: "t".into(),
        entries: Vec::new(),
    };
    let still = notes_to_pdf_with_summary(
        &[&a, &b],
        &PdfOptions::default(),
        &entries,
        Some((&empty, "Sommaire")),
    )
    .unwrap();
    assert_eq!(Document::load_mem(&still).unwrap().get_pages().len(), 2);
    assert!(
        outline_pages(&Document::load_mem(&still).unwrap())
            .iter()
            .all(|(title, _)| title != "Sommaire")
    );
}
