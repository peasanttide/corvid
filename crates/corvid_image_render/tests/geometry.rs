//! What the shader will do, checked without a shader.
//!
//! Everything here is arithmetic that a fragment shader repeats: where a slot
//! is in the atlas, how many of them a budget pays for, and which bits of a
//! table entry mean what. A device is not needed to be wrong about any of it,
//! and `tests/device.rs` -- which does need one -- only runs where there is an
//! adapter. So these are the assertions that run everywhere.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use std::collections::BTreeSet;

use corvid_image::{
    MAX_NUM_MAPS, MAX_NUM_TILES, PixelFormat, TILE_SIZE, TileConfig, TileEntry, TileSlot,
    VramBudget,
};
use corvid_image_render::{
    Atlas, FLOOR, GROUP, MAX_SOURCES, WGSL, device_bytes_per_texel, resident_tiles, texture_format,
    wgsl_at,
};

/// The limits `corvid_render` opens a device with on a machine whose adapter
/// allows a 16384-texel texture: the downlevel baseline with the resolution
/// raised, which is `Limits::using_resolution` and is why the array layer count
/// stays at the baseline's 256.
fn opened_limits() -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults();
    limits.max_texture_dimension_2d = 16384;
    limits
}

/// The value of a `const NAME: u32 = <n>u;` in [`WGSL`].
fn wgsl_const(name: &str) -> u32 {
    let line = WGSL
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(&format!("const {name}:")))
        .unwrap_or_else(|| panic!("{name} is not declared in the shader"));
    let value = line
        .rsplit('=')
        .next()
        .unwrap()
        .trim()
        .trim_end_matches(';')
        .trim_end_matches('u');
    value.parse().expect("a decimal literal")
}

#[test]
fn the_minimum_specification_fits_a_device_this_workspace_opens() {
    // The whole reason a layer holds a grid rather than one tile: 2048 slots
    // out of a device that allows 256 array layers.
    let atlas = Atlas::plan(TILE_SIZE, MAX_NUM_TILES, &opened_limits()).unwrap();
    assert_eq!(atlas.slots(), MAX_NUM_TILES);
    assert!(atlas.layers() <= opened_limits().max_texture_array_layers);
    let (width, height) = atlas.layer_extent();
    assert!(width <= opened_limits().max_texture_dimension_2d);
    assert!(height <= opened_limits().max_texture_dimension_2d);
    assert_eq!(atlas.mip_levels(), TILE_SIZE.ilog2() + 1);
}

#[test]
fn every_slot_gets_a_rectangle_of_its_own() {
    // The property the whole packing rests on. Two slots sharing a rectangle is
    // one tile overwriting another with nothing anywhere saying so, and it is
    // the failure a grid-packed atlas has that one tile per layer does not.
    let atlas = Atlas::plan(TILE_SIZE, MAX_NUM_TILES, &opened_limits()).unwrap();
    let (width, height) = atlas.layer_extent();
    let mut seen = BTreeSet::new();
    for slot in 0..atlas.slots() {
        let (layer, origin) = atlas
            .locate(TileSlot(u16::try_from(slot).unwrap()))
            .expect("a slot below the count is a slot");
        assert!(layer < atlas.layers());
        assert!(origin[0] + TILE_SIZE <= width);
        assert!(origin[1] + TILE_SIZE <= height);
        assert!(
            seen.insert((layer, origin)),
            "slot {slot} lands where another already is",
        );
    }
    assert_eq!(seen.len(), usize::try_from(atlas.slots()).unwrap());
    assert_eq!(atlas.locate(TileSlot(u16::MAX)), None);
}

