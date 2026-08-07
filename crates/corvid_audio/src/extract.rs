//! An [`AudioFrame`] turned into the notes a mixer has to start.
//!
//! This is the whole of the decision half of the crate and it has no device in
//! it, which is what makes it testable on a machine with no speakers. It runs
//! on the game's thread, so it may allocate and may take as long as a linear
//! scan of a small table takes; what crosses to the device afterwards is a list
//! of [`Note`]s with nothing left to resolve.

use corvid_sound::{AudioFrame, BusId, Cue};

use crate::catalogue::Catalogue;
use crate::heard::Heard;
use crate::mixer::Note;

/// How far a bus chain is followed before it is called a cycle.
///
/// `corvid_sound` stores the bus graph and never walks it, and says so: a bus
/// may name itself as its parent, or two may name each other, and the frame
/// carries that faithfully. So the walk is bounded by the number of buses that
/// exist, which is the length of the longest chain that is not a cycle.
const fn depth(frame: &AudioFrame) -> usize {
    frame.buses.len()
}

/// The gain a bus chain applies, from `bus` up to whatever it feeds.
///
/// A bus the frame does not contain contributes nothing rather than silencing
/// what names it. That is the friendlier of the two readings and it is a
/// reading rather than a rule: a frame naming a bus it does not carry is
/// already telling a backend something incomplete, and a game that lost every
/// sound because a bus was left out of a list would be much harder to diagnose
/// than one that lost a volume trim.
///
/// A cycle stops after as many steps as there are buses, with the gains it had
/// multiplied so far. There is no right answer for a cycle; this one is finite,
/// which is the property that matters on a device thread's critical path.
fn bus_gain(frame: &AudioFrame, bus: BusId) -> f32 {
    let mut gain = 1.0;
    let mut at = Some(bus);
    for _ in 0..=depth(frame) {
        let Some(id) = at else { break };
        let Some(found) = frame.buses.iter().find(|bus| bus.id == id) else {
            break;
        };
        gain *= found.gain.to_f32();
        at = found.parent;
    }
    gain
}

/// What one cue is worth, before the listener.
fn cue_gain(frame: &AudioFrame, cue: &Cue) -> f32 {
    cue.gain.to_f32() * bus_gain(frame, cue.bus)
}

/// Appends a [`Note`] for every cue in `frame` that has not been started.
///
/// `into` is cleared first, and is meant to be a buffer a caller holds across
/// frames so that a displayed frame with nothing new in it costs no allocation.
///
/// # What is used, and what is carried and ignored
///
/// A cue's `gain`, its bus chain's gains, the listener's gain and its `pitch`
/// are used. Its `position` is **not**: this crate does not spatialize, and the
/// offset a frame carries reaches the device only in the sense that it was
/// there and was not read. `sound` is resolved through `catalogue`, which
/// describes a sound rather than naming a recording, because there are no
/// recordings.
///
/// A [`Source`](corvid_sound::Source) is not a cue and is not played at all: it
/// is a voice a backend holds open across frames, which needs a loop, an
/// envelope and a rule for when a voice that has left the list stops. None of
/// those is here.
///
/// Pitch multiplies the timbre's frequency rather than a playback rate, because
/// there is no recording to play faster or slower. A cue at
/// [`I8F8::ONE`](corvid_fixed::I8F8) sounds at the described frequency.
pub fn notes(frame: &AudioFrame, catalogue: &Catalogue, heard: &mut Heard, into: &mut Vec<Note>) {
    into.clear();
    let listener = frame.listener.gain.to_f32();
    for cue in &frame.cues {
        if !heard.is_new(cue.id) {
            continue;
        }
        let mut timbre = catalogue.timbre(cue.sound);
        let pitch = cue.pitch.to_f32();
        if pitch.is_finite() && pitch > 0.0 {
            timbre.hertz *= pitch;
        }
        into.push(Note {
            timbre,
            gain: cue_gain(frame, cue) * listener,
        });
    }
}

#[cfg(test)]
mod tests {
    //! Which numbers reach a note and which are carried past it.

