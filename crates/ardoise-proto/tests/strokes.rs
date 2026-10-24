//! Real stroke data: the three Intuos Pro page files of Tuhi's test suite (see `common`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use ardoise_proto::stroke::{
    self, FileFormat, PenInfo, Point, Stroke, StrokeFile, StrokeTime, encode_slate,
};

fn decode_blob(index: usize) -> StrokeFile {
    stroke::decode(&common::tuhi_blobs()[index]).unwrap()
}

fn range(points: impl Iterator<Item = u16> + Clone) -> (u16, u16) {
    (points.clone().min().unwrap(), points.max().unwrap())
}

#[test]
fn blob_sizes_match_tuhi() {
    let sizes = common::tuhi_blobs().map(|blob| blob.len());
    assert_eq!(sizes, [505, 345, 981]);
}

#[test]
fn every_blob_decodes_to_the_end_with_the_announced_stroke_count() {
    for (index, expected) in [(0, 2), (1, 2), (2, 5)] {
        let file = decode_blob(index);
        assert_eq!(file.strokes.len(), expected, "blob {index}");
        let FileFormat::IntuosPro { stroke_count, .. } = file.format else {
            panic!("blob {index} is not an Intuos Pro file");
        };
        assert_eq!(stroke_count as usize, expected);
        assert_eq!(file.unknown_packets, 0);
        assert!(!file.eof);
        assert_eq!(file.trailing_bytes, 0);
        for stroke in &file.strokes {
            assert!(!stroke.points.is_empty());
            assert_eq!(stroke.lost_points, 0);
            assert!(matches!(stroke.time, StrokeTime::Unix { .. }));
        }
    }
}

#[test]
fn single_stroke_blob_matches_the_worked_example() {
    let file = decode_blob(0);
    assert_eq!(
        file.format,
        FileFormat::IntuosPro {
            seconds: 1_565_750_050,
            millis: 0,
            stroke_count: 2
        }
    );
    let counts: Vec<usize> = file.strokes.iter().map(|s| s.points.len()).collect();
    assert_eq!(counts, [36, 90]);

    let all = file.strokes.iter().flat_map(|s| s.points.iter());
    assert_eq!(range(all.clone().map(|p| p.x)), (15_398, 28_627));
    assert_eq!(range(all.clone().map(|p| p.y)), (12_306, 20_549));
    assert_eq!(range(all.map(|p| p.pressure)), (404, 4001));

    let first = &file.strokes[0];
    assert_eq!(
        first.time,
        StrokeTime::Unix {
            seconds: 1_565_750_047,
            subsecond: 302
        }
    );
    assert_eq!(
        first.pen,
        Some(PenInfo {
            pen_type: 3,
            new_layer: true,
            pen_id: Some(0x1000_4228_8075_870a)
        })
    );
    let expected = [
        (15_425, 12_307, 882),
        (15_426, 12_307, 1227),
        (15_426, 12_308, 1551),
        (15_426, 12_308, 1887),
        (15_426, 12_307, 2193),
        (15_426, 12_306, 2424),
        (15_428, 12_306, 2607),
    ]
    .map(|(x, y, pressure)| Point { x, y, pressure });
    assert_eq!(first.points[..expected.len()], expected);

    let second = &file.strokes[1];
    assert_eq!(
        second.time,
        StrokeTime::Unix {
            seconds: 1_565_750_049,
            subsecond: 582
        }
    );
    assert_eq!(
        second.pen,
        Some(PenInfo {
            pen_type: 3,
            new_layer: false,
            pen_id: None
        })
    );
    assert_eq!(second.points[0].x, 0x6fd3);
}

#[test]
fn real_points_survive_a_slate_re_encoding() {
    for index in 0..3 {
        let file = decode_blob(index);
        let strokes: Vec<Stroke> = file
            .strokes
            .iter()
            .enumerate()
            .map(|(i, s)| Stroke {
                time: StrokeTime::SincePowerUp {
                    millis: 5 * u32::try_from(i).unwrap(),
                },
                pen: None,
                points: s.points.clone(),
                lost_points: 0,
            })
            .collect();
        let encoded = encode_slate(&strokes);
        let decoded = stroke::decode(&encoded).unwrap();
        assert_eq!(decoded.format, FileFormat::Slate);
        assert_eq!(decoded.strokes, strokes, "blob {index}");
    }
}

#[test]
fn every_prefix_of_a_blob_decodes_or_fails_cleanly() {
    for blob in common::tuhi_blobs() {
        for len in 0..blob.len() {
            let _ = stroke::decode(&blob[..len]);
        }
    }
}
