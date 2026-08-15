//! The lifecycle, the eyes, and the rest of the vocabulary, driven by the
//! stand-in.

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

use corvid_fixed::{Angle16, I16F16, I48F16};
use corvid_glm::{Mat4, Vec3i};
use corvid_rotation::FineRotation;
use corvid_vector::{FinePoint, GlobalFinePoint, globalfinepoint};
use corvid_xr::{
    Anchor, EyeView, Hand, Haptic, Headset, Passthrough, Pose, PoseTrack, ScriptedHeadset, Side,
    Space, State, Views,
};

/// Every transition the machine allows, written out by hand so that the table
/// and the implementation are two statements of the same rule rather than one.
///
/// Staying put is legal everywhere and is not listed; leaving for `Exiting` is
/// legal everywhere and is not listed either.
const LEGAL: [(State, State); 7] = [
    (State::Idle, State::Ready),
    (State::Ready, State::Visible),
    (State::Ready, State::Stopping),
    (State::Visible, State::Focused),
    (State::Visible, State::Stopping),
    (State::Focused, State::Visible),
    (State::Focused, State::Stopping),
];

/// Whether the table above, plus the two blanket rules, allows this step.
fn allowed(from: State, to: State) -> bool {
    if from == to {
        return true;
    }
    if from == State::Exiting {
        return false;
    }
    to == State::Exiting
        || from == State::Stopping && to == State::Idle
        || LEGAL.contains(&(from, to))
}

/// Near and far for the eye tests: a decimetre to a hundred metres.
const NEAR: I16F16 = I16F16::from_f64(0.1);
/// The far plane those tests use.
const FAR: I16F16 = I16F16::from_f64(100.0);

/// `m * v`, the way a vertex stage would apply it.
///
/// A plain product now that a `Mat4` is stored in the order a shader reads it,
/// rather than the hand-written loop the row-major convention needed.
fn apply(m: Mat4, v: [f32; 4]) -> [f32; 4] {
    let out = m * corvid_glm::nalgebra::Vector4::new(v[0], v[1], v[2], v[3]);
    [out.x, out.y, out.z, out.w]
}

#[test]
fn every_legal_transition_is_allowed_and_every_other_is_refused() {
    let mut legal = 0;
    let mut illegal = 0;
    for from in State::ALL {
        for to in State::ALL {
            assert_eq!(
                from.may_become(to),
                allowed(from, to),
                "{from:?} -> {to:?} disagreed with the table"
            );
            if allowed(from, to) {
                legal += 1;
            } else {
                illegal += 1;
            }
        }
    }
    // Both halves of the machine were actually exercised, rather than a table
    // that happens to say yes to everything.
    assert!(legal > 0 && illegal > 0, "{legal} legal, {illegal} illegal");
}

#[test]
fn exiting_leads_nowhere() {
    for to in State::ALL {
        assert_eq!(State::Exiting.may_become(to), to == State::Exiting);
    }
    assert!(State::Exiting.is_over());
    assert!(!State::Exiting.is_drawing());
}

#[test]
fn a_whole_scripted_session_never_takes_an_illegal_step() {
    let mut headset = ScriptedHeadset::new(PoseTrack::table(900));
    let mut state = State::Idle;
    let mut steps = 0;
    let mut seen = Vec::new();
    loop {
        let next = headset.poll();
        assert!(
            state.may_become(next),
            "the stand-in stepped {state:?} -> {next:?} at frame {}",
            headset.frame()
        );
        if !seen.contains(&next) {
            seen.push(next);
        }
        state = next;
        steps += 1;
        if state.is_over() {
            break;
        }
    }
    assert_eq!(steps, 901, "nine hundred frames, then the end");
    // A track records a whole session, so playing one walks the machine.
    for state in State::ALL {
        assert!(seen.contains(&state), "the session never reached {state:?}");
    }
}

#[test]
fn an_asymmetric_frustum_puts_the_frustum_centre_off_the_image_centre() {
    let asymmetric = EyeView::default();
    let clip = asymmetric.clip(NEAR, FAR);
    let [tl, tr, _, _] = asymmetric.tangents();

    // The eye's forward axis is +Y. A symmetric projection would put it in the
    // middle of the image; this one does not, and that offset is the whole
    // reason the type carries four angles.
    let forward = apply(clip, [0.0, 1.0, 0.0, 1.0]);
    let offset = forward[0] / forward[3];
    assert!(
        offset.abs() > 0.05,
        "the frustum was centred after all: {offset}"
    );

    // The middle of the frustum is where the offset goes away.
    let centre = apply(clip, [f32::midpoint(tl, tr), 1.0, 0.0, 1.0]);
    assert!((centre[0] / centre[3]).abs() < 1e-5);

    // And a symmetric frustum has no offset at all, which is what says the
    // asymmetry above came from the angles rather than from the maths.
    let symmetric = EyeView {
        left: Angle16::from_degrees(-45.0),
        ..asymmetric
    };
    let straight = apply(symmetric.clip(NEAR, FAR), [0.0, 1.0, 0.0, 1.0]);
    assert!((straight[0] / straight[3]).abs() < 1e-5);
}

