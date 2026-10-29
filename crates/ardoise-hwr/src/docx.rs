//! A note (or several) as a Word document (`.docx`, Office Open XML), written with the `zip`
//! crate and plain strings: the title, the date and the tags, then the paragraphs, a bulleted
//! list for the list items and the drawings as embedded PNG pictures. One A4 portrait section;
//! each note starts on a new page.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::document::{Block, Document, Drawing, Labels, Mark, Text};

/// EMU (English Metric Units) in a millimetre.
const EMU_PER_MM: f64 = 36_000.0;
/// Width of the text area of the page: A4 (210 mm) minus two margins of 20 mm.
const TEXT_WIDTH_MM: f64 = 170.0;
/// A picture is never taller than this, so that it fits on a page (297 mm minus margins).
const MAX_PICTURE_HEIGHT_MM: f64 = 240.0;

/// Failure while building the file.
#[derive(Debug, thiserror::Error)]
pub enum DocxError {
    #[error("cannot write the Word document: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("cannot write the Word document: {0}")]
    Io(#[from] std::io::Error),
}

/// Escapes text for an XML element or attribute, dropping the characters XML 1.0 forbids.
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

const HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

const CONTENT_TYPES: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
    "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
    "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
    "<Default Extension=\"png\" ContentType=\"image/png\"/>",
    "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
    "<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>",
    "<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>",
    "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
    "</Types>"
);

const ROOT_RELS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
    "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>",
    "</Relationships>"
);

const STYLES: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    "<w:docDefaults>",
    "<w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"fr-FR\" w:eastAsia=\"en-US\" w:bidi=\"ar-SA\"/></w:rPr></w:rPrDefault>",
    "<w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"264\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault>",
    "</w:docDefaults>",
    "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:spacing w:after=\"80\"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val=\"44\"/><w:szCs w:val=\"44\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Subtitle\"><w:name w:val=\"Subtitle\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:spacing w:after=\"40\"/></w:pPr><w:rPr><w:color w:val=\"595959\"/><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"720\"/><w:contextualSpacing/></w:pPr></w:style>",
    "</w:styles>"
);

const NUMBERING: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    "<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>",
    "<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"\u{2022}\"/><w:lvlJc w:val=\"left\"/>",
    "<w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl>",
    "</w:abstractNum>",
    "<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>",
    "</w:numbering>"
);

/// A paragraph holding one run of text.
fn text_paragraph(properties: &str, text: &str) -> String {
    format!(
        "<w:p>{properties}<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
        xml_escape(text)
    )
}

/// A paragraph of marked text: one run per span (a space between two), the underlined ones
/// underlined, the struck ones struck.
fn marked_paragraph(properties: &str, text: &Text) -> String {
    let mut runs = String::new();
    let spans = text
        .spans
        .iter()
        .filter(|span| !span.text.trim().is_empty());
    for (index, span) in spans.enumerate() {
        let format = match span.mark {
            Mark::None => "",
            Mark::Underline => "<w:rPr><w:u w:val=\"single\"/></w:rPr>",
            Mark::Strike => "<w:rPr><w:strike/></w:rPr>",
        };
        // The space between two spans is never underlined or struck.
        let space = if index > 0 { " " } else { "" };
        if !space.is_empty() && span.mark != Mark::None {
            runs.push_str("<w:r><w:t xml:space=\"preserve\"> </w:t></w:r>");
        }
        let lead = if span.mark == Mark::None { space } else { "" };
        runs.push_str(&format!(
            "<w:r>{format}<w:t xml:space=\"preserve\">{lead}{}</w:t></w:r>",
            xml_escape(span.text.trim())
        ));
    }
    format!("<w:p>{properties}{runs}</w:p>")
}

fn properties(parts: &[&str]) -> String {
    if parts.is_empty() {
        String::new()
    } else {
        format!("<w:pPr>{}</w:pPr>", parts.concat())
    }
}

/// The size of a picture on the page, in EMU: its real size, reduced to fit the text area.
fn picture_size(drawing: &Drawing) -> (u64, u64) {
    let (width, height) = (
        f64::from(drawing.width_mm).max(1.0),
        f64::from(drawing.height_mm).max(1.0),
    );
    let fit = (TEXT_WIDTH_MM / width)
        .min(MAX_PICTURE_HEIGHT_MM / height)
        .min(1.0);
    (
        (width * fit * EMU_PER_MM).round() as u64,
        (height * fit * EMU_PER_MM).round() as u64,
    )
}

