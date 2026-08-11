//! What happens to a snapshot between one tick and the next.
//!
//! Split from [what it answers](../snapshot.rs) because a file stays under 400
//! lines, and this is the seam that was already there: every test here spans
//! two readings, and none of them is about which set is active.
//!
//! The resting state is here too -- what an untouched snapshot and an empty
//! table read as -- because that is the state [`clear`](Input::clear) returns
//! one to, and a test that pins it belongs next to the one that gets there.

use corvid_fixed::{I48F16, Signed16};
use corvid_input::{Analog, Cursor, Digital, DigitalId, Input, PoseId, Viewport};
use corvid_transform::FineTransform;
use corvid_vector::GlobalFinePoint;

/// Two sets that share no action, which is the arrangement a console overlay
/// makes: `Console` is layered over `Build` and neither knows about the other.
pub mod action {
    corvid_input::action_sets! {
        pub set Build {
            digital PLACE, CANCEL;
            analog LOOK, MOVE;
            pose POINTER;
        }
        pub set Console {
            digital SUBMIT, DISMISS;
            analog SCROLL;
            pose CARET;
        }
    }
}

/// A declaration whose active set owns three ranges that share no number.
///
/// The two sets above are the arrangement a game makes, and they are the wrong
/// arrangement to test a query with: every set there declares a similar handful
/// of each kind, so `Build`'s digital, analog and pose ranges all start at zero
/// and all cover the low numbers, and a query that consulted the wrong kind's
/// range would get the right answer by accident. Here `Cockpit` is declared
/// first and declares a different number of each kind, which pushes `Turret`'s
/// three ranges apart: its digital actions are 1..=3, its analog actions 4..=5
/// and its one pose is 6. No number is in two of them, and the three runs are
/// three different lengths, so a query that consults another kind's range is
/// out of range and answers with the silenced value.
pub mod disjoint {
    corvid_input::action_sets! {
        pub set Cockpit {
            digital EJECT;
            analog THROTTLE, YAW, PITCH, ROLL;
            pose HELMET, LEFT_HAND, RIGHT_HAND, SEAT, STICK, LEVER;
        }
        pub set Turret {
            digital FIRE, RELOAD, ZOOM_IN;
            analog TRAVERSE, ELEVATE;
            pose GUNNER;
        }
    }
}

/// The same shape with the three totals in the opposite order.
///
/// `disjoint` has fewer digital actions than analog and fewer analog than pose,
/// so a snapshot that sized one kind's storage from another kind's total would
/// only ever be caught when it made the storage *too small* -- and three of the
/// six ways to get that wrong make it too big, which nothing can observe. Here
/// the totals run the other way, ten digital against four analog against two
/// pose, so every swap that over-allocates in `disjoint` under-allocates here
/// and drops the identifier at the top of the range.
pub mod inverted {
    corvid_input::action_sets! {
        pub set Chat {
            digital SEND, ABANDON, HISTORY_UP, HISTORY_DOWN, CURSOR_LEFT,
                CURSOR_RIGHT, BACKSPACE;
            analog CARET_DRAG, SCROLL;
            pose STYLUS;
        }
        pub set Wand {
            digital TRIGGER, GRIP, MENU;
            analog TOUCHPAD, TWIST;
            pose TIP;
        }
    }
}

/// A stick pushed hard right and a little up.
///
/// Neither axis is zero and neither is the other, so a snapshot that answered
/// with the wrong axis, or with a zero it should not have, is visible.
const fn pushed() -> Analog {
    Analog::new(Signed16::from_bits(30_000), Signed16::from_bits(4_000))
}

/// A frame's worth of mouse motion, left and down.
///
/// Deliberately unlike [`pushed`] on both axes, so that a snapshot holding one
/// slot for the two analog accessors reads back as whichever was written last
/// rather than as either.
const fn swept() -> Analog {
    Analog::new(Signed16::from_bits(-9_000), Signed16::from_bits(-1_500))
}

/// A pose a metre east of the origin.
///
/// Somewhere other than the identity on purpose, so that a query answering with
/// `Some(FineTransform::IDENTITY)` could not pass for the pose that was
/// actually recorded.
const fn somewhere() -> FineTransform {
    FineTransform::IDENTITY.with_position(GlobalFinePoint::new(
        I48F16::ONE,
        I48F16::ZERO,
        I48F16::ZERO,
    ))
}

