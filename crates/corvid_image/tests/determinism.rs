//! The same input answers the same plan, down to the slot numbers.
//!
//! This is the property the crate's data structures were chosen for. A
//! streamer that reshuffles under you is not a heuristic that sometimes picks
//! differently; it is a bug that only shows up as a frame that flickers on one
//! machine and not on another, and it is unfindable once it is allowed.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_image::{
    PixelFormat, SourceId, SourceView, TileConfig, TilePlanner, UvRect, VramBudget, extent,
};

const CONFIG: TileConfig = TileConfig::MIN_SPEC;
const TILE: u64 = 256 * 256 * 4;

fn planner() -> (TilePlanner, [SourceId; 3]) {
    let mut planner = TilePlanner::new(CONFIG).expect("the minimum specification");
    let sizes = [extent(8192, 4096), extent(1024, 1024), extent(3000, 5000)];
    let mut ids = [SourceId(0); 3];
    for (id, size) in ids.iter_mut().zip(sizes) {
        *id = planner
            .register(size, PixelFormat::SRGBA8)
            .expect("a plate inside the ceiling");
    }
    (planner, ids)
}

fn views(ids: [SourceId; 3]) -> [SourceView; 3] {
    [
        SourceView {
            visible: UvRect::new([0.1, 0.2], [0.7, 0.8]),
            texels_per_pixel: 1.5,
            weight: 1.0,
            source: ids[0],
        },
        SourceView {
            visible: UvRect::new([0.0, 0.0], [1.0, 1.0]),
            texels_per_pixel: 3.0,
            weight: 0.4,
            source: ids[1],
        },
        SourceView {
            visible: UvRect::new([0.4, 0.4], [0.6, 0.6]),
            texels_per_pixel: 1.0,
            weight: 0.9,
            source: ids[2],
        },
    ]
}

#[test]
fn the_same_call_twice_answers_the_same_plan() {
    let (planner, ids) = planner();
    let budget = VramBudget::new(TILE * 40);
    let first = planner.plan(&views(ids), budget);
    let second = planner.plan(&views(ids), budget);
    assert_eq!(first, second);
}

/// Two planners built the same way, with no shared state at all. This is the
/// version that would catch an address or an allocation order leaking into the
/// answer, which the repeated call on one planner cannot.
#[test]
fn two_planners_built_alike_answer_alike() {
    let (left, left_ids) = planner();
    let (right, right_ids) = planner();
    let budget = VramBudget::new(TILE * 40);
    assert_eq!(
        left.plan(&views(left_ids), budget),
        right.plan(&views(right_ids), budget)
    );
}

/// The views are a set, not a sequence. A caller that walks its lenses in a
/// different order this frame has not asked for a different picture.
#[test]
fn the_order_of_the_views_does_not_matter() {
    let (planner, ids) = planner();
    let budget = VramBudget::new(TILE * 40);
    let forwards = views(ids);
    let mut backwards = forwards;
    backwards.reverse();
    assert_eq!(
        planner.plan(&forwards, budget),
        planner.plan(&backwards, budget)
    );
}

/// Two views of one source fold together, and fold the same way whichever
/// arrives first: the union of the rectangles, the finer level, the greater
/// weight.
#[test]
fn two_views_of_one_source_merge_commutatively() {
    let (planner, ids) = planner();
    let budget = VramBudget::new(TILE * 40);
    let lens = SourceView {
        visible: UvRect::new([0.0, 0.0], [0.2, 0.2]),
        texels_per_pixel: 1.0,
        weight: 1.0,
        source: ids[0],
    };
    let minimap = SourceView {
        visible: UvRect::new([0.6, 0.6], [1.0, 1.0]),
        texels_per_pixel: 8.0,
        weight: 0.2,
        source: ids[0],
    };
    assert_eq!(
        planner.plan(&[lens, minimap], budget),
        planner.plan(&[minimap, lens], budget)
    );
}

/// Running the whole loop twice -- plan, commit, plan, commit -- lands in the
/// same place both times. A plan that depended on its own history would pass
/// every single-shot check above and still drift.
#[test]
fn a_committed_sequence_replays_identically() {
    let budget = [TILE * 40, TILE * 12, TILE * 400, TILE * 40];
    let run = || {
        let (mut planner, ids) = planner();
        let mut last = None;
        for bytes in budget {
            let plan = planner.plan(&views(ids), VramBudget::new(bytes));
            planner.commit(&plan);
            last = Some(plan);
        }
        (planner, last.expect("four budgets is at least one plan"))
    };
    let (left_planner, left_plan) = run();
    let (right_planner, right_plan) = run();
    assert_eq!(left_plan, right_plan);
    assert_eq!(left_planner, right_planner);
}

/// A weight that is not a number is not an ordering hazard. The rank is
/// quantised before anything compares it, so a `NaN` is the bottom of the pile
/// and the plan is still a plan.
#[test]
fn a_weight_that_is_not_a_number_does_not_disorder_the_plan() {
    let (planner, ids) = planner();
    let budget = VramBudget::new(TILE * 40);
    let broken = SourceView {
        weight: f32::NAN,
        texels_per_pixel: f32::NAN,
        visible: UvRect::new([f32::NAN, 0.0], [1.0, f32::NAN]),
        source: ids[0],
    };
    let first = planner.plan(&[broken], budget);
    let second = planner.plan(&[broken], budget);
    assert_eq!(first, second);
    // And it is a real plan rather than an empty one: the rectangle's NaN
    // corners were read as the edges they are nearest.
    assert!(!first.uploads().is_empty());
}