/// An inline picture in its own paragraph; `number` (from 1) names it and its relationship.
fn picture_paragraph(drawing: &Drawing, number: usize, alt: &str) -> String {
    let (cx, cy) = picture_size(drawing);
    format!(
        concat!(
            "<w:p><w:r><w:drawing>",
            "<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">",
            "<wp:extent cx=\"{cx}\" cy=\"{cy}\"/>",
            "<wp:docPr id=\"{number}\" name=\"{name}\" descr=\"{alt}\"/>",
            "<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>",
            "<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
            "<pic:pic><pic:nvPicPr><pic:cNvPr id=\"{number}\" name=\"image{number}.png\"/><pic:cNvPicPr/></pic:nvPicPr>",
            "<pic:blipFill><a:blip r:embed=\"rIdImage{number}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>",
            "<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>",
            "</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        ),
        cx = cx,
        cy = cy,
        number = number,
        name = xml_escape(alt),
        alt = xml_escape(alt),
    )
}

/// The `word/document.xml` part, and the pictures in the order of their numbers.
fn document_xml<'a>(documents: &'a [Document], labels: &Labels) -> (String, Vec<&'a Drawing>) {
    let mut body = String::new();
    let mut pictures: Vec<&Drawing> = Vec::new();
    for (index, document) in documents.iter().enumerate() {
        let mut title_parts = vec!["<w:pStyle w:val=\"Title\"/>"];
        if index > 0 {
            title_parts.push("<w:pageBreakBefore/>");
        }
        let title_properties = properties(&title_parts);
        body.push_str(&text_paragraph(&title_properties, document.title.trim()));
        let subtitle = properties(&["<w:pStyle w:val=\"Subtitle\"/>"]);
        if !document.date.is_empty() {
            body.push_str(&text_paragraph(
                &subtitle,
                &format!("{} {}", labels.date, document.date),
            ));
        }
        if !document.tags.is_empty() {
            body.push_str(&text_paragraph(
                &subtitle,
                &format!("{} {}", labels.tags, document.tags.join(", ")),
            ));
        }
        for block in &document.blocks {
            match block {
                Block::Paragraph(text) => body.push_str(&marked_paragraph("", text)),
                Block::ListItem(text) => body.push_str(&marked_paragraph(
                    &properties(&[
                        "<w:pStyle w:val=\"ListParagraph\"/>",
                        "<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr>",
                    ]),
                    text,
                )),
                Block::Drawing(drawing) => {
                    pictures.push(drawing);
                    let number = pictures.len();
                    body.push_str(&picture_paragraph(
                        drawing,
                        number,
                        &format!("{} {number}", labels.drawing),
                    ));
                }
            }
        }
    }
    let xml = format!(
        concat!(
            "{header}<w:document ",
            "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
            "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
            "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" ",
            "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" ",
            "xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
            "<w:body>{body}",
            "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/>",
            "<w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" ",
            "w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr>",
            "</w:body></w:document>"
        ),
        header = HEADER,
        body = body
    );
    (xml, pictures)
}

fn document_rels(pictures: usize) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    xml.push_str("<Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>");
    xml.push_str("<Relationship Id=\"rIdNumbering\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>");
    for number in 1..=pictures {
        xml.push_str(&format!(
            "<Relationship Id=\"rIdImage{number}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image{number}.png\"/>"
        ));
    }
    xml.push_str("</Relationships>");
    xml
}

fn core_xml(title: &str) -> String {
    format!(
        concat!(
            "{header}<cp:coreProperties ",
            "xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" ",
            "xmlns:dc=\"http://purl.org/dc/elements/1.1/\">",
            "<dc:title>{title}</dc:title><dc:creator>Ardoise</dc:creator></cp:coreProperties>"
        ),
        header = HEADER,
        title = xml_escape(title)
    )
}

