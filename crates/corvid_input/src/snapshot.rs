//! One frame's worth of input, and the two value types it is made of.

use alloc::vec;
use alloc::{string::String, vec::Vec};

use corvid_fixed::Signed16;

use crate::cursor::Cursor;
use crate::id::{AnalogId, DigitalId, PoseId, SetId};
use crate::sets::SetDescriptor;
use crate::source::Button;
use corvid_transform::GlobalFineTransform;

/// One on-or-off action, with the two edges around it.
///
/// `held` is the level and the other two are edges: `pressed` on the frame the
/// action went down, `released` on the frame it came up. A game that only wants
/// the level reads `held`, and a game that wants "this frame, once" reads
/// `pressed` and does not have to remember last frame's answer.
///
/// No combination is rejected, and one that looks wrong is not. A tap that
/// starts and finishes inside one frame arrives as `pressed` and `released`
/// with `held` false, which is the honest report of what happened and is
/// exactly the event a game must not miss. Producing the edges is the job of
/// whatever fills the snapshot — a device layer, or a test — and this type only
/// carries them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Digital {
    /// Whether the action is down now.
    pub held: bool,
    /// Whether it went down between the last frame and this one.
    pub pressed: bool,
    /// Whether it came up between the last frame and this one.
    pub released: bool,
}

impl Digital {
    /// Down, with no edge — the steady state of a held button.
    pub const HELD: Self = Self {
        held: true,
        pressed: false,
        released: false,
    };

    /// Up, with no edge.
    ///
    /// This is what a query about an action outside the active set answers
    /// with, and what [`Default`] gives.
    pub const RELEASED: Self = Self {
        held: false,
        pressed: false,
        released: false,
    };
}

/// One two-axis action, read either as a deflection or as a displacement.
///
/// Both axes are [`Signed16`], which covers `-1.0 ..= 1.0` exactly and is
/// integer storage — a stick position that reached a game as `f32` would be a
/// different number on a machine that rounded differently, and the whole point
/// of this crate is to be the last thing between a device and a deterministic
/// tick.
///
/// A one-axis action is one of these with `y` at zero; there is no separate
/// type for it, because a trigger and a stick differ in what they are bound to
/// rather than in what they carry.
///
/// **What one of these means depends on which accessor it came out of**, and
/// that is the whole of why there are two. [`Input::analog`] answers a
/// *deflection*: how far a control is pushed, which is a rate, and which the
/// frame's `dt` multiplies. [`Input::delta`] answers a *displacement*: how far
/// something moved during the frame, which is a quantity already proportional
/// to how long the frame lasted, and which `dt` must not multiply again. The
/// type is the same because the storage is; the two are never mixed in one
/// slot, because a binding fills one accessor or the other and never both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Analog {
    /// The horizontal axis, positive to the right.
    pub x: Signed16,
    /// The vertical axis, positive up.
    pub y: Signed16,
}

impl Analog {
    /// Centred.
    ///
    /// This is what a query about an action outside the active set answers
    /// with, and what [`Default`] gives.
    pub const ZERO: Self = Self {
        x: Signed16::ZERO,
        y: Signed16::ZERO,
    };

    /// An analog value from the two axes.
    #[must_use]
    #[inline]
    pub const fn new(x: Signed16, y: Signed16) -> Self {
        Self { x, y }
    }
}

/// The rectangle a pointer is reported against, in physical pixels.
///
/// This is here rather than in a window crate because it is the other half of
/// [`Input::pointer`]: a pointer arrives in the window's own normalised
/// coordinates, and the only thing that turns one back into pixels is the size
/// of the thing it was normalised against. A game handed the first without the
/// second can tell which button the pointer is nearest and cannot tell how many
/// pixels wide that button is.
///
/// Physical pixels, so a game that lays its interface out in them lays it out
/// at the size the display actually has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Viewport {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
}