#[test]
fn the_near_and_far_planes_land_at_zero_and_one() {
    let clip = EyeView::default().clip(NEAR, FAR);
    let near = apply(clip, [0.0, NEAR.to_f32(), 0.0, 1.0]);
    let far = apply(clip, [0.0, FAR.to_f32(), 0.0, 1.0]);
    assert!(
        (near[2] / near[3]).abs() < 1e-4,
        "near was {}",
        near[2] / near[3]
    );
    assert!(
        (far[2] / far[3] - 1.0).abs() < 1e-4,
        "far was {}",
        far[2] / far[3]
    );
}

#[test]
fn a_metre_from_the_eye_survives_ten_thousand_kilometres_to_within_a_millimetre() {
    let anchor = Anchor::standing(globalfinepoint(10_000_000, 0, 0), FineRotation::IDENTITY);
    let stage = Pose::new(
        FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::from_f64(1.7)),
        FineRotation::IDENTITY,
    );
    let eye = EyeView::default().at(stage).eye(anchor, NEAR, FAR);
    let world = anchor.to_world(stage);

    // The coarse part is whole metres, floored, and exact.
    assert_eq!(eye.coarse, Vec3i::new(10_000_000, 0, 1));

    // What a game's vertex stage does: subtract the coarse origin in integers,
    // and only then let the difference become an `f32`.
    let relative = |point: GlobalFinePoint| {
        let axes = point.to_array();
        [0usize, 1, 2].map(|axis| {
            // The subtraction is exact and integer, and only what is left of it
            // is allowed to become an `f32`. `to_f32` is `corvid_fixed`'s, so
            // the scale is the type's rather than a literal here.
            axes[axis]
                .saturating_sub(I48F16::from(eye.coarse[axis]))
                .to_f32()
        })
    };
    let here = relative(world.position());
    let a_metre_off = relative(world.position().add(GlobalFinePoint::new(
        I48F16::ZERO,
        I48F16::ONE,
        I48F16::ZERO,
    )));
    let gap = (0..3)
        .map(|axis| (a_metre_off[axis] - here[axis]).powi(2))
        .sum::<f32>()
        .sqrt();
    assert!((gap - 1.0).abs() < 0.001, "a metre came back as {gap} m");
}

#[test]
fn both_eyes_sit_half_the_separation_either_side_of_the_head() {
    let head = Pose::IDENTITY;
    let views = Views::from_head(head, I16F16::from_f64(0.064), EyeView::default());
    let apart = views.right.pose.position().x().to_f64() - views.left.pose.position().x().to_f64();
    assert!(
        (apart - 0.064).abs() < 0.001,
        "the eyes were {apart} m apart"
    );
    assert_eq!(views.eye(Side::Left), views.left);
    assert_eq!(views.eye(Side::Right), views.right);
    assert_eq!(Views::from(views.to_array()), views);
}

#[test]
fn asking_a_headset_without_passthrough_for_it_answers_unavailable() {
    let mut track = PoseTrack::still(4);
    for frame in &mut track.frames {
        frame.passthrough = Passthrough::Unavailable;
    }
    let mut headset = ScriptedHeadset::new(track);
    headset.poll();
    assert_eq!(headset.set_passthrough(true), Passthrough::Unavailable);
    assert_eq!(headset.passthrough(), Passthrough::Unavailable);
}

#[test]
fn asking_a_headset_that_has_it_turns_it_on_and_off() {
    let mut headset = ScriptedHeadset::new(PoseTrack::still(4));
    headset.poll();
    assert_eq!(headset.passthrough(), Passthrough::Off);
    assert_eq!(headset.set_passthrough(true), Passthrough::On);
    assert_eq!(headset.passthrough(), Passthrough::On);
    assert_eq!(headset.set_passthrough(false), Passthrough::Off);
    assert_eq!(bool::try_from(headset.passthrough()), Ok(false));
}

#[test]
fn a_rumble_is_recorded_so_a_haptic_is_testable() {
    let mut headset = ScriptedHeadset::new(PoseTrack::still(4));
    headset.poll();
    headset.rumble(Side::Right.index(), Haptic::CLICK);
    headset.rumble(9, Haptic::THUD);
    assert_eq!(headset.rumbles(), &[(1, Haptic::CLICK)]);
    headset.clear_rumbles();
    assert!(headset.rumbles().is_empty());
}

#[test]
fn a_hand_is_four_values_and_the_two_predicates_read_them() {
    let mut headset = ScriptedHeadset::new(PoseTrack::table(4));
    headset.poll();
    let right = headset.hand(Side::Right);
    assert!(
        right.value.is_gripping(),
        "the table track grips throughout"
    );
    assert!(!right.value.is_pinching());
    assert_eq!(headset.hands()[Side::Right.index()], right);
    assert!(!headset.hand(Side::Left).value.is_gripping());
    assert_eq!(Side::Left.other(), Side::Right);
    assert_eq!(usize::from(Side::Right), 1);
    assert!(Side::try_from(2).is_err());
    assert_eq!(Hand::open(Pose::IDENTITY).aim, Pose::IDENTITY);
}

#[test]
fn the_head_is_the_stage_pose_and_the_view_space_pose_is_the_identity() {
    let mut headset = ScriptedHeadset::new(PoseTrack::surface(8));
    headset.poll();
    assert_eq!(headset.head(Space::View).value, Pose::IDENTITY);
    assert_eq!(
        headset.head(Space::Local).value.position(),
        FinePoint::ZERO,
        "the first frame is where local space begins"
    );
    assert_ne!(headset.head(Space::Stage).value.position(), FinePoint::ZERO);
    assert_eq!(headset.rate(), corvid_xr::RATE);
}
