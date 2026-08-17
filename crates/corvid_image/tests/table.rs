//! The table resolves a uv to the tile closed-form arithmetic says it should.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "by_hand deliberately repeats the crate's own narrowing so that the two can be compared; every value it casts is a texel index under 1024"
)]

use corvid_image::{
    PixelFormat, Residency, SourceId, SourceView, Sources, TileConfig, TileEntry, TileKey,
    TilePlanner, TileSlot, TileTable, VramBudget, extent,
};

const CONFIG: TileConfig = TileConfig::MIN_SPEC;

/// One 1024-texel plate: four pages a side, and a pyramid three levels deep.
fn plate() -> Sources {
    let mut sources = Sources::new();
    sources
        .push(&CONFIG, extent(1024, 1024), PixelFormat::SRGB8)
        .expect("a plate inside the ceiling");
    sources
}

/// What the shader is supposed to compute, written out with no reference to the
/// implementation: scale, shift, look up, shift again.
fn by_hand(
    table: &TileTable,
    source: SourceId,
    uv: [f32; 2],
    side: u32,
) -> (TileSlot, u8, [u32; 2]) {
    let texel = [
        ((uv[0] * side as f32) as u32).min(side - 1),
        ((uv[1] * side as f32) as u32).min(side - 1),
    ];
    let tile = CONFIG.tile_size;
    let entry = table.entry(source, texel[0] / tile, texel[1] / tile);
    let slot = entry.slot().expect("a resident tile covers this page");
    let level = u32::from(entry.level());
    let offset = entry.offset();
    (
        slot,
        entry.level(),
        [
            (u32::from(offset[0]) * tile + texel[0] % tile) / (1 << level),
            (u32::from(offset[1]) * tile + texel[1] % tile) / (1 << level),
        ],
    )
}

/// Every level-zero page resident: the entry is the tile the page is in, and
/// the texel inside it is the texel's own remainder.
#[test]
fn a_uv_resolves_to_the_tile_the_arithmetic_names() {
    let sources = plate();
    let source = SourceId(0);
    let mut resident = Residency::new();
    for y in 0..4u16 {
        for x in 0..4u16 {
            resident.insert(TileKey::new(source, 0, x, y), TileSlot(y * 4 + x));
        }
    }
    let table = TileTable::build(&CONFIG, &sources, &resident, &[0]);
    assert_eq!(table.side(), 4);
    assert_eq!(table.layers(), 1);
    assert_eq!(table.words().len(), 16);

    // 0.6 * 1024 is 614, which is page 2 and texel 102 inside it; 0.3 * 1024 is
    // 307, which is page 1 and texel 51. Written out rather than computed, so
    // that a change to the addressing has to change this line too.
    let sample = table.resolve(source, [0.6, 0.3]).expect("a covered uv");
    assert_eq!(sample.level, 0);
    assert_eq!(sample.texel, [102, 51]);
    // Row one, column two of the four-by-four grid of level-zero tiles.
    assert_eq!(sample.slot, TileSlot(6));

    for uv in [[0.0, 0.0], [0.5, 0.5], [0.6, 0.3], [0.999, 0.001]] {
        let sample = table.resolve(source, uv).expect("a covered uv");
        let (slot, level, texel) = by_hand(&table, source, uv, 1024);
        assert_eq!(
            (sample.slot, sample.level, sample.texel),
            (slot, level, texel)
        );
    }
}

/// The whole of the fallback, in one table: nothing but the root is resident,
/// so every page is served by it and every texel is scaled down by the four
/// levels between them.
#[test]
fn a_zoom_that_is_not_resident_answers_the_next_coarser_one() {
    let sources = plate();
    let source = SourceId(0);
    let mut resident = Residency::new();
    // Level two is the top of a 1024-texel plate in 256-texel tiles: one tile
    // holding the whole thing.
    resident.insert(TileKey::new(source, 2, 0, 0), TileSlot(9));

    let table = TileTable::build(&CONFIG, &sources, &resident, &[0]);

    let sample = table.resolve(source, [0.6, 0.3]).expect("a covered uv");
    assert_eq!(sample.slot, TileSlot(9));
    // Asked for level zero and answered level two, without an error and without
    // a hole.
    assert_eq!(sample.level, 2);
    // Texel 614 of 1024 is texel 153 of the 256-texel level-two tile, and 307
    // is 76. Integer division, the same one the shader's shift does.
    assert_eq!(sample.texel, [614 / 4, 307 / 4]);

    // And a middle level is preferred to the root when it is there.
    resident.insert(TileKey::new(source, 1, 1, 0), TileSlot(3));
    let table = TileTable::build(&CONFIG, &sources, &resident, &[0]);
    let sample = table.resolve(source, [0.6, 0.3]).expect("a covered uv");
    assert_eq!((sample.slot, sample.level), (TileSlot(3), 1));
    assert_eq!(sample.texel, [614 / 2 - 256, 307 / 2]);

    // A page the level-one tile does not cover still falls through to the root.
    let elsewhere = table.resolve(source, [0.1, 0.9]).expect("a covered uv");
    assert_eq!((elsewhere.slot, elsewhere.level), (TileSlot(9), 2));
}

