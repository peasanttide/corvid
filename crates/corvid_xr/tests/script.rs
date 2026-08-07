//! The stand-in plays back exactly, on every machine and every run.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "these tests build fixtures out of raw bit patterns and read matrices back as floats; every cast here is the thing under test rather than an oversight"
)]

use std::{fs, path::Path};

use corvid_fixed::Factor16;
use corvid_hash::digest;
use corvid_xr::{Confidence, Headset, PoseTrack, ScriptedHeadset, Space, State, Tracked, Views};

/// The three recordings, and what each of them comes to.
///
/// Frozen, so a track that is accidentally re-recorded — by running
/// `cargo run --example record` and committing the result — is a red test
/// rather than a fixture that quietly moved under every golden built on it.
const FROZEN: [(&str, u64); 3] = [
    ("table", 0x360E_5B10_A872_CE02),
    ("surface", 0xCA34_7056_5646_1F39),
    ("lossy", 0x5A99_3E66_FEE2_F4A2),
];

/// A track from `tracks/`.
fn recorded(name: &str) -> PoseTrack {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tracks")
        .join(format!("{name}.track"));
    let bytes = fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    PoseTrack::decode(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Every view a whole run of `track` produces, in order.
fn played(track: PoseTrack) -> Vec<Tracked<Views>> {
    let mut headset = ScriptedHeadset::new(track);
    let mut seen = Vec::new();
    while !headset.poll().is_over() {
        seen.push(headset.views());
    }
    seen
}

#[test]
fn ninety_still_frames_are_ninety_identical_head_poses_and_then_the_end() {
    let mut headset = ScriptedHeadset::new(PoseTrack::still(90));
    let mut heads = Vec::new();
    while !headset.poll().is_over() {
        heads.push(headset.head(Space::Stage).value);
    }
    assert_eq!(heads.len(), 90);
    assert!(
        heads.windows(2).all(|pair| pair[0] == pair[1]),
        "a still track moved"
    );
    // And it stays ended.
    assert_eq!(headset.poll(), State::Exiting);
    assert!(headset.is_over());
}

#[test]
fn a_looping_track_never_ends() {
    let mut headset = ScriptedHeadset::new(PoseTrack::table(30)).looping(true);
    for _ in 0..1_000 {
        assert!(!headset.poll().is_over());
    }
    assert!(!headset.is_over());
    assert!(headset.frame() < 30);
}

#[test]
fn an_empty_track_is_over_before_it_starts() {
    let mut headset = ScriptedHeadset::new(PoseTrack::empty());
    assert_eq!(headset.poll(), State::Exiting);
    assert_eq!(headset.frame(), 0);
    assert_eq!(headset.rate(), corvid_xr::RATE);
}

#[test]
fn a_lossy_track_reports_lost_and_keeps_the_last_known_value() {
    let track = PoseTrack::lossy(90);
    let (from, to) = (30, 60);

    let believed: Vec<_> = track
        .frames
        .iter()
        .map(|frame| frame.head.confidence)
        .collect();
    for (index, confidence) in believed.iter().enumerate() {
        let expected = if (from..to).contains(&index) {
            Confidence::Lost
        } else {
            Confidence::Tracked
        };
        assert_eq!(*confidence, expected, "frame {index}");
    }

    // Through the whole loss the value is the last one that was measured,
    // rather than the origin. A frozen hand reads as a glitch; a hand at the
    // origin reads as a bug.
    let last_known = track.frames[from - 1].head.value;
    for frame in &track.frames[from..to] {
        assert_eq!(frame.head.value, last_known);
        assert_eq!(frame.head.believed(), None);
        for hand in frame.hands {
            assert_eq!(hand.confidence, Confidence::Lost);
            assert_eq!(hand.believed(), None);
        }
    }
    // And tracking comes back.
    assert_eq!(track.frames[to].head.confidence, Confidence::Tracked);
}

#[test]
fn a_track_round_trips_through_the_wire_byte_identically() {
    for track in [
        PoseTrack::still(30),
        PoseTrack::lossy(30),
        PoseTrack::table(30),
        PoseTrack::surface(30),
    ] {
        let once = track.encode().expect("a track encodes");
        let back = PoseTrack::decode(&once).expect("and decodes");
        assert_eq!(back, track);
        assert_eq!(back.encode().expect("and encodes again"), once);
    }
}

#[test]
fn two_runs_of_one_track_produce_the_same_views() {
    let first = played(PoseTrack::surface(300));
    let second = played(PoseTrack::surface(300));
    assert_eq!(first.len(), 300);
    assert_eq!(digest(&first), digest(&second));
    // It advances on `poll` and never on a clock, so this is a byte comparison
    // rather than a tolerance.
    assert_eq!(first, second);
}

#[test]
fn the_three_recorded_tracks_load_and_play_and_their_digests_are_frozen() {
    let mut found = Vec::new();
    for (name, _) in FROZEN {
        let track = recorded(name);
        assert_eq!(track.frames.len(), 900, "{name}");
        assert_eq!(track.rate, corvid_xr::RATE, "{name}");
        found.push((name, digest(&track).to_u64()));
        assert_eq!(played(track).len(), 900, "{name}");
    }
    let written: Vec<_> = found
        .iter()
        .map(|(name, seen)| format!("(\"{name}\", {seen:#018X})"))
        .collect();
    assert_eq!(
        found,
        FROZEN.to_vec(),
        "a track moved; if that was deliberate, freeze: {}",
        written.join(", ")
    );
}

#[test]
fn a_recorded_track_is_the_session_the_recorder_built() {
    assert_eq!(recorded("table"), PoseTrack::table(900));
    assert_eq!(recorded("surface"), PoseTrack::surface(900));
    assert_eq!(recorded("lossy"), PoseTrack::lossy(900));
}

#[test]
fn the_table_track_grips_and_the_surface_track_pinches() {
    let table = recorded("table");
    assert!(
        table
            .frames
            .iter()
            .all(|frame| frame.hands[1].value.is_gripping()),
        "a swarm session holds the planet"
    );
    let surface = recorded("surface");
    assert!(
        surface
            .frames
            .iter()
            .any(|frame| frame.hands[1].value.is_pinching()),
        "a defender session places something"
    );
    assert!(
        surface
            .frames
            .iter()
            .any(|frame| frame.hands[1].value.pinch == Factor16::MIN),
        "and lets go again"
    );
}

#[test]
fn a_tracks_duration_is_its_frames_at_its_own_rate() {
    let track = PoseTrack::still(90);
    assert_eq!(track.duration().as_millis(), 1_000);
    assert_eq!(PoseTrack::empty().duration().as_nanos(), 0);
}