/// The `.docx` file of `documents`, one note after the other, each on a new page.
pub fn to_docx(documents: &[Document], labels: &Labels) -> Result<Vec<u8>, DocxError> {
    let (document, pictures) = document_xml(documents, labels);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // PNG data is already compressed.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut add = |name: &str, options: SimpleFileOptions, bytes: &[u8]| -> Result<(), DocxError> {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
        Ok(())
    };
    add("[Content_Types].xml", deflated, CONTENT_TYPES.as_bytes())?;
    add("_rels/.rels", deflated, ROOT_RELS.as_bytes())?;
    add("word/document.xml", deflated, document.as_bytes())?;
    add(
        "word/_rels/document.xml.rels",
        deflated,
        document_rels(pictures.len()).as_bytes(),
    )?;
    add("word/styles.xml", deflated, STYLES.as_bytes())?;
    add("word/numbering.xml", deflated, NUMBERING.as_bytes())?;
    let title = documents.first().map_or("", |d| d.title.trim());
    add("docProps/core.xml", deflated, core_xml(title).as_bytes())?;
    for (index, drawing) in pictures.iter().enumerate() {
        add(
            &format!("word/media/image{}.png", index + 1),
            stored,
            &drawing.png,
        )?;
    }
    Ok(zip.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use crate::document::tests::{labels, sample};

    fn parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut file = archive.by_index(i).unwrap();
                let mut data = Vec::new();
                file.read_to_end(&mut data).unwrap();
                (file.name().unwrap().into_owned(), data)
            })
            .collect()
    }

    fn text_of(parts: &[(String, Vec<u8>)], name: &str) -> String {
        let (_, data) = parts.iter().find(|(n, _)| n == name).unwrap();
        String::from_utf8(data.clone()).unwrap()
    }

    #[test]
    fn xml_text_is_escaped_and_cleaned() {
        assert_eq!(xml_escape("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
        assert_eq!(xml_escape("a\u{0}b\u{8}c\td"), "abc d");
        assert_eq!(xml_escape("é ç 😀"), "é ç 😀");
    }

    #[test]
    fn every_part_is_well_formed_and_every_reference_resolves() {
        let two = [sample(), sample()];
        let bytes = to_docx(&two, &labels()).unwrap();
        let parts = parts(&bytes);
        let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names[0], "[Content_Types].xml");
        for expected in [
            "_rels/.rels",
            "word/document.xml",
            "word/_rels/document.xml.rels",
            "word/styles.xml",
            "word/numbering.xml",
            "docProps/core.xml",
            "word/media/image1.png",
            "word/media/image2.png",
        ] {
            assert!(names.contains(&expected), "{expected} in {names:?}");
        }
        assert_eq!(names.len(), 9);
        // Every XML part parses.
        for (name, data) in &parts {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let text = std::str::from_utf8(data).unwrap();
                roxmltree::Document::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
        }
        // Every embedded picture has a relationship, and every relationship a part.
        let document = text_of(&parts, "word/document.xml");
        let rels = text_of(&parts, "word/_rels/document.xml.rels");
        let rels = roxmltree::Document::parse(&rels).unwrap();
        let targets: Vec<(String, String)> = rels
            .descendants()
            .filter(|n| n.has_tag_name("Relationship"))
            .map(|n| {
                (
                    n.attribute("Id").unwrap().to_owned(),
                    n.attribute("Target").unwrap().to_owned(),
                )
            })
            .collect();
        let doc = roxmltree::Document::parse(&document).unwrap();
        let embeds: Vec<&str> = doc
            .descendants()
            .filter(|n| n.tag_name().name() == "blip")
            .map(|n| {
                n.attribute((
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                    "embed",
                ))
                .unwrap()
            })
            .collect();
        assert_eq!(embeds, ["rIdImage1", "rIdImage2"]);
        for id in embeds {
            let (_, target) = targets.iter().find(|(i, _)| i == id).unwrap();
            assert!(
                names.contains(&format!("word/{target}").as_str()),
                "{target}"
            );
        }
        for (_, target) in &targets {
            assert!(
                names.contains(&format!("word/{target}").as_str()),
                "{target}"
            );
        }
        // The pictures are the PNG files given.
        let (_, png) = parts
            .iter()
            .find(|(n, _)| n == "word/media/image2.png")
            .unwrap();
        assert_eq!(png, &crate::document::tests::tiny_png());
    }

    #[test]
    fn the_body_has_the_title_the_header_the_list_and_a_page_per_note() {
        let two = [sample(), sample()];
        let parts = parts(&to_docx(&two, &labels()).unwrap());
        let document = text_of(&parts, "word/document.xml");
        let doc = roxmltree::Document::parse(&document).unwrap();
        let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
        let body = doc
            .descendants()
            .find(|n| n.has_tag_name((w, "body")))
            .unwrap();
        let paragraphs: Vec<_> = body
            .children()
            .filter(|n| n.has_tag_name((w, "p")))
            .collect();
        let text = |p: &roxmltree::Node| -> String {
            p.descendants()
                .filter(|n| n.has_tag_name((w, "t")))
                .filter_map(|n| n.text())
                .collect()
        };
        let style = |p: &roxmltree::Node| -> Option<String> {
            p.descendants()
                .find(|n| n.has_tag_name((w, "pStyle")))
                .and_then(|n| n.attribute((w, "val")))
                .map(str::to_owned)
        };
        // Per note: title, date, tags, paragraph, 2 items, picture, paragraph.
        assert_eq!(paragraphs.len(), 16);
        assert_eq!(text(&paragraphs[0]), "Réunion & *plan* #1");
        assert_eq!(style(&paragraphs[0]).as_deref(), Some("Title"));
        assert_eq!(text(&paragraphs[1]), "Created: 2026-10-22 14:05");
        assert_eq!(style(&paragraphs[1]).as_deref(), Some("Subtitle"));
        assert_eq!(text(&paragraphs[2]), "Tags: école, classe 3e");
        assert_eq!(
            text(&paragraphs[3]),
            "Premier paragraphe avec_des [signes] <spéciaux>."
        );
        assert_eq!(style(&paragraphs[4]).as_deref(), Some("ListParagraph"));
        assert_eq!(text(&paragraphs[5]), "- poires");
        assert!(
            paragraphs[6]
                .descendants()
                .any(|n| n.has_tag_name((w, "drawing")))
        );
        // Only the second note's title breaks the page.
        let breaks: Vec<bool> = paragraphs
            .iter()
            .map(|p| {
                p.descendants()
                    .any(|n| n.has_tag_name((w, "pageBreakBefore")))
            })
            .collect();
        assert_eq!(breaks.iter().filter(|&&b| b).count(), 1);
        assert!(breaks[8]);
        // The list uses the bullet numbering.
        assert!(document.contains("<w:numId w:val=\"1\"/>"));
        assert!(text_of(&parts, "word/numbering.xml").contains("w:numFmt w:val=\"bullet\""));
        assert!(
            text_of(&parts, "docProps/core.xml")
                .contains("<dc:title>Réunion &amp; *plan* #1</dc:title>")
        );
    }

    #[test]
    fn marked_words_are_underlined_or_struck_in_their_own_runs() {
        let text = Text::from_words([
            ("la", Mark::None),
            ("lumière", Mark::Underline),
            ("se", Mark::Underline),
            ("propage", Mark::None),
            ("<vite>", Mark::Strike),
        ]);
        let xml = marked_paragraph("", &text);
        let w = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
        let wrapped = format!("<w:root xmlns:w=\"{w}\">{xml}</w:root>");
        let doc = roxmltree::Document::parse(&wrapped).unwrap();
        let runs: Vec<(String, &str)> = doc
            .descendants()
            .filter(|n| n.has_tag_name((w, "r")))
            .map(|run| {
                let text: String = run
                    .descendants()
                    .filter(|n| n.has_tag_name((w, "t")))
                    .filter_map(|n| n.text())
                    .collect();
                let mark = if run.descendants().any(|n| n.has_tag_name((w, "u"))) {
                    "u"
                } else if run.descendants().any(|n| n.has_tag_name((w, "strike"))) {
                    "s"
                } else {
                    ""
                };
                (text, mark)
            })
            .collect();
        assert_eq!(
            runs,
            [
                ("la".to_owned(), ""),
                (" ".to_owned(), ""),
                ("lumière se".to_owned(), "u"),
                (" propage".to_owned(), ""),
                (" ".to_owned(), ""),
                ("<vite>".to_owned(), "s"),
            ]
        );
        // The paragraph reads as the plain text.
        let all: String = runs.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(all, text.to_string());
        assert_eq!(marked_paragraph("", &Text::default()), "<w:p></w:p>");
    }

    #[test]
    fn pictures_keep_their_size_and_proportions_within_the_page() {
        let drawing = |w: f32, h: f32| Drawing {
            png: crate::document::tests::tiny_png(),
            width_px: 10,
            height_px: 10,
            width_mm: w,
            height_mm: h,
        };
        // Small: real size.
        assert_eq!(picture_size(&drawing(60.0, 30.0)), (2_160_000, 1_080_000));
        // Wider than the text: reduced to 170 mm, same proportions.
        let (cx, cy) = picture_size(&drawing(340.0, 170.0));
        assert_eq!(cx, 6_120_000);
        assert_eq!(cy, 3_060_000);
        // Taller than a page: reduced to 240 mm.
        let (cx, cy) = picture_size(&drawing(100.0, 480.0));
        assert_eq!(cy, 8_640_000);
        assert_eq!(cx, 1_800_000);
        // Degenerate sizes still give a picture.
        let (cx, cy) = picture_size(&drawing(0.0, 0.0));
        assert!(cx > 0 && cy > 0);
    }

    #[test]
    fn a_note_without_body_or_header_is_just_its_title() {
        let bare = Document {
            title: "Vide".into(),
            date: String::new(),
            tags: Vec::new(),
            blocks: Vec::new(),
        };
        let parts = parts(&to_docx(&[bare], &labels()).unwrap());
        assert_eq!(parts.len(), 7, "no picture");
        let document = text_of(&parts, "word/document.xml");
        assert_eq!(document.matches("<w:p>").count(), 1);
        roxmltree::Document::parse(&document).unwrap();
    }
}
