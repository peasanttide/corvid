//! What a snapshot answers, and — mostly — what it refuses to answer.

use corvid_fixed::I48F16;

use corvid_fixed::Signed16;
use corvid_input::{
    Analog, AnalogId, Cursor, Digital, DigitalId, IdRange, Input, PoseId, SetId, Viewport,
};
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
/// only ever be caught when it made the storage *too small* — and three of the
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
fn an_action_of_the_active_set_reads_what_was_recorded() {
    let mut input = Input::new(action::SETS);
    input.set_digital(action::PLACE, Digital::HELD);
    input.set_analog(action::LOOK, pushed());
    input.set_pose(action::POINTER, Some(somewhere()));

    assert_eq!(input.active_set(), action::Build::ID);
    assert_eq!(input.digital(action::PLACE), Digital::HELD);
    assert_eq!(input.analog(action::LOOK), pushed());
    assert_eq!(input.pose(action::POINTER), Some(somewhere()));
}

#[test]
fn the_two_analog_accessors_are_two_slots() {
    // A deflection and a displacement are different numbers about the same
    // action, and a snapshot that stored one of them would report a stick as
    // though it were a mouse. Both are written here and both are read back, so
    // a single slot behind two accessors would fail whichever was written
    // second.
    let mut input = Input::new(action::SETS);
    input.set_analog(action::LOOK, pushed());
    assert_eq!(input.delta(action::LOOK), Analog::ZERO);

    input.set_delta(action::LOOK, swept());
    assert_eq!(input.analog(action::LOOK), pushed());
    assert_eq!(input.delta(action::LOOK), swept());

    // And the displacement is silenced by an inactive set exactly as the
    // deflection is, because it is an action's value like any other.
    input.activate(action::Console::ID);
    assert_eq!(input.delta(action::LOOK), Analog::ZERO);
}

#[test]
fn an_action_outside_the_active_set_reads_released() {
    // Nothing has ever been recorded against `Console`'s actions, and the game
    // asks about them anyway. This is the plain case.
    let input = Input::new(action::SETS);

    assert_eq!(input.active_set(), action::Build::ID);
    assert_eq!(input.digital(action::SUBMIT), Digital::RELEASED);
    assert_eq!(input.analog(action::SCROLL), Analog::ZERO);
    assert_eq!(input.pose(action::CARET), None);
}

#[test]
fn an_action_outside_the_active_set_does_not_read_what_it_last_held() {
    // The neighbouring bug, and the one that actually bites: `PLACE` was held
    // while `Build` was active, the console then opened, and the game's
    // `if input.digital(PLACE).held` must stop firing on the frame the overlay
    // took over — not on the frame after, and not when the player happens to
    // let go. An implementation that only silences values arriving after the
    // switch passes the test above and fails this one.
    let mut input = Input::new(action::SETS);
    input.set_digital(action::PLACE, Digital::HELD);
    input.set_analog(action::LOOK, pushed());
    input.set_pose(action::POINTER, Some(somewhere()));
    assert_eq!(input.digital(action::PLACE), Digital::HELD);

    input.activate(action::Console::ID);

    assert_eq!(input.digital(action::PLACE), Digital::RELEASED);
    assert_eq!(input.analog(action::LOOK), Analog::ZERO);
    assert_eq!(input.pose(action::POINTER), None);
}

#[test]
fn the_edges_are_silenced_as_well_as_the_level() {
    // A press is the half of a digital action a game reacts to once, so an
    // overlay that silenced `held` and let `pressed` through would place a
    // building on the frame the console opened. `pressed` and `released` are
    // separate fields and a mask that only cleared `held` would leave them.
    let mut input = Input::new(action::SETS);
    input.set_digital(
        action::PLACE,
        Digital {
            held: true,
            pressed: true,
            released: false,
        },
    );

    input.activate(action::Console::ID);
    let hidden = input.digital(action::PLACE);

    assert!(!hidden.held);
    assert!(!hidden.pressed);
    assert!(!hidden.released);
}