#[test]
fn a_slot_is_two_shifts_and_two_masks_the_way_the_shader_does_it() {
    // `Atlas::locate` and the WGSL are one piece of arithmetic written twice.
    // This is the second copy, spelled out from the two shifts the parameter
    // block carries rather than from `locate`'s own fields.
    let atlas = Atlas::plan(TILE_SIZE, MAX_NUM_TILES, &opened_limits()).unwrap();
    let (across_shift, layer_shift) = atlas.shifts();
    assert_eq!(1 << across_shift, atlas.tiles_across());
    assert_eq!(1 << layer_shift, atlas.per_layer());

    for slot in (0..atlas.slots()).step_by(37) {
        let cell = slot & ((1 << layer_shift) - 1);
        let column = cell & ((1 << across_shift) - 1);
        let row = cell >> across_shift;
        assert_eq!(
            atlas.locate(TileSlot(u16::try_from(slot).unwrap())),
            Some((slot >> layer_shift, [column * TILE_SIZE, row * TILE_SIZE])),
        );
    }
}

#[test]
fn a_small_device_answers_fewer_slots_rather_than_a_texture_it_cannot_make() {
    // A tile larger than a texture is the one case with no smaller answer.
    let mut tiny = wgpu::Limits::downlevel_defaults();
    tiny.max_texture_dimension_2d = 128;
    assert_eq!(Atlas::plan(TILE_SIZE, MAX_NUM_TILES, &tiny), None);

    // Whereas a device that cannot hold the layers just holds fewer tiles, and
    // says so rather than promising slots it does not have.
    let mut shallow = opened_limits();
    shallow.max_texture_dimension_2d = 1024;
    shallow.max_texture_array_layers = 2;
    let atlas = Atlas::plan(TILE_SIZE, MAX_NUM_TILES, &shallow).unwrap();
    assert!(atlas.slots() < MAX_NUM_TILES);
    assert_eq!(atlas.layers(), 2);
    assert_eq!(atlas.slots(), atlas.layers() * atlas.per_layer());
}

#[test]
fn a_fitted_atlas_stays_inside_the_bytes_it_was_given() {
    // A device whose textures stop at 2048 texels needs many layers for the
    // minimum specification, which is what makes dropping one of them the way a
    // cache shrinks. With a single layer there is nothing to drop, and the
    // floor below says so.
    let mut narrow = opened_limits();
    narrow.max_texture_dimension_2d = 2048;
    let atlas = Atlas::plan(TILE_SIZE, MAX_NUM_TILES, &narrow).unwrap();
    assert!(atlas.layers() > 1);
    let whole = atlas.bytes(PixelFormat::SRGBA8);

    // Half the memory is at most half the layers, and never more bytes.
    let half = atlas.fitted(PixelFormat::SRGBA8, whole / 2);
    assert!(half.bytes(PixelFormat::SRGBA8) <= whole / 2);
    assert!(half.slots() <= atlas.slots());

    // And a budget that pays for everything changes nothing.
    assert_eq!(atlas.fitted(PixelFormat::SRGBA8, whole), atlas);

    // One layer is the floor: a cache with no texture is not a smaller cache.
    assert_eq!(atlas.fitted(PixelFormat::SRGBA8, 1).layers(), 1);
}

#[test]
fn the_design_floor_holds_the_design_tile_count() {
    // The one number this crate's budget promises: the minimum specification's
    // share of the minimum specification's card holds the minimum
    // specification's tiles, in the archive's own three-channel scans, with the
    // mip chain and the widening to four bytes a texel both already paid for.
    let config = TileConfig::MIN_SPEC;
    let floor = VramBudget::new(FLOOR);
    assert_eq!(
        resident_tiles(floor, &config, PixelFormat::SRGB8),
        MAX_NUM_TILES,
    );

    // And it is the *device's* cost, not the file's: `VramBudget::capacity`
    // counts three bytes a texel and no mip chain, so it is the more generous
    // of the two and a cache sized from it would be over budget on the first
    // frame. Below the tile-count cap, where the difference is visible:
    let small = VramBudget::new(64 << 20);
    assert!(
        resident_tiles(small, &config, PixelFormat::SRGB8)
            < small.capacity(&config, PixelFormat::SRGB8),
    );

    let atlas = Atlas::plan(config.tile_size, MAX_NUM_TILES, &opened_limits())
        .unwrap()
        .fitted(PixelFormat::SRGB8, FLOOR);
    assert_eq!(atlas.slots(), MAX_NUM_TILES);
    assert!(atlas.bytes(PixelFormat::SRGB8) <= FLOOR);
}