impl Viewport {
    /// A viewport from its two numbers.
    #[must_use]
    #[inline]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Whether either dimension is zero, which is what a minimised window
    /// reports and what no layout can be solved in.
    #[must_use]
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// One frame of input, as data.
///
/// A snapshot holds a value for every action in the declaration and answers
/// queries about the actions of the **active set only**. Everything else reads
/// as [`Digital::RELEASED`], [`Analog::ZERO`] or `None`, whatever the device is
/// doing and whatever the action last read as. That is what lets a console
/// overlay a game's set without either knowing about the other: the console
/// activates its own set, the game's `if input.digital(action::PLACE).pressed`
/// stops firing, and neither of them had to be told.
///
/// The values behind an inactive set are kept rather than cleared, so
/// activating the set again reads the device as it is now rather than as
/// whatever the last frame before the overlay saw. An overlay is a view of the
/// device, not an edit of it.
///
/// This crate holds no devices. Filling a snapshot is the platform half's job;
/// what is here is the shape the two halves meet at, and it is `no_std` for the
/// same reason `corvid_behavior` is — the path from a snapshot to one player's
/// action for one tick may not need an operating system.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Input {
    sets: &'static [SetDescriptor],
    active: SetId,
    digital: Vec<Digital>,
    analog: Vec<Analog>,
    delta: Vec<Analog>,
    poses: Vec<Option<GlobalFineTransform>>,
    pointer: Option<Analog>,
    cursor: Cursor,
    viewport: Option<Viewport>,
    captured: Option<Button>,
    focus: Digital,
    text: String,
}

impl Input {
    /// An empty snapshot over a declaration's table, with the first set active.
    ///
    /// The table is the `SETS` that [`action_sets!`](crate::action_sets)
    /// generated. Storage is sized from it once, here, so no query allocates
    /// and no query has to decide what to do about an identifier that has never
    /// been written.
    ///
    /// The first set is active because a snapshot has to answer for something
    /// and the first declared set is the one a game reaches for first; call
    /// [`activate`](Self::activate) to say otherwise. A table with no sets in it
    /// leaves nothing active, and every query answers with the released value.
    #[must_use]
    pub fn new(sets: &'static [SetDescriptor]) -> Self {
        let mut digital = 0usize;
        let mut analog = 0usize;
        let mut poses = 0usize;

        for set in sets {
            digital = digital.max(end_of(set.digital()));
            analog = analog.max(end_of(set.analog()));
            poses = poses.max(end_of(set.pose()));
        }

        Self {
            sets,
            active: sets.first().map_or(SetId(0), |set| set.id()),
            digital: vec![Digital::RELEASED; digital],
            analog: vec![Analog::ZERO; analog],
            delta: vec![Analog::ZERO; analog],
            poses: vec![None; poses],
            pointer: None,
            cursor: Cursor::Free,
            viewport: None,
            captured: None,
            focus: Digital::RELEASED,
            text: String::new(),
        }
    }

    /// Whether this window has the player's attention, and the two edges
    /// around it.
    ///
    /// A [`Digital`] because that is exactly the shape of it — a level with an
    /// edge either side — and inventing a second type spelled the same way
    /// would buy nothing:
    ///
    /// | | Means |
    /// |---|---|
    /// | `held` | the window has focus now |
    /// | `pressed` | it got focus between the last frame and this one |
    /// | `released` | it lost focus |
    ///
    /// It is **not an action**, so it is not filtered by the active set and no
    /// binding table decides it: there is nothing for a player to bind, and a
    /// game asking about focus is asking about the window rather than about
    /// something they did.
    ///
    /// # What it is for
    ///
    /// Two things, and both are about the pointer. A game that captures the
    /// pointer wants to take it back when the player returns — `pressed` is
    /// that frame — and wants to know it was taken away when they leave, which
    /// no key release will tell it, because the platform stops reporting those
    /// the moment focus goes. That is also why the runtime releases everything
    /// held on focus loss, and this is the same event said out loud so that a
    /// game can act on it rather than infer it.
    ///
    /// A run with no window never gains focus, which is honest: there is
    /// nothing there to be focused. A game keying its pointer off this simply
    /// never asks for it, and a headless run has no pointer either.
    #[must_use]
    #[inline]
    pub const fn focus(&self) -> Digital {
        self.focus
    }

    /// Records whether the window has focus.
    ///
    /// The platform half calls this; a game reads [`focus`](Self::focus).
    #[inline]
    pub const fn set_focus(&mut self, focus: Digital) {
        self.focus = focus;
    }

