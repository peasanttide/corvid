#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

//! What was typed, folded across frames exactly as an edge is.

use corvid_input::{Input, SetDescriptor};

/// A game with nothing to press. Text is not an action, so it needs no set.
static SETS: &[SetDescriptor] = &[];

#[test]
fn text_accumulates_across_frames_and_is_spent_by_one_tick() {
    let mut unspent = Input::new(SETS);

    let mut first = Input::new(SETS);
    first.type_text("he");
    unspent.absorb(&first);

    let mut second = Input::new(SETS);
    second.type_text("llo");
    unspent.absorb(&second);

    // Two display frames and one tick between them: the tick sees the word.
    assert_eq!(unspent.text(), "hello");

    unspent.settle();
    assert_eq!(
        unspent.text(),
        "",
        "an interval that has been spent is empty"
    );
}

/// The failure this exists to prevent.
///
/// A character delivered on a frame that owed no tick would be dropped by a
/// snapshot that replaced rather than folded — which at fifteen hertz on a
/// sixty-hertz display is three keystrokes in four.
#[test]
fn a_character_typed_between_two_ticks_is_not_lost() {
    let mut unspent = Input::new(SETS);

    // Three display frames, only the last of which owes a tick.
    for letter in ["a", "b", "c"] {
        let mut frame = Input::new(SETS);
        frame.type_text(letter);
        unspent.absorb(&frame);
    }

    assert_eq!(unspent.text(), "abc");
}

/// And not delivered twice, which is the other half.
#[test]
fn a_character_reaches_exactly_one_tick() {
    let mut unspent = Input::new(SETS);
    let mut frame = Input::new(SETS);
    frame.type_text("x");
    unspent.absorb(&frame);

    assert_eq!(unspent.text(), "x");
    unspent.settle();

    // A frame that saw nothing new must not resurrect it, however many ticks
    // the runtime owes.
    unspent.absorb(&Input::new(SETS));
    assert_eq!(unspent.text(), "");
}

/// Order is preserved, because a word is not a set.
#[test]
fn the_letters_arrive_in_the_order_they_were_typed() {
    let mut unspent = Input::new(SETS);
    for letter in ["c", "o", "r", "v", "i", "d"] {
        let mut frame = Input::new(SETS);
        frame.type_text(letter);
        unspent.absorb(&frame);
    }
    assert_eq!(unspent.text(), "corvid");
}

/// A whole grapheme cluster arrives at once, because a platform hands over what
/// its input method committed rather than one code point at a time.
#[test]
fn text_is_whatever_the_platform_committed() {
    let mut unspent = Input::new(SETS);
    let mut frame = Input::new(SETS);
    frame.type_text("日本語");
    unspent.absorb(&frame);
    assert_eq!(unspent.text(), "日本語");
}

/// `clear` is the runtime refilling a snapshot it holds forever, and text is a
/// device reading like any other.
#[test]
fn clearing_a_snapshot_empties_the_text_too() {
    let mut input = Input::new(SETS);
    input.type_text("gone");
    input.clear();
    assert_eq!(input.text(), "");
}