#[test]
fn a_three_channel_scan_costs_four_bytes_a_texel_on_a_device() {
    // No device has a three-byte texel, so this is the one place a picture's
    // own idea of what it weighs and the card's disagree.
    assert_eq!(PixelFormat::SRGB8.bytes_per_texel(), 3);
    assert_eq!(device_bytes_per_texel(PixelFormat::SRGB8), 4);
    assert_eq!(
        texture_format(PixelFormat::SRGB8),
        Some(wgpu::TextureFormat::Rgba8UnormSrgb),
    );
    assert_eq!(
        texture_format(PixelFormat::RGBA8),
        Some(wgpu::TextureFormat::Rgba8Unorm),
    );
    // And the refusal: no graphics API has a one-channel sRGB texture, so a
    // mask declared sRGB has nowhere to live and is told so.
    assert_eq!(
        texture_format(PixelFormat::new(
            corvid_image::Channels::R,
            corvid_image::ColorSpace::Srgb,
        )),
        None,
    );
}

#[test]
fn the_shader_unpacks_a_table_entry_the_way_the_planner_packed_it() {
    // The agreement this crate exists to keep. `TileEntry` is `corvid_image`'s
    // and the constants below are the shader's, and nothing but this test is
    // between them.
    let entry = TileEntry::new(TileSlot(1337), 3, [5, 3]);
    let word = entry.bits().cast_unsigned();

    let slot_mask = wgsl_const("CORVID_TILE_SLOT_MASK");
    let level_shift = wgsl_const("CORVID_TILE_LEVEL_SHIFT");
    let level_mask = wgsl_const("CORVID_TILE_LEVEL_MASK");
    let u_shift = wgsl_const("CORVID_TILE_OFFSET_U_SHIFT");
    let v_shift = wgsl_const("CORVID_TILE_OFFSET_V_SHIFT");
    let offset_mask = wgsl_const("CORVID_TILE_OFFSET_MASK");

    assert_eq!(word & slot_mask, 1337);
    assert_eq!((word >> level_shift) & level_mask, 3);
    assert_eq!((word >> u_shift) & offset_mask, 5);
    assert_eq!((word >> v_shift) & offset_mask, 3);

    // And the sentinel, which is the value every entry of a fresh table holds.
    let absent = TileEntry::ABSENT.bits().cast_unsigned();
    assert_eq!(absent & slot_mask, wgsl_const("CORVID_TILE_ABSENT"));
    assert_eq!(TileEntry::ABSENT.slot(), None);
}

#[test]
fn the_shader_has_room_for_every_source_a_configuration_may_hold() {
    const { assert!(MAX_NUM_MAPS <= MAX_SOURCES) };
    assert_eq!(wgsl_const("CORVID_TILE_SOURCES"), MAX_SOURCES);
    const { assert!(TileConfig::MIN_SPEC.max_sources <= MAX_SOURCES) };
}

#[test]
fn moving_the_bind_group_moves_all_four_bindings_and_nothing_else() {
    let moved = wgsl_at(7);
    assert_eq!(moved.matches("@group(7)").count(), 4);
    assert_eq!(moved.matches(&format!("@group({GROUP})")).count(), 0);
    // The rest of the text is untouched, which is what makes a substitution
    // safe to do at all: only the four binding lines carry a group.
    assert_eq!(moved.len(), WGSL.len());
    assert_eq!(wgsl_at(GROUP), WGSL);
}