    /// Which control the player pressed this frame, whatever it is bound to.
    ///
    /// **The one place a raw control reaches a game, and it exists for exactly
    /// one screen.** Everything else here is an *action*: a game declares what
    /// it can be asked to do and never sees a key code, which is what lets a
    /// binding table sit between the two and what makes a game playable on a
    /// board it was not written for. A rebinding screen is the one thing that
    /// cannot work that way, because "press the control you want" is a question
    /// about the control and not about the action, and until this existed the
    /// screen in `cradle_ui` could only list what was already bound.
    ///
    /// [`None`] on a frame where nothing went down, and on every frame of a run
    /// with no devices under it. When several controls went down together this
    /// is the lowest of them in [`Button`]'s own order, so a frame that saw two
    /// presses reports the same one on every machine rather than whichever the
    /// platform mentioned first.
    ///
    /// A press, never a release and never a level: a screen that bound on
    /// release would bind the control the player let go of to dismiss it.
    ///
    /// **It is not filtered by the active set**, because it is not an action
    /// and belongs to no set. A game that reads this while the player is not
    /// rebinding gets whatever they last pressed, which is why it is read
    /// inside a capture mode and not beside the other queries.
    #[must_use]
    #[inline]
    pub const fn captured(&self) -> Option<Button> {
        self.captured
    }

    /// Records the control that went down this frame.
    ///
    /// The platform half calls this; a game reads [`captured`](Self::captured).
    #[inline]
    pub const fn set_captured(&mut self, control: Option<Button>) {
        self.captured = control;
    }