#[test]
fn reactivating_a_set_reads_the_device_as_it_is_now() {
    // The other half of the overlay contract. Silencing is a view and not an
    // edit: the button is still down, so handing control back reads it as down.
    // An implementation that cleared the storage on the way out would report a
    // button the player never let go of as released, and the press edge that
    // would have told the game otherwise has already gone past.
    let mut input = Input::new(action::SETS);
    input.set_digital(action::PLACE, Digital::HELD);
    input.set_analog(action::LOOK, pushed());

    input.activate(action::Console::ID);
    input.activate(action::Build::ID);

    assert_eq!(input.digital(action::PLACE), Digital::HELD);
    assert_eq!(input.analog(action::LOOK), pushed());
}

#[test]
fn a_set_the_table_does_not_name_silences_everything() {
    let mut input = Input::new(action::SETS);
    input.set_digital(action::PLACE, Digital::HELD);
    input.set_digital(action::SUBMIT, Digital::HELD);

    input.activate(SetId(7));

    assert_eq!(input.digital(action::PLACE), Digital::RELEASED);
    assert_eq!(input.digital(action::SUBMIT), Digital::RELEASED);
    assert!(input.descriptor(SetId(7)).is_none());
}

#[test]
fn an_identifier_the_table_does_not_name_reads_released() {
    let mut input = Input::new(action::SETS);
    let unnamed = DigitalId(64);

    // Recording against it is ignored rather than growing the storage, which is
    // what keeps a query from having to decide what an unnumbered action means.
    input.set_digital(unnamed, Digital::HELD);

    assert_eq!(input.digital(unnamed), Digital::RELEASED);
    assert_eq!(input.pose(PoseId(64)), None);
}

#[test]
fn the_pointer_is_not_an_action_and_is_not_silenced() {
    // A console overlay wants the cursor as much as the game did, so the
    // pointer is deliberately outside the mask. It is the one query in this
    // file that reads the same whatever is active.
    let mut input = Input::new(action::SETS);
    input.set_pointer(Some(pushed()));

    assert_eq!(input.pointer(), Some(pushed()));
    input.activate(action::Console::ID);
    assert_eq!(input.pointer(), Some(pushed()));

    // Including under a set the table does not name, which silences every
    // action there is. "Not in the active set" and "in no set" are the two
    // ways a query gets silenced, and the pointer has to survive both or it is
    // an action after all.
    input.activate(SetId(7));
    assert_eq!(input.pointer(), Some(pushed()));

    input.set_pointer(None);
    assert_eq!(input.pointer(), None);
}