/// A page with nothing resident above it at any zoom is absent rather than
/// pointing at whatever happens to be in slot zero.
#[test]
fn a_page_with_nothing_resident_is_absent() {
    let sources = plate();
    let table = TileTable::build(&CONFIG, &sources, &Residency::new(), &[0]);
    assert_eq!(table.resolve(SourceId(0), [0.5, 0.5]), None);
    assert_eq!(table.entry(SourceId(0), 0, 0), TileEntry::ABSENT);
    assert!(
        table
            .words()
            .iter()
            .all(|word| *word == TileEntry::ABSENT.bits())
    );
    // A source that was never registered has no layer at all.
    assert_eq!(table.resolve(SourceId(7), [0.5, 0.5]), None);
}

/// The packing is the contract with the shader, so it is frozen as bits rather
/// than as accessors that could move together.
#[test]
fn an_entry_is_the_word_the_shader_reads() {
    let entry = TileEntry::new(TileSlot(0x123), 5, [0x2a, 0x7f]);
    // Low twelve bits slot, next four level, then eight and eight of offset.
    assert_eq!(entry.bits(), 0x7f2a_5123_u32.cast_signed());
    assert_eq!(entry.slot(), Some(TileSlot(0x123)));
    assert_eq!(entry.level(), 5);
    assert_eq!(entry.offset(), [0x2a, 0x7f]);
    assert!(entry.is_present());

    assert_eq!(TileEntry::ABSENT.slot(), None);
    assert!(!TileEntry::ABSENT.is_present());
    assert_eq!(
        TileEntry::from_bits(TileEntry::ABSENT.bits()),
        TileEntry::ABSENT
    );
}

/// The relationship between levels is a shift and a mask, and the table's
/// offset field is that mask -- which is what makes the two agree.
#[test]
fn a_key_at_a_coarser_level_is_a_shift() {
    let (column, row) = (0x25u16, 0x0bu16);
    let key = TileKey::new(SourceId(2), 0, column, row);
    for level in 0..=8u8 {
        let coarse = key.at_level(level);
        let mask = (1u16 << level) - 1;
        assert_eq!(coarse.x, column >> level);
        assert_eq!(coarse.y, row >> level);
        assert_eq!(coarse.level, level);
        assert_eq!(coarse.source, key.source);
        assert_eq!(key.offset_in(level), [column & mask, row & mask]);
    }
}

/// The end-to-end claim, through a planner rather than a hand-built residency:
/// what the plan says is resident is what the table points at.
#[test]
fn the_table_agrees_with_the_residency_the_plan_produced() {
    let mut planner = TilePlanner::new(CONFIG).expect("the minimum specification");
    let plate = planner
        .register(extent(4096, 2048), PixelFormat::SRGBA8)
        .expect("a plate");
    let plan = planner.plan(&[SourceView::full(plate)], VramBudget::MIN_SPEC);
    planner.commit(&plan);

    for uv in [[0.0, 0.0], [0.25, 0.75], [0.5, 0.5], [0.99, 0.99]] {
        let sample = plan.table().resolve(plate, uv).expect("a covered uv");
        let held = plan
            .residency()
            .iter()
            .find(|(_, slot)| *slot == sample.slot)
            .expect("the slot holds something");
        assert_eq!(held.0.level, sample.level);
        assert_eq!(held.0.source, plate);
        assert!(sample.texel[0] < CONFIG.tile_size && sample.texel[1] < CONFIG.tile_size);
    }
}
