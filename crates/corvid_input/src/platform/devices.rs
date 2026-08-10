//! What the devices are doing, accumulated between two snapshots.
//!
//! This is where an edge comes from. A platform reports a key going down and
//! later reports it coming up; an [`Input`] carries `held`, `pressed` and
//! `released` for one frame, and working out the second and third from a stream
//! of the first is the whole job here.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use core::cmp::Ordering;

use crate::id::{AnalogId, DigitalId};
use crate::platform::bind::{Bindings, Component, Reading};
use crate::snapshot::Input;
use crate::source::{Axis, Button};
use crate::value::{Analog, Digital};
use corvid_fixed::Signed16;

/// What the devices are doing, and what they did since the last snapshot.
///
/// A platform's event handler drives this: [`press`](Self::press) and
/// [`release`](Self::release) as buttons move, [`moved`](Self::moved) as
/// relative axes report, and [`snapshot`](Self::snapshot) once per frame to
/// turn all of it into an [`Input`].
///
/// # What a snapshot consumes and what it leaves
///
/// A *level* survives a snapshot and a *delta* does not. Which button is down,
/// how far a stick is pushed and where the pointer is are levels: they are
/// still true after the frame that read them, because nothing has happened to
/// change them. The edges and the accumulated motion are deltas: they describe
/// an interval, the snapshot ends that interval, and a second snapshot taken
/// with no events in between reports no edges and no motion. Getting that
/// backwards is a mouse that keeps turning the camera after the player stopped
/// moving it, which is why `tests/edges.rs` and `tests/motion.rs` assert both
/// halves.
///
/// That distinction is the same one [`Reading`] draws, and it is why the two
/// analog kinds are accumulated separately here: [`moved`](Self::moved) adds up
/// a displacement and [`deflected`](Self::deflected) replaces a level.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Devices {
    /// Which buttons are down now.
    held: BTreeSet<Button>,
    /// Which went down since the last snapshot.
    pressed: BTreeSet<Button>,
    /// Which came up since the last snapshot.
    released: BTreeSet<Button>,
    /// How far each relative axis has moved since the last snapshot.
    motion: BTreeMap<Axis, [i32; 2]>,
    /// How far each absolute axis is pushed now.
    deflection: BTreeMap<Axis, [i32; 2]>,
    /// Where the pointer is, if the platform says.
    pointer: Option<Analog>,
    /// What has been typed since the last snapshot, in order.
    ///
    /// Not a button and not bound to anything: a key that raises an action
    /// raises it through the table above, and what the same keystroke *spells*
    /// is a separate question only the platform -- with a layout, a modifier and
    /// possibly an input method -- can answer.
    typed: String,
    /// Whether the window has focus, and the two edges around it.
    ///
    /// A level like the deflections, with edges like the buttons, which is why
    /// it is a [`Digital`] rather than a `bool`: a game that wants to take the
    /// pointer back when the player returns needs the *frame* they returned on.
    focus: Digital,
}

impl Default for Devices {
    fn default() -> Self {
        Self::new()
    }
}

