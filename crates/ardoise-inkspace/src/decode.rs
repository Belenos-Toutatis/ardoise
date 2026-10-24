//! Walking the note tree (`__mate__` → documents → pages → layers → inline strokes) and
//! decoding the WILL-2.0-style stroke payloads (research §4.1, §5, §8).

use crate::DecodeError;
use crate::protobuf;
use crate::store::{CloudDb, NodeId, NodeRef, Props, parse_child_list};

/// Largest accepted `decimalPrecision` (values are integers × 10^p).
const MAX_PRECISION: u64 = 9;

/// One Inkspace document (a note), as stored.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DocumentData {
    pub id: NodeId,
    pub props: Props,
    pub pages: Vec<PageData>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageData {
    pub props: Props,
    pub layers: Vec<LayerData>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct LayerData {
    pub strokes: Vec<InkStroke>,
}

/// One decoded stroke, in Inkspace stroke space (page units, before the page transform).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InkStroke {
    /// Catmull-Rom control points; the first and last ones are normally duplicates.
    pub points: Vec<[f64; 2]>,
    /// Diameter at each control point, page units.
    pub widths: Vec<f64>,
    /// `0xRRGGBBAA`.
    pub rgba: u32,
    /// `pointsPerSecond` property, if present and parseable.
    pub points_per_second: Option<f64>,
    /// `creationDate` property (Unix ms), if present and parseable.
    pub creation_date: Option<i64>,
}

/// A document that could not be decoded.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DocumentFailure {
    pub id: NodeId,
    pub error: DecodeError,
}

/// Every document under the root, in stored order. Documents that fail to decode are
/// reported separately so the others still import.
pub(crate) fn documents(
    db: &CloudDb,
) -> Result<(Vec<DocumentData>, Vec<DocumentFailure>), DecodeError> {
    let root = NodeId::Name(NodeId::ROOT.into());
    let Some(root_doc) = db.doc(&root) else {
        // An empty store has no tree at all; a non-empty store without a root is not one
        // we understand.
        return if db.docs.is_empty() {
            Ok((Vec::new(), Vec::new()))
        } else {
            Err(DecodeError::MissingNode(NodeId::ROOT.into()))
        };
    };
    let mut docs = Vec::new();
    let mut failures = Vec::new();
    for child in parse_child_list(root_doc)? {
        let NodeRef::Node(id) = child.node else {
            continue;
        };
        if !node_type_is(&child.props, "document") {
            continue;
        }
        match document(db, &id, child.props) {
            Ok(doc) => docs.push(doc),
            Err(error) => failures.push(DocumentFailure { id, error }),
        }
    }
    Ok((docs, failures))
}

/// `nodeType` equals `expected`, or is absent.
fn node_type_is(props: &Props, expected: &str) -> bool {
    props.get("nodeType").is_none_or(|t| t == expected)
}

fn document(db: &CloudDb, id: &NodeId, props: Props) -> Result<DocumentData, DecodeError> {
    let doc = db
        .doc(id)
        .ok_or_else(|| DecodeError::MissingNode(id.to_string()))?;
    let mut pages = Vec::new();
    for child in parse_child_list(doc)? {
        let NodeRef::Node(page_id) = child.node else {
            continue;
        };
        if !node_type_is(&child.props, "page") {
            continue;
        }
        let page_doc = db
            .doc(&page_id)
            .ok_or_else(|| DecodeError::MissingNode(page_id.to_string()))?;
        let mut layers = Vec::new();
        for layer in parse_child_list(page_doc)? {
            let NodeRef::Node(layer_id) = layer.node else {
                continue;
            };
            if !node_type_is(&layer.props, "layer") {
                continue;
            }
            // A layer without a doc has no strokes.
            let strokes = match db.doc(&layer_id) {
                Some(layer_doc) => layer_strokes(layer_doc)?,
                None => Vec::new(),
            };
            layers.push(LayerData { strokes });
        }
        pages.push(PageData {
            props: child.props,
            layers,
        });
    }
    Ok(DocumentData {
        id: id.clone(),
        props,
        pages,
    })
}

fn layer_strokes(layer_doc: &[u8]) -> Result<Vec<InkStroke>, DecodeError> {
    let mut strokes = Vec::new();
    for child in parse_child_list(layer_doc)? {
        let NodeRef::Inline(payload) = child.node else {
            continue;
        };
        let index = strokes.len();
        let stroke = decode_stroke(payload, &child.props).map_err(|error| DecodeError::Stroke {
            index,
            reason: Box::new(error),
        })?;
        strokes.push(stroke);
    }
    Ok(strokes)
}