    #![allow(
        clippy::panic,
        reason = "a failed assertion in a test is a failed test, which is what a test is for"
    )]

    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::{bus_gain, notes};
    use crate::{Catalogue, Heard, Note, Timbre};
    use corvid_fixed::{Factor16, I8F8, I16F16};
    use corvid_sound::{AudioFrame, Bus, BusId, Cue, Listener, SoundId};
    use corvid_time::Tick;
    use corvid_vector::FinePoint;
    /// How long the bus walk below is given to answer before the test calls it
    /// non-terminating.
    ///
    /// Generous by three orders of magnitude — the walk is a handful of
    /// comparisons — so that a loaded build machine cannot fail on it while an
    /// unbounded walk still cannot pass.
    const DEADLINE: Duration = Duration::from_secs(5);

    /// The sound every fixture fires.
    const THUD: SoundId = SoundId(2);

    /// A frame with one cue on tick 97, at full gain everywhere.
    fn one_bounce() -> AudioFrame {
        let mut frame = AudioFrame::new();
        let id = frame.next_id(Tick(97));
        frame.cue(Cue::new(id, THUD));
        frame
    }

    fn extracted(frame: &AudioFrame) -> Vec<Note> {
        let mut out = Vec::new();
        notes(frame, &Catalogue::new(), &mut Heard::new(64), &mut out);
        out
    }

    #[test]
    fn a_cue_becomes_one_note_at_the_gain_it_asked_for() {
        let mut frame = one_bounce();
        let notes = extracted(&frame);
        assert_eq!(notes.len(), 1);
        assert!((notes.first().map_or(0.0, |note| note.gain) - 1.0).abs() < 1e-4);

        // Half the cue gain is half the note's, which is the whole of "apply
        // gain" for a frame with no buses.
        frame.cues.clear();
        let id = frame.next_id(Tick(97));
        frame.cue(Cue::new(id, THUD).with_gain(Factor16::from_f64(0.5)));
        let half = extracted(&frame);
        assert!((half.first().map_or(0.0, |note| note.gain) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn the_listener_and_the_bus_chain_both_multiply() {
        // Three gains that a backend reading only one of them would get wrong,
        // and each is a different half so no two can be swapped without the
        // product changing.
        let mut frame = AudioFrame::new();
        frame.listen(Listener::default().with_gain(Factor16::from_f64(0.5)));
        frame.bus(
            Bus::new(BusId(1))
                .under(BusId::MASTER)
                .with_gain(Factor16::from_f64(0.25)),
        );
        frame.bus(Bus::new(BusId::MASTER).with_gain(Factor16::from_f64(0.5)));
        let id = frame.next_id(Tick(97));
        frame.cue(
            Cue::new(id, THUD)
                .on(BusId(1))
                .with_gain(Factor16::from_f64(0.5)),
        );

        let gain = extracted(&frame).first().map_or(0.0, |note| note.gain);
        assert!(
            (gain - 0.031_25).abs() < 1e-3,
            "the four gains multiplied to {gain}",
        );
    }

    #[test]
    fn a_bus_the_frame_does_not_carry_is_a_missing_trim_and_not_a_silence() {
        let mut frame = AudioFrame::new();
        let id = frame.next_id(Tick(97));
        frame.cue(Cue::new(id, THUD).on(BusId(9)));
        let gain = extracted(&frame).first().map_or(0.0, |note| note.gain);
        assert!((gain - 1.0).abs() < 1e-4, "a missing bus gave {gain}");
    }

    #[test]
    fn a_bus_cycle_is_finite() {
        // `corvid_sound` accepts a cycle and says so, so this is the case a
        // backend has to survive rather than one it can refuse.
        //
        // The bug this guards against is a walk that never ends, and a test
        // that called `bus_gain` on this thread would report that bug by
        // hanging — which is not a failing test, it is a suite that never
        // finishes and a person who has to work out which of two hundred tests
        // stopped. So the walk happens on a thread of its own with a deadline
        // on the answer, and an unbounded walk is a named failure in a bounded
        // time.
        let mut frame = AudioFrame::new();
        frame.bus(
            Bus::new(BusId(1))
                .under(BusId(2))
                .with_gain(Factor16::from_f64(0.5)),
        );
        frame.bus(
            Bus::new(BusId(2))
                .under(BusId(1))
                .with_gain(Factor16::from_f64(0.5)),
        );

        let (answer, wait) = mpsc::channel();
        let walked = thread::spawn(move || {
            let _ = answer.send(bus_gain(&frame, BusId(1)));
        });
        let Ok(gain) = wait.recv_timeout(DEADLINE) else {
            // The walking thread is left running deliberately: it cannot be
            // stopped from here, and the process is about to end.
            panic!("the bus walk did not answer within {DEADLINE:?}, so it does not terminate");
        };
        assert!(walked.join().is_ok(), "the bus walk panicked");

        assert!(gain.is_finite());
        // The value the bounded walk multiplies, and not merely that it is in
        // range: three buses' worth of steps at a half each — the walk runs
        // `depth + 1` times over a frame of two buses — is an eighth, and a
        // walk that stopped one step early or ran one step late would give a
        // quarter or a sixteenth. The tolerance is wide enough for
        // `Factor16::from_f64(0.5)` being 32768/65535 rather than a half, and
        // narrow enough that a step either way is a failure.
        assert!(
            (gain - 0.125).abs() < 1e-4,
            "a cycle of two halves multiplied to {gain}",
        );
    }

    #[test]
    fn pitch_moves_the_frequency_and_a_nonsense_pitch_does_not() {
        let mut frame = AudioFrame::new();
        let id = frame.next_id(Tick(97));
        frame.cue(Cue::new(id, THUD).with_pitch(I8F8::from_f64(2.0)));
        let described = Timbre::knock(100.0);
        let catalogue = Catalogue::new().with(THUD, described);
        let mut out = Vec::new();
        notes(&frame, &catalogue, &mut Heard::new(8), &mut out);
        assert_eq!(out.first().map(|note| note.timbre.hertz), Some(200.0));

        // A pitch of zero is a frequency of zero, which is silence dressed as a
        // sound. The described frequency is the honest answer.
        frame.cues.clear();
        let id = frame.next_id(Tick(97));
        frame.cue(Cue::new(id, THUD).with_pitch(I8F8::ZERO));
        notes(&frame, &catalogue, &mut Heard::new(8), &mut out);
        assert_eq!(out.first().map(|note| note.timbre.hertz), Some(100.0));
    }

    #[test]
    fn a_position_changes_nothing_which_is_the_stub_stated_as_a_test() {
        // The spatialization stub, asserted rather than described. When a
        // backend learns to place a sound this test is the one that fails, and
        // that is what it is for.
        let mut near = one_bounce();
        let mut far = AudioFrame::new();
        let id = far.next_id(Tick(97));
        far.cue(Cue::new(id, THUD).at(FinePoint::new(
            I16F16::from_f64(-30.0),
            I16F16::from_f64(12.0),
            I16F16::from_f64(4.0),
        )));
        assert_ne!(near.cues, far.cues);
        assert_eq!(extracted(&near), extracted(&far));
        near.clear();
    }

    #[test]
    fn a_source_is_carried_and_not_played() {
        use corvid_sound::{Source, SourceId};
        let mut frame = AudioFrame::new();
        frame.source(Source::new(SourceId(1), SoundId(4)));
        assert_eq!(frame.sources.len(), 1);
        assert!(extracted(&frame).is_empty());
    }

    #[test]
    fn a_buffer_handed_in_full_is_emptied_first() {
        // The caller is meant to keep one buffer across frames, so a frame with
        // nothing new in it has to leave it empty rather than replaying what
        // the last frame put there.
        let frame = one_bounce();
        let mut heard = Heard::new(64);
        let mut out = Vec::new();
        notes(&frame, &Catalogue::new(), &mut heard, &mut out);
        assert_eq!(out.len(), 1);
        notes(&frame, &Catalogue::new(), &mut heard, &mut out);
        assert!(out.is_empty());
    }
}
