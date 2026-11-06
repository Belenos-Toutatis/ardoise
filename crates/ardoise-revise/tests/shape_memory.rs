//! A shape with an input array must not cost memory before it is counted: a curve's vertices are
//! hundreds of times its points, and a list of orders comes from an assistant. Measured with a
//! counting allocator, alone in this test binary so that nothing else allocates meanwhile.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_revise::{Input, Order, Shape, apply};

/// Bytes allocated now, and the most since it was last reset.
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: every call goes straight to the system's allocator, which keeps its contract; the
// counters only read the sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(now, Ordering::Relaxed);
        // SAFETY: the same layout, as the caller promised.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: the same pointer and layout, as the caller promised.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// How much more than what is allocated now `work` needs at its worst, bytes.
fn extra_memory(work: impl FnOnce()) -> usize {
    let before = NOW.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    work();
    PEAK.load(Ordering::Relaxed).saturating_sub(before)
}

fn add(shape: Shape) -> Order {
    Order::Add {
        shapes: vec![shape],
        color: "#000000".to_owned(),
        width: None,
        tool: None,
        layer: None,
        behind: false,
        after: None,
    }
}

/// `count` points, each stretch 500 mm long: 400 pieces of curve each.
fn long_stretches(count: usize) -> Vec<[f32; 2]> {
    (0..count)
        .map(|i| [if i % 2 == 0 { 0.0 } else { 500.0 }, 0.0])
        .collect()
}

#[test]
fn a_curve_is_refused_without_the_memory_of_its_vertices() {
    const LIMIT: usize = 8 << 20;
    let strokes = vec![Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 50.0,
        layer: 0,
        points: vec![Point {
            x: 0.0,
            y: 0.0,
            pressure: 0.5,
            t: 0,
        }],
    }];
    let input = Input {
        strokes: &strokes,
        page_mm: [300.0, 200.0],
        layers: 1,
        visible_layers: &[],
        narration: false,
    };
    // 1,000,000 points: 3.2 GB of vertices. 100,000 points, within the limit on the points: 320 MB.
    // 3,000 points, the smallest list that goes over the limit on samples: 9.6 MB.
    for count in [1_000_000, 100_000, 3_000] {
        let orders = [add(Shape::Curve {
            points: long_stretches(count),
        })];
        let mut outcome = None;
        let used = extra_memory(|| outcome = Some(apply(&input, &orders).map(|_| ())));
        assert!(outcome.unwrap().is_err(), "{count} points were accepted");
        assert!(used < LIMIT, "{count} points took {used} bytes");
    }
}