#[test]
fn clear_returns_every_value_and_keeps_the_active_set() {
    let mut input = Input::new(action::SETS);
    input.activate(action::Console::ID);
    input.set_digital(action::SUBMIT, Digital::HELD);
    input.set_analog(action::SCROLL, pushed());
    input.set_delta(action::SCROLL, swept());
    input.set_pose(action::CARET, Some(somewhere()));
    input.set_pointer(Some(pushed()));

    input.clear();

    assert_eq!(input.active_set(), action::Console::ID);
    assert_eq!(input.digital(action::SUBMIT), Digital::RELEASED);
    assert_eq!(input.analog(action::SCROLL), Analog::ZERO);
    assert_eq!(input.delta(action::SCROLL), Analog::ZERO);
    assert_eq!(input.pose(action::CARET), None);
    assert_eq!(input.pointer(), None);

    // And it really cleared rather than merely stopped answering: the values
    // behind the inactive set are gone too, which is what tells `clear` apart
    // from `activate`.
    input.activate(action::Build::ID);
    assert_eq!(input.digital(action::PLACE), Digital::RELEASED);
}

#[test]
fn a_tap_inside_one_frame_carries_both_edges() {
    // Not a property of the mask, but of `Digital`: the combination that looks
    // wrong is the one a game must not miss, so nothing here rejects it.
    let mut input = Input::new(action::SETS);
    let tap = Digital {
        held: false,
        pressed: true,
        released: true,
    };
    input.set_digital(action::PLACE, tap);

    assert_eq!(input.digital(action::PLACE), tap);
}

#[test]
fn absorbing_keeps_an_edge_no_tick_has_spent_and_replaces_the_levels() {
    // The reason `absorb` exists. A window ends the edge interval once per
    // displayed frame and a loop consumes it once per tick, and at fifteen
    // hertz on a sixty-hertz display three readings in four owe no tick at all
    // -- so a snapshot that was replaced rather than folded would drop the tap
    // that started and finished between two of them.
    let mut unspent = Input::new(action::SETS);

    let mut tapped = Input::new(action::SETS);
    tapped.set_digital(
        action::PLACE,
        Digital {
            held: false,
            pressed: true,
            released: true,
        },
    );
    tapped.set_delta(
        action::LOOK,
        Analog::new(Signed16::from_bits(30), Signed16::ZERO),
    );
    unspent.absorb(&tapped);

    // The next reading has no edge in it and the button is not down. The edge
    // has still not been spent, so it is still there; the level is this
    // reading's.
    let quiet = Input::new(action::SETS);
    unspent.absorb(&quiet);

    assert_eq!(
        unspent.digital(action::PLACE),
        Digital {
            held: false,
            pressed: true,
            released: true,
        },
        "an edge no tick has consumed survives a reading that has none",
    );

    // Displacements add up over the interval rather than being replaced by the
    // last reading of it.
    let mut moved = Input::new(action::SETS);
    moved.set_delta(
        action::LOOK,
        Analog::new(Signed16::from_bits(12), Signed16::ZERO),
    );
    unspent.absorb(&moved);
    assert_eq!(unspent.delta(action::LOOK).x, Signed16::from_bits(42));
}

#[test]
fn settling_spends_the_edges_and_the_displacements_and_leaves_the_levels() {
    // The other half, and what stops a frame that owes eight catch-up ticks
    // from turning one keypress into eight actions.
    let mut input = Input::new(action::SETS);
    input.set_digital(
        action::PLACE,
        Digital {
            held: true,
            pressed: true,
            released: false,
        },
    );
    input.set_analog(action::MOVE, Analog::new(Signed16::MAX, Signed16::ZERO));
    input.set_delta(
        action::LOOK,
        Analog::new(Signed16::from_bits(9), Signed16::ZERO),
    );

    input.settle();

    assert_eq!(
        input.digital(action::PLACE),
        Digital::HELD,
        "the button is still down; only the edge was spent",
    );
    assert_eq!(input.delta(action::LOOK), Analog::ZERO);
    assert_eq!(
        input.analog(action::MOVE),
        Analog::new(Signed16::MAX, Signed16::ZERO),
        "a deflection is a level and is not an interval to end",
    );
}

