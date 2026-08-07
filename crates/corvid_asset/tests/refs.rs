#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]

//! A `G::Ref` resolves to a `Handle<G::Level>`, and nothing goes the other way.

use std::str::FromStr;

use corvid_asset::{Asset, Assets, Locate, Lod, Malformed, Memory, Unavailable};
use corvid_behavior::{Level, Source};
use serde::{Deserialize, Serialize};

/// The level: authored, diffable text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Rooms(Vec<String>);

impl Asset for Rooms {
    fn placeholder() -> Self {
        Self::default()
    }

    fn decode(bytes: &[u8], _lod: Lod) -> Result<Self, Malformed> {
        let text = str::from_utf8(bytes).map_err(|_| Malformed::new("not utf-8"))?;
        Ok(Self(text.lines().map(str::to_owned).collect()))
    }
}

/// What a tick emits, and what the log carries. Small, hashable, and nothing
/// like a pointer into one machine's cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum Which {
    Terminus,
    Cellar,
    Nowhere,
}

/// A string that is not one of the levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct NotALevel;

impl FromStr for Which {
    type Err = NotALevel;

    fn from_str(text: &str) -> Result<Self, NotALevel> {
        match text {
            "terminus" => Ok(Self::Terminus),
            "cellar" => Ok(Self::Cellar),
            "nowhere" => Ok(Self::Nowhere),
            _ => Err(NotALevel),
        }
    }
}

/// `Locate` sits on the level now, which is where its reference is declared.
/// There used to be a marker between them for the orphan rule; there is no
/// marker any more.
impl Level for Rooms {
    type Reference = Which;

    fn load(reference: &Which, files: &dyn Source) -> Result<Self, Malformed> {
        let bytes = files.read(&<Self as Locate>::locate(reference))?;
        let text = str::from_utf8(&bytes).map_err(|_| Malformed::new("not utf-8"))?;
        Ok(Self(text.lines().map(str::to_owned).collect()))
    }
}

impl Locate for Rooms {
    fn locate(reference: &Which) -> String {
        match reference {
            Which::Terminus => "levels/terminus".to_owned(),
            Which::Cellar => "levels/cellar".to_owned(),
            Which::Nowhere => "levels/nowhere".to_owned(),
        }
    }
}

fn registry() -> Assets {
    let mut files = Memory::new();
    files.insert("levels/terminus", b"hall\ncellar".to_vec());
    files.insert("levels/cellar", b"cellar".to_vec());
    Assets::new(Box::new(files))
}

#[test]
fn a_reference_resolves_to_a_handle() {
    let assets = registry();

    let level = corvid_asset::resolve::<Rooms>(&assets, &Which::Terminus);
    assert!(!level.is_resident());

    for _ in 0..100_000 {
        assets.poll();
        if assets.is_settled() {
            break;
        }
        std::thread::yield_now();
    }

    assert!(level.is_resident());
    assert_eq!(level.get().0, ["hall", "cellar"]);
}

#[test]
fn the_same_reference_twice_names_one_asset() {
    let assets = registry();

    let first =
        corvid_asset::resolve_now::<Rooms>(&assets, &Which::Cellar).expect("the level is there");
    let second =
        corvid_asset::resolve_now::<Rooms>(&assets, &Which::Cellar).expect("the level is there");

    assert_eq!(first.get(), second.get());
    assert_eq!(assets.len(), 1);
}

#[test]
fn a_reference_naming_a_missing_level_fails_rather_than_stalling() {
    let assets = registry();

    let refused = corvid_asset::resolve_now::<Rooms>(&assets, &Which::Nowhere).unwrap_err();
    assert!(matches!(refused, Unavailable::Missing(_)));

    // And the barrier the runtime is waiting on lifts: the request is answered.
    assert!(assets.is_settled());
}

#[test]
fn a_reference_is_what_the_tick_carries() {
    // A `Ref` is `Data`, so a log can hold one and two peers can compare them.
    // The `Handle` a runtime resolves it to is not, and the `compile_fail`
    // doctest on `Handle` is where that is checked.
    fn only_data<T: corvid_behavior::Data>() {}

    only_data::<Which>();
    only_data::<Rooms>();
}
