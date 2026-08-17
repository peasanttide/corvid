//! A budget smaller than the working set evicts the least valuable tile, says
//! which, and leaves a picture that is coarse rather than absent.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_image::{
    PixelFormat, SourceId, SourceView, Tier, TileConfig, TileKey, TilePlan, TilePlanner, UvRect,
    VramBudget, extent,
};

const CONFIG: TileConfig = TileConfig::MIN_SPEC;

/// One 256-texel tile of a three-channel plate.
const TILE: u64 = 256 * 256 * 3;

/// A planner holding one 1024-texel plate: sixteen level-zero tiles, four at
/// level one, and one root at level two. Twenty-one in the whole pyramid.
fn plate() -> (TilePlanner, SourceId) {
    let mut planner = TilePlanner::new(CONFIG).expect("the minimum specification");
    let id = planner
        .register(extent(1024, 1024), PixelFormat::SRGB8)
        .expect("a plate inside the ceiling");
    (planner, id)
}

/// How many tiles of each of the plate's three levels the plan holds.
fn levels(plan: &TilePlan) -> [usize; 3] {
    let mut counts = [0usize; 3];
    for (key, _) in plan.residency().iter() {
        counts[usize::from(key.level)] += 1;
    }
    counts
}

/// With room for the whole pyramid, the whole pyramid arrives and nothing is
/// thrown away.
#[test]
fn a_budget_that_fits_evicts_nothing() {
    let (mut planner, plate) = plate();
    let plan = planner.plan(&[SourceView::full(plate)], VramBudget::new(TILE * 64));

    assert_eq!(plan.wanted(), 21);
    assert_eq!(plan.uploads().len(), 21);
    assert!(plan.evictions().is_empty());
    assert!(!plan.is_degraded());
    assert_eq!(levels(&plan), [16, 4, 1]);

    // The most valuable thing in the plan is the root, and it is first.
    assert_eq!(plan.uploads()[0].key, TileKey::new(plate, 2, 0, 0));
    assert_eq!(plan.uploads()[0].priority.tier, Tier::Root);

    // Nothing has happened to the planner until it is told so.
    assert!(planner.residency().is_empty());
    planner.commit(&plan);
    assert_eq!(planner.residency().len(), 21);

    // And a second plan against the same view is steady: everything wanted is
    // already there, so there is nothing to do.
    let again = planner.plan(&[SourceView::full(plate)], VramBudget::new(TILE * 64));
    assert!(again.uploads().is_empty());
    assert!(again.evictions().is_empty());
    assert_eq!(again.residency(), plan.residency());
}

/// The ordering that makes an overloaded streamer degrade: what a short budget
/// keeps is the coarse levels, and what it drops is the detail.
#[test]
fn a_budget_smaller_than_the_working_set_keeps_the_coarse_levels() {
    let (mut planner, plate) = plate();
    let plan = planner.plan(&[SourceView::full(plate)], VramBudget::new(TILE * 6));

    assert_eq!(plan.capacity(), 6);
    assert_eq!(plan.wanted(), 21);
    assert!(plan.is_degraded());
    assert_eq!(plan.uploads().len(), 6);
    // The root, all four of level one, and one solitary level-zero tile.
    assert_eq!(levels(&plan), [1, 4, 1]);

    planner.commit(&plan);
    // Degraded is not broken: every uv on the plate still resolves, at whatever
    // zoom survived.
    for uv in [[0.1, 0.1], [0.5, 0.5], [0.9, 0.9]] {
        let sample = plan.table().resolve(plate, uv).expect("a covered uv");
        assert!(sample.level <= 2);
    }
}