    /// What the pointer is actually doing.
    ///
    /// **What happened, not what was asked for.** A game requests a mode
    /// through `Present::cursor` and the platform may decline — pointer locking
    /// is a permission in a browser, a protocol extension on Wayland, and a
    /// compositor's choice elsewhere — so this is where a game finds out. The
    /// runtime falls back down [`Cursor::fallback`] rather than failing, so
    /// asking for [`Cursor::Locked`] on a platform that refuses gives
    /// [`Cursor::Confined`] here rather than [`Cursor::Free`].
    ///
    /// Reading it matters for one thing above the rest: while
    /// [`Cursor::is_locked`] is true, [`pointer`](Self::pointer) stops moving
    /// and [`delta`](Self::delta) is the whole of what the mouse says. A game
    /// that assumed the lock took and steers from `pointer` has a camera that
    /// stops at the edge of the monitor.
    ///
    /// A headless run answers [`Cursor::Free`], because there is no pointer to
    /// be doing anything else.
    #[must_use]
    #[inline]
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// Records what the platform did with the pointer.
    ///
    /// The platform half calls this; a game reads [`cursor`](Self::cursor).
    #[inline]
    pub const fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
    }

    /// The declaration this snapshot was built over.
    #[must_use]
    #[inline]
    pub const fn sets(&self) -> &'static [SetDescriptor] {
        self.sets
    }

    /// The set whose actions the queries answer for.
    #[must_use]
    #[inline]
    pub const fn active_set(&self) -> SetId {
        self.active
    }

    /// Makes `set` the one the queries answer for.
    ///
    /// Nothing stored is disturbed. A set that names no descriptor in the table
    /// is accepted and answers for nothing, which is how a layer that wants
    /// every action silenced says so.
    #[inline]
    pub const fn activate(&mut self, set: SetId) {
        self.active = set;
    }

    /// The descriptor of `set`, if the table has one.
    #[must_use]
    pub fn descriptor(&self, set: SetId) -> Option<SetDescriptor> {
        self.sets.iter().copied().find(|found| found.id() == set)
    }

    /// The state of a digital action.
    ///
    /// [`Digital::RELEASED`] when the action does not belong to the active set,
    /// and when it belongs to no set at all.
    #[must_use]
    pub fn digital(&self, id: DigitalId) -> Digital {
        if self.owns(SetDescriptor::digital, id.0) {
            self.digital
                .get(usize::from(id.0))
                .copied()
                .unwrap_or(Digital::RELEASED)
        } else {
            Digital::RELEASED
        }
    }

    /// How far a control is pushed: a **deflection**, in `-1.0 ..= 1.0`.
    ///
    /// A rate. A stick held half over means "turn at half speed", so what reads
    /// this multiplies it by the frame's `dt`.
    ///
    /// [`Analog::ZERO`] when the action does not belong to the active set, when
    /// it belongs to no set at all, and — the case worth knowing about — when
    /// the action is bound to something that reports a *displacement* rather
    /// than a deflection, which answers on [`delta`](Self::delta) instead.
    #[must_use]
    pub fn analog(&self, id: AnalogId) -> Analog {
        if self.owns(SetDescriptor::analog, id.0) {
            self.analog
                .get(usize::from(id.0))
                .copied()
                .unwrap_or(Analog::ZERO)
        } else {
            Analog::ZERO
        }
    }

    /// How far something moved during the frame: a **displacement**, as a
    /// fraction of a full sweep.
    ///
    /// A quantity, and already integrated over the frame it happened in. The
    /// pixels a mouse reported are proportional to how long that frame lasted,
    /// so what reads this adds it as it stands and does **not** multiply by
    /// `dt`. Multiplying anyway turns a camera by the square of the frame time,
    /// which reads as a smooth sweep at a steady frame rate and as shake the
    /// moment the rate wobbles.
    ///
    /// [`Analog::ZERO`] under the same three conditions as
    /// [`analog`](Self::analog), the third of them the other way round: an
    /// action bound to a stick answers there and reads zero here. That is
    /// deliberate and is the point of the split — reaching for the wrong
    /// accessor is a value that stays still, which is a mistake that finds
    /// itself, rather than a camera whose behaviour depends on the frame rate.
    #[must_use]
    pub fn delta(&self, id: AnalogId) -> Analog {
        if self.owns(SetDescriptor::analog, id.0) {
            self.delta
                .get(usize::from(id.0))
                .copied()
                .unwrap_or(Analog::ZERO)
        } else {
            Analog::ZERO
        }
    }

    /// The transform of a tracked pose.
    ///
    /// `None` when the pose does not belong to the active set, when it belongs
    /// to no set at all, and when it belongs to the active set but is not being
    /// tracked this frame. A caller that has to tell the last case from the
    /// first two compares [`active_set`](Self::active_set) against the
    /// descriptor itself; a caller drawing a hand does not care, which is why
    /// the three collapse here.
    #[must_use]
    pub fn pose(&self, id: PoseId) -> Option<GlobalFineTransform> {
        if self.owns(SetDescriptor::pose, id.0) {
            self.poses.get(usize::from(id.0)).copied().flatten()
        } else {
            None
        }
    }

    /// Where the pointer is, if there is one.
    ///
    /// A mouse, a touch, or a ray cast from a tracked controller, in whatever
    /// normalized space the platform half hands over. It is not an action and
    /// belongs to no set, so it is not silenced by activating another one: a
    /// console overlay wants the cursor as much as the game did.
    #[must_use]
    #[inline]
    pub const fn pointer(&self) -> Option<Analog> {
        self.pointer
    }

    /// Records the state of a digital action.
    ///
    /// An identifier the table does not name is ignored, because there is
    /// nowhere to put it and no query that could read it back.
    #[inline]
    pub fn set_digital(&mut self, id: DigitalId, value: Digital) {
        if let Some(slot) = self.digital.get_mut(usize::from(id.0)) {
            *slot = value;
        }
    }

    /// Records the deflection of an analog action. An unnamed identifier is
    /// ignored.
    #[inline]
    pub fn set_analog(&mut self, id: AnalogId, value: Analog) {
        if let Some(slot) = self.analog.get_mut(usize::from(id.0)) {
            *slot = value;
        }
    }

    /// Records the displacement of an analog action over this frame. An
    /// unnamed identifier is ignored.
    #[inline]
    pub fn set_delta(&mut self, id: AnalogId, value: Analog) {
        if let Some(slot) = self.delta.get_mut(usize::from(id.0)) {
            *slot = value;
        }
    }

    /// Records a tracked pose, or its absence. An unnamed identifier is
    /// ignored.
    #[inline]
    pub fn set_pose(&mut self, id: PoseId, value: Option<GlobalFineTransform>) {
        if let Some(slot) = self.poses.get_mut(usize::from(id.0)) {
            *slot = value;
        }
    }

    /// Records where the pointer is, or that there is none.
    #[inline]
    pub const fn set_pointer(&mut self, value: Option<Analog>) {
        self.pointer = value;
    }

    /// How big the rectangle the pointer is reported against is, if there is
    /// one.
    ///
    /// [`None`] for a run with no display — a headless determinism check, a
    /// dedicated server — and that is the honest answer rather than a
    /// placeholder size: a run with no window has no viewport, and a game that
    /// was handed a made-up one would lay its interface out for a display
    /// nobody is looking at. A game that needs a rectangle either way picks its
    /// own logical size for the [`None`] case and says so.
    #[must_use]
    #[inline]
    pub const fn viewport(&self) -> Option<Viewport> {
        self.viewport
    }

    /// Records how big that rectangle is, or that there is no display.
    ///
    /// The platform half calls this; a game reads
    /// [`viewport`](Self::viewport).
    #[inline]
    pub const fn set_viewport(&mut self, value: Option<Viewport>) {
        self.viewport = value;
    }

    /// Folds a freshly read snapshot into this one, keeping whatever has not
    /// been spent yet.
    ///
    /// **Levels** — `held`, the deflections, the poses, the pointer — say what
    /// the device is doing *now*, so `fresh` replaces them outright.
    /// **Events** — `pressed`, `released` and the displacements — describe an
    /// interval, so they add up: the result is every edge and every millimetre
    /// since the last [`settle`](Self::settle), not just the ones in the last
    /// reading.
    ///
    /// This is what a loop that reads its devices more often than it ticks
    /// needs, and there is no such loop that does not. A window ends the edge
    /// interval once per displayed frame; at a fifteen-hertz tick on a
    /// sixty-hertz display three frames in four owe no tick at all, so a
    /// snapshot that was replaced rather than folded would drop the `pressed`
    /// of any tap that started and finished between two ticks — "exactly the
    /// event a game must not miss", as [`Digital`] puts it — along with
    /// three-quarters of the mouse.
    ///
    /// Displacements saturate rather than wrap, so an interval nobody ticked
    /// for a long time reads as a full sweep rather than as a sweep the other
    /// way.
    ///
    /// Values `fresh` has no slot for are left alone, which is what a snapshot
    /// over a shorter table amounts to.
    pub fn absorb(&mut self, fresh: &Self) {
        for (mine, theirs) in self.digital.iter_mut().zip(&fresh.digital) {
            mine.held = theirs.held;
            mine.pressed |= theirs.pressed;
            mine.released |= theirs.released;
        }
        for (mine, theirs) in self.analog.iter_mut().zip(&fresh.analog) {
            *mine = *theirs;
        }
        for (mine, theirs) in self.delta.iter_mut().zip(&fresh.delta) {
            *mine = Analog::new(
                Signed16::from_bits(mine.x.to_bits().saturating_add(theirs.x.to_bits())),
                Signed16::from_bits(mine.y.to_bits().saturating_add(theirs.y.to_bits())),
            );
        }
        for (mine, theirs) in self.poses.iter_mut().zip(&fresh.poses) {
            *mine = *theirs;
        }
        // A level with two edges, folded exactly as a digital action is: the
        // freshest level wins and neither edge is lost, so a player who
        // alt-tabbed away and back between two ticks is reported as having done
        // both rather than as never having left.
        self.focus.held = fresh.focus.held;
        self.focus.pressed |= fresh.focus.pressed;
        self.focus.released |= fresh.focus.released;
        self.pointer = fresh.pointer;
        // An edge, like `pressed`: the first control of the interval is kept,
        // so a press that happened between two ticks is not dropped by a later
        // reading that saw nothing. `settle` is what spends it.
        self.captured = self.captured.or(fresh.captured);
        // An interval, like the edges above: what was typed over the whole
        // stretch since the last `settle`, in the order it was typed. A
        // snapshot that replaced this rather than appending would drop three
        // keystrokes in four at fifteen hertz on a sixty-hertz display.
        self.text.push_str(&fresh.text);
        // A level, like the deflections: the pointer is still in whatever mode
        // it is in, so the freshest reading is the whole answer.
        self.cursor = fresh.cursor;
        // And so is the window's size — a resize is a state the display is in
        // rather than an interval something happened over.
        self.viewport = fresh.viewport;
    }

    /// Ends the interval the edges and the displacements describe, spending
    /// them.
    ///
    /// `pressed`, `released`, [`captured`](Self::captured), the two edges of
    /// [`focus`](Self::focus) and every displacement go to nothing. `held`, the
    /// deflections, the poses, the pointer, the cursor's mode and the viewport
    /// stay, because they are levels and the device is still doing them.
    ///
    /// The counterpart of [`absorb`](Self::absorb): a loop calls this once the
    /// tick that was owed the edge has run, which is what stops a frame that
    /// owes eight catch-up ticks from turning one keypress into eight actions.
    /// Between them the two make an edge reach exactly one tick, however many
    /// readings or ticks a frame happens to hold.
    pub fn settle(&mut self) {
        for slot in &mut self.digital {
            slot.pressed = false;
            slot.released = false;
        }
        self.delta.fill(Analog::ZERO);
        self.captured = None;
        self.focus.pressed = false;
        self.focus.released = false;
        self.text.clear();
    }

    /// Returns every device reading to released, zero and absent, keeping the
    /// pointer's mode, the capacity
    /// and the active set.
    ///
    /// This is for a runtime that holds one snapshot forever and refills it,
    /// which is what keeps a per-frame allocation out of the loop. It is not
    /// what switching sets does — that is [`activate`](Self::activate), and it
    /// deliberately leaves the values alone.
    ///
    /// # The cursor survives, and the bug that says why
    ///
    /// [`cursor`](Self::cursor) is **not a device reading**. Everything else
    /// here is something a device did, which a fresh reading replaces; the
    /// pointer's mode is platform state published *into* the snapshot from the
    /// other direction, and nothing refills it on a frame where it did not
    /// change.
    ///
    /// Clearing it made [`cursor`](Self::cursor) permanently
    /// [`Free`](Cursor::Free) for every game in the workspace. The order of a
    /// frame is: take the snapshot, ask the game what it wants the pointer to
    /// do, tell the platform, and **write back what actually took** — into this
    /// snapshot, where the next frame's [`Devices::snapshot`] wiped it before
    /// anybody could read it. A game asking whether its lock had been granted
    /// got "no" for ever, whatever the platform had actually done.
    /// `corvid_window/tests/cursor.rs` opens a real window and is what would
    /// have caught it.
    ///
    /// [`absorb`](Self::absorb) already treated it as a level for the same
    /// reason. This is the two agreeing.
    ///
    /// [`viewport`](Self::viewport) is cleared, and the difference is worth
    /// being exact about: it is written afresh every frame from the target's
    /// own size, so there is nothing to preserve and "no display" is the honest
    /// answer for a snapshot nobody has told.
    ///
    /// [`Devices::snapshot`]: crate::platform::Devices::snapshot
    pub fn clear(&mut self) {
        self.digital.fill(Digital::RELEASED);
        self.analog.fill(Analog::ZERO);
        self.delta.fill(Analog::ZERO);
        self.poses.fill(None);
        self.pointer = None;
        self.viewport = None;
        self.captured = None;
        self.text.clear();
    }

    /// What was typed over this interval, in the order it was typed.
    ///
    /// An **interval** quantity, exactly like
    /// [`pressed`](Digital::pressed): [`absorb`](Self::absorb) appends and
    /// [`settle`](Self::settle) clears, so a character delivered between two
    /// ticks reaches exactly one tick rather than none or eight. That is what
    /// makes typing into a text field work at a tick rate slower than the
    /// display's, which is every tick rate.
    ///
    /// # This is not an action, and it is not bound
    ///
    /// Nothing here goes through the binding table. A key that is bound to an
    /// action raises that action; what a platform decided the same keystroke
    /// *spells* is a separate question, answered by a keyboard layout, a
    /// modifier and possibly an input method, and only the platform can answer
    /// it. A game reading this is reading text; a game reading
    /// [`digital`](Self::digital) is reading intent.
    ///
    /// So this is whatever the platform committed, which is a whole grapheme
    /// cluster at a time rather than a code point: `"日本語"` arrives from one
    /// commit and not three.
    ///
    /// # It does not reach the simulation
    ///
    /// Not by itself. Like everything else in a snapshot, what crosses into a
    /// tick is the `Action` a controller
    /// built — so a game with a chat box puts the finished line in an action
    /// of its own rather than putting keystrokes on the wire.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Appends what a platform says was typed.
    ///
    /// Called by the window layer from a key event's committed text and from an
    /// input method's commit, and by a test standing in for either.
    pub fn type_text(&mut self, text: &str) {
        self.text.push_str(text);
    }

    /// Whether the active set owns `id` in the kind `range` picks out.
    fn owns(&self, range: impl Fn(SetDescriptor) -> crate::IdRange, id: u16) -> bool {
        self.descriptor(self.active)
            .is_some_and(|set| range(set).contains(id))
    }
}

/// One past the last identifier of a range, as a length.
fn end_of(range: crate::IdRange) -> usize {
    usize::from(range.first()) + usize::from(range.count())
}
