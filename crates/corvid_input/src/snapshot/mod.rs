//! One frame of input, and the questions an action set asks of it.

mod frame;
mod window;

use alloc::vec;
use alloc::{string::String, vec::Vec};

use crate::cursor::Cursor;
use crate::id::{AnalogId, DigitalId, PoseId, SetId};
use crate::sets::SetDescriptor;
use crate::source::Button;
use crate::value::{Analog, Digital, Viewport};
use corvid_transform::FineTransform;

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
/// same reason `corvid_behavior` is -- the path from a snapshot to one player's
/// action for one tick may not need an operating system.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Input {
    sets: &'static [SetDescriptor],
    active: SetId,
    digital: Vec<Digital>,
    analog: Vec<Analog>,
    delta: Vec<Analog>,
    poses: Vec<Option<FineTransform>>,
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
    /// it belongs to no set at all, and -- the case worth knowing about -- when
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
    /// deliberate and is the point of the split -- reaching for the wrong
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
    pub fn pose(&self, id: PoseId) -> Option<FineTransform> {
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
    pub fn set_pose(&mut self, id: PoseId, value: Option<FineTransform>) {
        if let Some(slot) = self.poses.get_mut(usize::from(id.0)) {
            *slot = value;
        }
    }

    /// Records where the pointer is, or that there is none.
    #[inline]
    pub const fn set_pointer(&mut self, value: Option<Analog>) {
        self.pointer = value;
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