/// The eviction itself: the budget shrinks, and the plan names the tiles that
/// go and puts the least valuable one first.
#[test]
fn a_shrinking_budget_evicts_the_least_valuable_and_says_which() {
    let (mut planner, plate) = plate();
    let roomy = planner.plan(&[SourceView::full(plate)], VramBudget::new(TILE * 6));
    planner.commit(&roomy);
    assert_eq!(planner.residency().len(), 6);

    let cramped = planner.plan(&[SourceView::full(plate)], VramBudget::new(TILE * 3));
    assert_eq!(cramped.capacity(), 3);
    assert_eq!(cramped.evictions().len(), 3);

    // Least valuable first, and the least valuable thing resident is the one
    // level-zero tile: within a source, a coarser tile always outranks a finer
    // one, because the coarse one is what the fine one falls back to.
    let first = cramped.evictions()[0];
    assert_eq!(first.key.level, 0);
    assert_eq!(
        first.priority.map(|priority| priority.level),
        Some(0),
        "the tile is still wanted; it is the budget that cannot hold it"
    );
    for pair in cramped.evictions().windows(2) {
        assert!(pair[0].priority <= pair[1].priority);
    }

    // What is left is the root and the two lowest-keyed level-one tiles, and
    // every freed slot is one an upload could take.
    assert_eq!(levels(&cramped), [0, 2, 1]);
    assert!(cramped.uploads().is_empty());
    assert!(
        cramped
            .evictions()
            .iter()
            .all(|eviction| planner.residency().slot(eviction.key) == Some(eviction.slot))
    );
}

/// A budget of nothing is a plan of nothing rather than a panic or a plan that
/// quietly ignores the number it was given.
#[test]
fn a_budget_of_nothing_holds_nothing() {
    let (mut planner, plate) = plate();
    let plan = planner.plan(&[SourceView::full(plate)], VramBudget::new(0));
    assert_eq!(plan.capacity(), 0);
    assert!(plan.uploads().is_empty());
    assert!(plan.residency().is_empty());
    assert_eq!(plan.table().resolve(plate, [0.5, 0.5]), None);

    planner.commit(&plan);
    assert!(planner.residency().is_empty());
}

/// Two sources and a budget that cannot hold both: the second one still draws,
/// because its root outranks the first one's detail whatever the weights say.
#[test]
fn a_second_source_is_not_starved_to_a_hole() {
    let mut planner = TilePlanner::new(CONFIG).expect("the minimum specification");
    let front = planner
        .register(extent(4096, 4096), PixelFormat::SRGB8)
        .expect("the plate being read");
    let back = planner
        .register(extent(4096, 4096), PixelFormat::SRGB8)
        .expect("the plate underneath it");

    let views = [
        SourceView::full(front),
        SourceView {
            weight: 0.05,
            ..SourceView::full(back)
        },
    ];
    let plan = planner.plan(&views, VramBudget::new(TILE * 8));
    planner.commit(&plan);

    assert!(plan.is_degraded());
    assert!(
        plan.table().resolve(back, [0.5, 0.5]).is_some(),
        "the faint source keeps its root"
    );
    // And the loud one got the rest.
    let front_tiles = plan
        .residency()
        .iter()
        .filter(|(key, _)| key.source == front)
        .count();
    assert!(front_tiles > 1, "the weighted source got the detail");
}

/// A view of a corner asks for that corner, not for the whole plate.
#[test]
fn only_what_is_visible_is_asked_for() {
    let (planner, plate) = plate();
    let corner = SourceView {
        visible: UvRect::new([0.0, 0.0], [0.25, 0.25]),
        ..SourceView::full(plate)
    };
    let plan = planner.plan(&[corner], VramBudget::new(TILE * 64));

    // One level-zero tile, one level-one tile, one root: the corner's column of
    // the pyramid rather than all twenty-one tiles of it.
    assert_eq!(plan.wanted(), 3);
    assert!(
        plan.residency()
            .iter()
            .all(|(key, _)| key.x == 0 && key.y == 0)
    );
}

/// A minified view asks for the level its texels are actually seen at, not for
/// level zero and sixteen times the memory.
#[test]
fn a_minified_view_asks_for_a_coarser_level() {
    let (planner, plate) = plate();
    let far = SourceView {
        texels_per_pixel: 2.0,
        ..SourceView::full(plate)
    };
    let plan = planner.plan(&[far], VramBudget::new(TILE * 64));

    assert_eq!(
        plan.wanted(),
        5,
        "level two and level one, and nothing finer"
    );
    assert!(plan.residency().iter().all(|(key, _)| key.level >= 1));
}