/// Decodes one stroke payload (a varint-length-prefixed WILL path message, research §5).
pub(crate) fn decode_stroke(payload: &[u8], props: &Props) -> Result<InkStroke, DecodeError> {
    let message = protobuf::length_delimited(payload)?;
    let fields = protobuf::parse_message(message)?;
    let precision = protobuf::varint_field(&fields, 3).unwrap_or(2);
    if precision > MAX_PRECISION {
        return Err(DecodeError::Invalid {
            what: "decimal precision",
            value: precision.to_string(),
        });
    }
    let scale = 10f64.powi(precision as i32);

    let xy = protobuf::repeated_sint(&fields, 4)?;
    if xy.len() % 2 != 0 {
        return Err(DecodeError::Invalid {
            what: "coordinate count",
            value: xy.len().to_string(),
        });
    }
    let (mut x, mut y) = (0i64, 0i64);
    let mut points = Vec::with_capacity(xy.len() / 2);
    for &[dx, dy] in xy.as_chunks::<2>().0 {
        x = x.checked_add(dx).ok_or(DecodeError::Overflow)?;
        y = y.checked_add(dy).ok_or(DecodeError::Overflow)?;
        points.push([x as f64 / scale, y as f64 / scale]);
    }

    let mut w = 0i64;
    let mut widths = Vec::new();
    for delta in protobuf::repeated_sint(&fields, 5)? {
        w = w.checked_add(delta).ok_or(DecodeError::Overflow)?;
        widths.push(w as f64 / scale);
    }
    if widths.len() != points.len() {
        return Err(DecodeError::Invalid {
            what: "width count",
            value: format!("{} widths for {} points", widths.len(), points.len()),
        });
    }

    // Opaque black when absent; the colour is one int32 0xRRGGBBAA (low 32 bits).
    let rgba = protobuf::repeated_sint(&fields, 6)?
        .first()
        .map_or(0x0000_00ff, |&c| c as u32);

    Ok(InkStroke {
        points,
        widths,
        rgba,
        points_per_second: props
            .get("pointsPerSecond")
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite()),
        creation_date: props
            .get("creationDate")
            .and_then(|s| s.trim().parse::<i64>().ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first bytes of the research §5 example, completed with one more point.
    fn example_payload() -> Vec<u8> {
        let mut msg = vec![
            0x0d, 0, 0, 0, 0, // f1 = 0.0
            0x15, 0, 0, 0x80, 0x3f, // f2 = 1.0
            0x18, 0x02, // precision 2
        ];
        // f4: x,y deltas: (80313 zz -> -40157?) use plain values: (100, 200), (+4, -2)
        let xy = [200u8, 1, 144, 3, 8, 3]; // zigzag(100)=200 -> 0xc8 0x01, zigzag(200)=400 -> 0x90 0x03, 8 -> 4, 3 -> -2
        msg.push(0x22);
        msg.push(xy.len() as u8);
        msg.extend_from_slice(&xy);
        // f5: widths 1.50, 2.10 -> 150, +60
        let w = [0xac, 0x02, 0x78];
        msg.push(0x2a);
        msg.push(w.len() as u8);
        msg.extend_from_slice(&w);
        // f6: colour 0x51A7F9FF
        msg.extend_from_slice(&[0x32, 0x05, 0xfe, 0xe7, 0xbf, 0x9a, 0x0a]);
        msg.extend_from_slice(&[0x48, 0x02]);
        let mut payload = vec![msg.len() as u8];
        payload.extend_from_slice(&msg);
        payload
    }

    #[test]
    fn decodes_a_stroke() {
        let props = Props::from_pairs(&[
            ("pointsPerSecond", "200"),
            ("creationDate", "1700000017515"),
        ]);
        let stroke = decode_stroke(&example_payload(), &props).unwrap();
        assert_eq!(stroke.points, vec![[1.0, 2.0], [1.04, 1.98]]);
        assert_eq!(stroke.widths, vec![1.5, 2.1]);
        assert_eq!(stroke.rgba, 0x51A7_F9FF);
        assert_eq!(stroke.points_per_second, Some(200.0));
        assert_eq!(stroke.creation_date, Some(1_700_000_017_515));
        let untimed = decode_stroke(&example_payload(), &Props::default()).unwrap();
        assert_eq!(untimed.points_per_second, None);
        assert_eq!(untimed.creation_date, None);
    }

    #[test]
    fn malformed_strokes_fail_cleanly() {
        let props = Props::default();
        let good = example_payload();
        // Every truncation fails with an error (never a panic).
        for len in 0..good.len() {
            assert!(decode_stroke(&good[..len], &props).is_err(), "len {len}");
        }
        // Odd coordinate count.
        let odd = [0x03, 0x22, 0x01, 0x02];
        assert!(matches!(
            decode_stroke(&odd, &props),
            Err(DecodeError::Invalid { .. })
        ));
        // Widths missing.
        let no_widths = [0x04, 0x22, 0x02, 0x02, 0x02];
        assert!(matches!(
            decode_stroke(&no_widths, &props),
            Err(DecodeError::Invalid { .. })
        ));
        // Precision too large.
        assert!(matches!(
            decode_stroke(&[0x02, 0x18, 0x63], &props),
            Err(DecodeError::Invalid { .. })
        ));
        // Accumulated deltas overflowing i64.
        // x: i64::MAX (zigzag u64::MAX - 1), then +1.
        let packed = [
            0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x02, 0x00,
        ];
        let mut framed = vec![packed.len() as u8 + 2, 0x22, packed.len() as u8];
        framed.extend_from_slice(&packed);
        assert_eq!(decode_stroke(&framed, &props), Err(DecodeError::Overflow));
        // An empty message is a stroke without points.
        let empty = decode_stroke(&[0x00], &props).unwrap();
        assert!(empty.points.is_empty() && empty.rgba == 0xff);
    }
}
