//! Registration refuses what it cannot plan, rather than trimming it to fit.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

use corvid_image::{
    ConfigError, Extent, PixelFormat, SourceId, TileConfig, TileError, TileKey, TilePlanner,
    VramBudget, extent,
};

fn planner() -> TilePlanner {
    TilePlanner::new(TileConfig::MIN_SPEC).expect("the minimum specification validates")
}

/// The refusal the whole design turns on. A plate wider than the page index can
/// address is not clipped to the ceiling and not silently halved -- both of
/// those produce a map that is wrong in a way nothing downstream can see.
#[test]
fn an_image_past_the_ceiling_is_refused() {
    let mut planner = planner();
    let too_big = extent(TileConfig::MIN_SPEC.max_image_size + 1, 4096);

    assert_eq!(
        planner.register(too_big, PixelFormat::SRGB8),
        Err(TileError::TooLarge {
            extent: too_big,
            max: TileConfig::MIN_SPEC.max_image_size,
        })
    );
    // And nothing was registered under a truncated size instead.
    assert!(planner.sources().is_empty());
    assert_eq!(planner.sources().get(SourceId(0)), None);
}

/// The ceiling itself is fine; it is one texel past it that is not.
#[test]
fn an_image_at_the_ceiling_is_accepted() {
    let mut planner = planner();
    let side = TileConfig::MIN_SPEC.max_image_size;
    let id = planner
        .register(Extent::new(side, side), PixelFormat::SRGB8)
        .expect("the configured maximum registers");
    let source = planner.sources().get(id).expect("a registered source");

    // 131072 texels over 256-texel tiles is 512 pages, and the eight bits of
    // offset in an entry stop the pyramid at level eight, where the plate is
    // still a two-by-two grid rather than a single tile. That is the tradeoff
    // `TileConfig::MAX_LEVEL` documents, asserted rather than assumed.
    assert_eq!(source.pages(&TileConfig::MIN_SPEC), [512, 512]);
    assert_eq!(source.top_level(), TileConfig::MAX_LEVEL);
    assert_eq!(
        source.tiles_at(&TileConfig::MIN_SPEC, u32::from(TileConfig::MAX_LEVEL)),
        [2, 2]
    );
}

#[test]
fn an_image_with_no_texels_is_refused() {
    let mut planner = planner();
    let flat = extent(4096, 0);
    assert_eq!(
        planner.register(flat, PixelFormat::SRGB8),
        Err(TileError::Empty { extent: flat })
    );
}

#[test]
fn the_source_count_is_a_limit_rather_than_a_wrap() {
    let config = TileConfig {
        max_sources: 3,
        ..TileConfig::MIN_SPEC
    };
    let mut planner = TilePlanner::new(config).expect("three sources is a valid configuration");
    for _ in 0..3 {
        planner
            .register(extent(512, 512), PixelFormat::R8)
            .expect("a source under the limit");
    }
    assert_eq!(
        planner.register(extent(512, 512), PixelFormat::R8),
        Err(TileError::TooManySources(3))
    );
    assert_eq!(planner.sources().len(), 3);
}

/// Every configuration the packed entry has no bits for is refused where it is
/// built, which is what lets every later step be arithmetic with no error path.
#[test]
fn a_configuration_the_entry_cannot_hold_is_refused() {
    let odd_tile = TileConfig {
        tile_size: 300,
        ..TileConfig::MIN_SPEC
    };
    assert_eq!(odd_tile.validate(), Err(ConfigError::TileSize(300)));
    assert!(TilePlanner::new(odd_tile).is_err());

    let odd_image = TileConfig {
        max_image_size: 100_000,
        ..TileConfig::MIN_SPEC
    };
    assert_eq!(
        odd_image.validate(),
        Err(ConfigError::MaxImageSize {
            size: 100_000,
            tile: 256,
        })
    );

    // 4096 is one past what twelve bits can name once the sentinel is spent.
    let too_many = TileConfig {
        max_tiles: 4096,
        ..TileConfig::MIN_SPEC
    };
    assert_eq!(
        too_many.validate(),
        Err(ConfigError::TileCount {
            tiles: 4096,
            bits: 12,
        })
    );
    assert!(
        TileConfig {
            max_tiles: 4095,
            ..TileConfig::MIN_SPEC
        }
        .validate()
        .is_ok()
    );

    let too_many_maps = TileConfig {
        max_sources: 256,
        ..TileConfig::MIN_SPEC
    };
    assert_eq!(too_many_maps.validate(), Err(ConfigError::SourceCount(256)));
}

/// The budget converts to tiles, and the configured tile count is a ceiling on
/// the answer rather than a suggestion.
#[test]
fn the_budget_is_capped_by_the_configuration() {
    let config = TileConfig::MIN_SPEC;
    // One four-channel tile is 256 KiB.
    assert_eq!(config.tile_bytes(PixelFormat::SRGBA8), 262_144);
    assert_eq!(
        VramBudget::new(262_144 * 9).capacity(&config, PixelFormat::SRGBA8),
        9
    );
    assert_eq!(VramBudget::new(0).capacity(&config, PixelFormat::SRGBA8), 0);
    // A card with room for sixteen thousand still gets the configured maximum.
    assert_eq!(
        VramBudget::MIN_SPEC.capacity(&config, PixelFormat::SRGBA8),
        config.max_tiles
    );
}

/// The configuration is a configuration: nothing above depends on the minimum
/// specification's own numbers being the ones in the arithmetic.
#[test]
fn the_numbers_are_configurable_rather_than_baked() {
    let small = TileConfig {
        tile_size: 64,
        max_tiles: 32,
        max_sources: 2,
        max_image_size: 1 << 12,
    };
    small.validate().expect("a smaller machine's configuration");

    let mut planner = TilePlanner::new(small).expect("a planner for it");
    let id = planner
        .register(extent(1024, 512), PixelFormat::R8)
        .expect("a plate inside the smaller ceiling");
    let source = planner.sources().get(id).expect("a registered source");

    assert_eq!(source.pages(&small), [16, 8]);
    // 1024 over 64-texel tiles is 16 tiles, so level four is one tile across
    // and the short side got there two levels earlier.
    assert_eq!(source.top_level(), 4);
    assert_eq!(source.tiles_at(&small, 4), [1, 1]);
    assert_eq!(small.tile_bytes(PixelFormat::R8), 4096);

    // The grid is rectangular, so a key can name a tile the plate does not
    // have: at level three the plate is two tiles across and one down.
    assert_eq!(source.tiles_at(&small, 3), [2, 1]);
    assert!(source.contains(&small, TileKey::new(id, 3, 1, 0)));
    assert!(!source.contains(&small, TileKey::new(id, 3, 1, 1)));
    assert!(!source.contains(&small, TileKey::new(id, 5, 0, 0)));
    assert!(source.contains(&small, TileKey::new(id, 0, 15, 7)));
    assert!(!source.contains(&small, TileKey::new(id, 0, 16, 0)));
}