#[test]
fn a_pose_can_be_absent_while_its_set_is_active() {
    // Which is why `pose` returns an `Option` and the other two do not: a hand
    // outside the tracking volume has no transform, and that is not the same
    // situation as the set being inactive even though the answer is.
    let mut input = Input::new(action::SETS);
    input.set_pose(action::POINTER, Some(somewhere()));
    assert_eq!(input.pose(action::POINTER), Some(somewhere()));

    input.set_pose(action::POINTER, None);
    assert_eq!(input.pose(action::POINTER), None);
    assert_eq!(input.active_set(), action::Build::ID);
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
fn storage_covers_the_last_identifier_of_the_last_set() {
    // `Input::new` sizes itself from the table, and the identifier most likely
    // to fall off the end is the highest one. `DISMISS` is the last digital
    // action of the last set.
    let mut input = Input::new(action::SETS);
    input.activate(action::Console::ID);
    input.set_digital(action::DISMISS, Digital::HELD);

    assert_eq!(input.digital(action::DISMISS), Digital::HELD);
    assert_eq!(input.sets().len(), 2);
}

#[test]
fn the_declaration_that_tells_the_three_kinds_apart_really_does() {
    // The premise of the three tests below, checked rather than assumed: if
    // `Cockpit` ever gains or loses an action the ranges could start to overlap
    // again and those tests would go on passing while testing nothing.
    let turret = disjoint::SETS[1];

    assert_eq!(turret.digital(), IdRange::new(1, 3));
    assert_eq!(turret.analog(), IdRange::new(4, 2));
    assert_eq!(turret.pose(), IdRange::new(6, 1));

    // Which is to say: every identifier the active set owns of one kind is out
    // of range for both of the others, and the three runs are three lengths.
    for id in 0..8u16 {
        let owned = u32::from(turret.digital().contains(id))
            + u32::from(turret.analog().contains(id))
            + u32::from(turret.pose().contains(id));
        assert!(owned <= 1, "{id} is owned by more than one kind of range");
    }
}

#[test]
fn each_query_consults_the_range_of_its_own_kind() {
    // Recorded against the active set, and every value distinct from the
    // silenced one, so a query that consulted another kind's range would find
    // its identifier out of that range and answer with the silenced value.
    let mut input = Input::new(disjoint::SETS);
    input.activate(disjoint::Turret::ID);
    input.set_digital(disjoint::FIRE, Digital::HELD);
    input.set_analog(disjoint::TRAVERSE, pushed());
    input.set_pose(disjoint::GUNNER, Some(somewhere()));

    assert_eq!(input.digital(disjoint::FIRE), Digital::HELD);
    assert_eq!(input.analog(disjoint::TRAVERSE), pushed());
    assert_eq!(input.pose(disjoint::GUNNER), Some(somewhere()));

    // And the same for the identifier at the far end of each run, because a
    // range consulted for the wrong kind can still contain the low numbers.
    input.set_digital(disjoint::ZOOM_IN, Digital::HELD);
    input.set_analog(disjoint::ELEVATE, pushed());
    assert_eq!(input.digital(disjoint::ZOOM_IN), Digital::HELD);
    assert_eq!(input.analog(disjoint::ELEVATE), pushed());
}

#[test]
fn a_number_that_names_an_action_of_another_kind_is_still_silenced() {
    // The other direction. `TRAVERSE` is `AnalogId(4)`, and `DigitalId(4)` is
    // not an action of the active set — it is not an action at all. A query
    // that consulted the analog range would answer for it, so this fails on
    // exactly the mutation the test above fails on, from the other side.
    let mut input = Input::new(disjoint::SETS);
    input.activate(disjoint::Turret::ID);
    input.set_digital(DigitalId(4), Digital::HELD);
    input.set_analog(AnalogId(1), pushed());
    input.set_pose(PoseId(2), Some(somewhere()));

    assert_eq!(input.digital(DigitalId(4)), Digital::RELEASED);
    assert_eq!(input.analog(AnalogId(1)), Analog::ZERO);
    assert_eq!(input.pose(PoseId(2)), None);

    // Including the numbers `Cockpit` owns of each kind, which are the ones a
    // range check that ignored the active set would let through.
    input.set_digital(disjoint::EJECT, Digital::HELD);
    input.set_analog(disjoint::THROTTLE, pushed());
    input.set_pose(disjoint::HELMET, Some(somewhere()));

    assert_eq!(input.digital(disjoint::EJECT), Digital::RELEASED);
    assert_eq!(input.analog(disjoint::THROTTLE), Analog::ZERO);
    assert_eq!(input.pose(disjoint::HELMET), None);
}

#[test]
fn storage_covers_the_last_identifier_of_every_kind() {
    // `Input::new` sizes each kind's storage from that kind's totals, and the
    // identifier that falls off the end when it sizes one kind from another is
    // the highest one of the kind that came out short. Both declarations are
    // here because the three totals are ordered one way in `disjoint` and the
    // other way in `inverted`, and a swap that leaves a kind's storage too
    // *long* changes no answer at all — so each of the six swaps has to be the
    // one that comes out short in at least one of the two.
    let mut top = Input::new(disjoint::SETS);
    top.activate(disjoint::Turret::ID);
    top.set_digital(disjoint::ZOOM_IN, Digital::HELD);
    top.set_analog(disjoint::ELEVATE, pushed());
    top.set_pose(disjoint::GUNNER, Some(somewhere()));

    assert_eq!(top.digital(disjoint::ZOOM_IN), Digital::HELD);
    assert_eq!(top.analog(disjoint::ELEVATE), pushed());
    assert_eq!(top.pose(disjoint::GUNNER), Some(somewhere()));

    let mut bottom = Input::new(inverted::SETS);
    bottom.activate(inverted::Wand::ID);
    bottom.set_digital(inverted::MENU, Digital::HELD);
    bottom.set_analog(inverted::TWIST, pushed());
    bottom.set_pose(inverted::TIP, Some(somewhere()));

    assert_eq!(bottom.digital(inverted::MENU), Digital::HELD);
    assert_eq!(bottom.analog(inverted::TWIST), pushed());
    assert_eq!(bottom.pose(inverted::TIP), Some(somewhere()));
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
    // — so a snapshot that was replaced rather than folded would drop the tap
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

    // It is not an action, so activating another set does not silence it — a
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
    // write back what actually took — and `Devices::snapshot` begins by
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