#[test]
fn the_viewport_is_a_level_and_absent_where_there_is_no_display() {
    // The other half of the pointer: an `Analog` position is normalised against
    // a rectangle, and the only thing that turns one back into pixels is the
    // size of that rectangle. A game handed the first without the second can
    // tell which button the pointer is nearest and cannot tell how wide it is.
    let mut input = Input::new(action::SETS);
    assert_eq!(
        input.viewport(),
        None,
        "a fresh snapshot claims a display it has not been told about",
    );

    let window = Viewport::new(1280, 1024);
    input.set_viewport(Some(window));
    assert_eq!(input.viewport(), window.into());
    assert!(!window.is_empty());
    assert!(Viewport::new(0, 1080).is_empty(), "a minimised window");

    // It is not an action, so activating another set does not silence it -- a
    // console overlay is laid out in the same window the game was.
    input.activate(action::Console::ID);
    assert_eq!(input.viewport(), Some(window));

    // A level rather than an edge: the freshest reading is the whole answer,
    // and a tick that spent this frame's edges leaves it alone. A window that
    // was resized is in its new size, not in the union of both.
    let mut fresh = Input::new(action::SETS);
    let resized = Viewport::new(2560, 1080);
    fresh.set_viewport(Some(resized));
    input.absorb(&fresh);
    assert_eq!(input.viewport(), Some(resized));
    input.settle();
    assert_eq!(input.viewport(), Some(resized));

    // And `clear` puts it back to the honest answer for a run with no display.
    input.clear();
    assert_eq!(input.viewport(), None);
}

#[test]
fn clearing_a_snapshot_keeps_what_the_platform_published_into_it() {
    // The bug this is here for made `Input::cursor` permanently `Free` for
    // every game in the workspace. The order of a frame is: take the snapshot,
    // ask the game what it wants the pointer to do, tell the platform, and
    // write back what actually took -- and `Devices::snapshot` begins by
    // clearing, so the write-back was wiped before anybody could read it. A
    // game asking whether its lock had been granted got "no" for ever.
    //
    // The distinction the fix rests on: everything else here is a *device
    // reading*, which a fresh reading replaces. The pointer's mode and the
    // window's size are platform state published from the other direction, and
    // nothing refills them on a frame where they did not change.
    let mut input = Input::new(action::SETS);
    input.set_digital(action::PLACE, Digital::HELD);
    input.set_cursor(Cursor::Locked);

    input.clear();

    assert_eq!(
        input.cursor(),
        Cursor::Locked,
        "the pointer is still locked; nothing has unlocked it",
    );
    // And the device readings did go, which is what clearing is for. The
    // viewport goes with them, because it is written afresh from the target
    // every frame and has nothing to preserve.
    assert_eq!(input.digital(action::PLACE), Digital::RELEASED);
    assert_eq!(input.viewport(), None);
}

#[test]
fn a_snapshot_over_an_empty_table_answers_with_the_released_value() {
    let input = Input::new(&[]);

    assert_eq!(input.digital(DigitalId(0)), Digital::RELEASED);
    assert_eq!(input.pose(PoseId(0)), None);
    assert!(input.descriptor(input.active_set()).is_none());
}

#[test]
fn the_default_of_each_value_type_is_the_silenced_one() {
    // Which is what makes the mask cheap to reason about: the value a query
    // gives for an action it will not answer for is the same value the type
    // would have held if nothing had ever touched it.
    assert_eq!(Digital::default(), Digital::RELEASED);
    assert_eq!(Analog::default(), Analog::ZERO);
    assert_ne!(Digital::HELD, Digital::RELEASED);
}

/// Saturating a displacement lands on the type's own end, not one step past it.
///
/// `Signed16` is `SNORM`, which spends one bit pattern twice: `i16::MIN` and
/// `-32767` both denote `-1.0`, and the workspace's rule is that raw bits may
/// carry the denormal in from outside but arithmetic never produces one. An
/// `i16::saturating_add` on the raw patterns stops at `i16::MIN` and breaks
/// that; the type's own stops at `-1.0`.
///
/// The two compare and hash alike, so nothing here would have noticed --
/// which is exactly why it is worth a test rather than a reading.
#[test]
fn a_saturated_displacement_is_canonical() {
    let mut unspent = Input::new(action::SETS);
    let mut frame = Input::new(action::SETS);
    let full = Analog::new(Signed16::MIN, Signed16::MIN);

    for _ in 0..2 {
        frame.set_delta(action::LOOK, full);
        unspent.absorb(&frame);
    }

    let piled = unspent.delta(action::LOOK);
    assert_eq!(piled, full, "a saturated sum should still be a full push");
    assert!(!piled.x.is_denormal(), "x carried the denormal encoding");
    assert!(!piled.y.is_denormal(), "y carried the denormal encoding");
    assert_eq!(piled.x.to_bits(), Signed16::MIN.to_bits());
}