impl Devices {
    /// Devices with nothing down, nothing moved and no pointer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            held: BTreeSet::new(),
            pressed: BTreeSet::new(),
            released: BTreeSet::new(),
            motion: BTreeMap::new(),
            deflection: BTreeMap::new(),
            pointer: None,
            // Not focused until the platform says so. A window that has never
            // been told is a window nobody is looking at, and the alternative --
            // assuming focus -- would have a game take the pointer on the first
            // frame of a run that started in the background.
            focus: Digital::RELEASED,
            typed: String::new(),
        }
    }

    /// Records what the platform says was typed.
    ///
    /// Whole commits rather than code points: a key event's committed text, or
    /// an input method's, which is one grapheme cluster at a time. Nothing here
    /// consults the binding table, because text is not an action -- see
    /// [`Input::text`](crate::Input::text).
    pub fn typed(&mut self, text: &str) {
        self.typed.push_str(text);
    }

    /// Records that `button` went down.
    ///
    /// A repeat -- the platform reporting a key that is already held, which
    /// every desktop does while a key is left down -- leaves the edge alone, so
    /// `pressed` is true on one frame rather than on every frame of a hold.
    pub fn press(&mut self, button: Button) {
        if self.held.insert(button) {
            self.pressed.insert(button);
        }
    }

    /// Records that `button` came up.
    ///
    /// A release of something that was not down is recorded anyway, because it
    /// is what the platform said and because the alternative is deciding that
    /// the platform is wrong.
    pub fn release(&mut self, button: Button) {
        self.held.remove(&button);
        self.released.insert(button);
    }

    /// Records that `axis` moved by `dx`, `dy` in the device's own units.
    ///
    /// Accumulated rather than replaced: a mouse that reports four events
    /// between two frames moved the sum of them, and a frame that took twice as
    /// long sees twice the motion. Saturating, so a device that reports an
    /// absurd delta pins the axis instead of wrapping it.
    pub fn moved(&mut self, axis: Axis, dx: i32, dy: i32) {
        let total = self.motion.entry(axis).or_insert([0, 0]);
        total[0] = total[0].saturating_add(dx);
        total[1] = total[1].saturating_add(dy);
    }

    /// Records that `axis` is now pushed to `x`, `y` in the device's own units.
    ///
    /// Replaced rather than accumulated, and it survives a snapshot: a stick
    /// held over is still held over on the next frame whether or not the
    /// platform said so again. That is the opposite of [`moved`](Self::moved)
    /// in both respects, which is the whole difference between a deflection and
    /// a displacement.
    ///
    /// Nothing in this workspace calls this, because nothing in it reads a
    /// device that reports a level. It is here because the accessor it feeds
    /// is, and because a
    /// [`Reading::Deflection`](crate::platform::Reading) binding with nowhere to
    /// take its value from would be a half of the split that could not be
    /// tested.
    pub fn deflected(&mut self, axis: Axis, x: i32, y: i32) {
        self.deflection.insert(axis, [x, y]);
    }

    /// Records that the window gained or lost the player's attention.
    ///
    /// The edge is only raised when the level actually changes, because a
    /// platform reports focus far more often than it changes -- and a game
    /// taking the pointer back on every `pressed` would take it back on every
    /// frame.
    pub const fn focused(&mut self, focused: bool) {
        if focused == self.focus.held {
            return;
        }
        self.focus.held = focused;
        if focused {
            self.focus.pressed = true;
        } else {
            self.focus.released = true;
        }
    }

    /// Records where the pointer is, or that there is none.
    ///
    /// A level rather than a delta: it survives a snapshot, because a mouse
    /// that has not moved is still where it was.
    pub const fn point(&mut self, pointer: Option<Analog>) {
        self.pointer = pointer;
    }

    /// Records that everything came up at once.
    ///
    /// What a window does when it loses focus. The platform stops reporting key
    /// releases the moment the player switches away, so a game that was not
    /// told would come back to find a key held down that nobody is touching.
    /// Every held button gets its release edge, which is what a game watching
    /// for `released` expects, and the accumulated motion is dropped because
    /// motion that happened over another window is not motion in this one.
    /// Every deflection goes to centred for the same reason a key goes up: the
    /// platform has stopped saying, and a stick that stayed pushed would turn
    /// the camera for as long as the player was away.
    pub fn released_all(&mut self) {
        for button in core::mem::take(&mut self.held) {
            self.released.insert(button);
        }
        self.motion.clear();
        self.deflection.clear();
    }

    /// Whether `button` is down now.
    #[must_use]
    pub fn is_held(&self, button: Button) -> bool {
        self.held.contains(&button)
    }

    /// Fills `into` from `bindings` and ends the interval.
    ///
    /// Every value in `into` is written, including the ones no binding names --
    /// the snapshot is cleared first, so an action that stopped being bound
    /// reads released rather than keeping whatever it last said.
    ///
    /// Several bindings may name one action, and the action is the union: down
    /// if any of them is down, pressed if any of them went down. That is what
    /// makes "either shift" one action rather than two.
    ///
    /// An analog binding writes the accessor its [`Reading`] names and leaves
    /// the other one at [`Analog::ZERO`], because the snapshot is cleared first
    /// and nothing else writes it. So an action bound only to a stick reads
    /// zero from [`Input::delta`], one bound only to a mouse reads zero from
    /// [`Input::analog`], and reading the wrong one is a value that stays still.
    ///
    /// Several analog bindings on one action are a union too, per accessor:
    /// each component takes whichever binding is further from rest. A control
    /// that is not being moved therefore contributes nothing rather than
    /// zeroing what another control just said, so "either hand" works for a
    /// stick and a mouse the way it does for two shift keys, and the order the
    /// table lists them in does not change what a frame reads.
    pub fn snapshot(&mut self, bindings: &Bindings, into: &mut Input) {
        into.clear();

        // Before the bindings, because nothing binds it: this is what the
        // platform committed rather than what any action was raised by.
        into.type_text(&self.typed);
        self.typed.clear();

        let mut union: BTreeMap<DigitalId, Digital> = BTreeMap::new();
        for &(button, action) in bindings.buttons() {
            let value = union.entry(action).or_insert(Digital::RELEASED);
            value.held |= self.held.contains(&button);
            value.pressed |= self.pressed.contains(&button);
            value.released |= self.released.contains(&button);
        }
        for (action, value) in union {
            into.set_digital(action, value);
        }

        // Unioned rather than assigned, for the same reason the buttons above
        // are. Writing each binding straight into the snapshot made the *last*
        // one win, so a second control bound to an action overwrote the first
        // with `Analog::ZERO` on every frame it was not itself being moved --
        // the mouse would stop working because a wheel was also bound to look,
        // and swapping the order of the two `axis` calls would fix it, which is
        // not a thing a binding table should depend on.
        //
        // The largest deflection wins per component, which is the analogue of
        // "down if any of them is down": whichever control the player is
        // actually pushing is the one the action follows.
        let mut levels: BTreeMap<AnalogId, Analog> = BTreeMap::new();
        let mut deltas: BTreeMap<AnalogId, Analog> = BTreeMap::new();
        for binding in bindings.axes() {
            let span = i64::from(binding.span.get());
            let fraction = |units: i32| Signed16::saturating_from_ratio(i64::from(units), span);
            let (raw, union) = match binding.reading {
                Reading::Deflection => (self.deflection.get(&binding.axis), &mut levels),
                Reading::Displacement => (self.motion.get(&binding.axis), &mut deltas),
            };
            let value = raw.copied().unwrap_or([0, 0]);
            let read = Analog::new(fraction(value[0]), fraction(value[1]));
            let slot = union.entry(binding.action).or_insert(Analog::ZERO);
            *slot = Analog::new(further(slot.x, read.x), further(slot.y, read.y));
        }
        // A pair of buttons is a stick pushed either way, so it joins the
        // *deflections*, and joins them through the same union as everything
        // else: a player with a hand on the keys and a hand on a stick has one
        // action following whichever is pushed further.
        //
        // Both buttons held is exactly centred rather than "whichever was
        // pressed first", so pressing left and right together stands still
        // instead of creeping.
        for pair in bindings.pairs() {
            let pushed = match (
                self.held.contains(&pair.low),
                self.held.contains(&pair.high),
            ) {
                (true, false) => Signed16::MIN,
                (false, true) => Signed16::MAX,
                _ => Signed16::ZERO,
            };
            let slot = levels.entry(pair.action).or_insert(Analog::ZERO);
            *slot = match pair.component {
                Component::X => Analog::new(further(slot.x, pushed), slot.y),
                Component::Y => Analog::new(slot.x, further(slot.y, pushed)),
            };
        }
        for (action, value) in levels {
            into.set_analog(action, value);
        }
        for (action, value) in deltas {
            into.set_delta(action, value);
        }

        into.set_pointer(self.pointer);
        // The raw control, beside the actions and not instead of them. It is
        // not looked up in the table and is not filtered by the active set,
        // because a rebinding screen is asking which control was pressed and
        // the answer must include the controls that are bound to nothing --
        // those are exactly the ones a player is most likely to be binding.
        //
        // `first` rather than any of them, because a `BTreeSet` is ordered and
        // two controls pressed inside one frame should report the same one on
        // every machine.
        into.set_captured(self.pressed.first().copied());
        // Not looked up in the table and not filtered by the active set: focus
        // is a property of the window rather than an action, so there is
        // nothing for a player to bind it to.
        into.set_focus(self.focus);

        self.focus.pressed = false;
        self.focus.released = false;
        self.pressed.clear();
        self.released.clear();
        self.motion.clear();
    }
}

/// Whichever of two readings of one action is further from rest.
///
/// The analog union rule, and the counterpart of `held |= held` for a button.
/// Magnitude rather than signed maximum, because an axis runs both ways and a
/// stick pushed fully left has to beat a mouse that did not move.
///
/// A tie in magnitude is broken towards the greater value rather than towards
/// whichever reading arrived first, which is what makes this commutative and so
/// makes the fold over the table independent of the order the table lists its
/// bindings in. Two controls on one axis pushed equally far in opposite
/// directions is a reachable tie rather than a contrived one, and answering it
/// by position would mean the same two controls read one way or the other
/// depending on which line of a binding file was written first.
fn further(one: Signed16, two: Signed16) -> Signed16 {
    match two.abs().cmp(&one.abs()) {
        Ordering::Greater => two,
        Ordering::Equal if two > one => two,
        _ => one,
    }
}
