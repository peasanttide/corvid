//! Which control means which action.

use alloc::vec::Vec;
use core::num::NonZeroU32;

use crate::id::{AnalogId, DigitalId};
use crate::sets::SetDescriptor;
use crate::source::{Axis, Button, Key, PadButton};

/// Which of a snapshot's two analog accessors a binding answers on.
///
/// A stick and a mouse are not the same kind of number. A stick reports a
/// *deflection*, which is a rate: how fast to turn, so the frame's `dt`
/// multiplies it. A mouse reports the motion that already happened, which is a
/// *quantity*: the pixels a frame accumulated are already proportional to how
/// long that frame lasted, so multiplying by `dt` again turns a camera by the
/// square of the frame time -- smoothly at a steady frame rate, and visibly as
/// jitter the moment the rate wobbles.
///
/// One accessor for both is therefore a bug generator, and this is what tells
/// them apart. A binding fills one accessor and leaves the other at
/// [`Analog::ZERO`](crate::Analog), so an action bound the wrong way round is a
/// value that stays still rather than a camera whose feel depends on the
/// display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[non_exhaustive]
pub enum Reading {
    /// A level the control holds: a stick, a trigger, a pad. Answers on
    /// [`Input::analog`](crate::Input::analog).
    Deflection,
    /// Motion since the last snapshot: a mouse, a wheel, a trackball. Answers
    /// on [`Input::delta`](crate::Input::delta).
    Displacement,
}

/// How much of a device's own unit makes one full sweep of an analog action,
/// and which accessor that action answers on.
///
/// A mouse reports pixels and a wheel reports detents, and neither is a number
/// an action can be expressed in: an action's axes run `-1.0 ..= 1.0`. The span
/// is the divisor between the two, and it is an integer so that the conversion
/// is integer arithmetic from end to end -- a sensitivity that arrived as an
/// `f32` would be a different number on a machine that parsed it differently,
/// and this crate is the last thing between a device and a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct AxisBinding {
    /// Which control.
    pub axis: Axis,
    /// Which action it drives.
    pub action: AnalogId,
    /// How many of the device's own units make a full sweep. More than that
    /// within one frame clamps rather than wrapping.
    pub span: NonZeroU32,
    /// Which accessor the action answers on, and therefore what the number
    /// means.
    pub reading: Reading,
}

/// Which component of a two-axis action a binding drives.
///
/// A key pair makes one axis move, not two, so a binding built out of buttons
/// has to say which. A stick says nothing because it already has both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
#[non_exhaustive]
pub enum Component {
    /// The horizontal axis, positive to the right.
    X,
    /// The vertical axis, positive up.
    Y,
}

/// Two buttons standing in for one axis of a stick.
///
/// `W` and `S` are how a player without a pad pushes a stick forwards and
/// backwards, and this is the layer that says so -- once, in the binding table,
/// rather than in every game that wants to be playable both ways. A game reads
/// [`Input::analog`](crate::Input::analog) and cannot tell which the player
/// used.
///
/// # It is always a deflection, and there is no field to say otherwise
///
/// A held key means "keep going", which is a *rate*: it answers on
/// [`Input::analog`] and the frame's `dt` multiplies it. That is not a default --
/// it is the only reading a pair can honestly have, because a button reports no
/// quantity for a displacement to be made of. Letting a table say
/// [`Reading::Displacement`](Reading) here would be offering a way to build a
/// control that moves by the square of the frame time.
///
/// [`Input::analog`]: crate::Input::analog
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct PairBinding {
    /// The button that pushes the axis negative: down, or left.
    pub low: Button,
    /// The one that pushes it positive: up, or right.
    pub high: Button,
    /// Which action it drives.
    pub action: AnalogId,
    /// Which of that action's two components.
    pub component: Component,
}

/// The table between controls and actions.
///
/// A game never sees a [`Button`] or an [`Axis`]; it declares actions with
/// [`action_sets!`](crate::action_sets) and reads them from an
/// [`Input`](crate::Input). This is the layer in between, and it is data -- a
/// binding file is this table written down.
///
/// # What the type system enforces, and what the caller owes
///
/// The type system enforces that a [`Button`] can only be bound to a
/// [`DigitalId`] and an [`Axis`] only to an [`AnalogId`], because the two
/// identifier spaces are separate types. Everything else is owed by the caller:
/// nothing here checks that an identifier names a declared action, that two
/// actions in the same set are not bound to the same key, or that every action
/// a game reads has been bound at all. An unbound action reads as
/// [`Digital::RELEASED`](crate::Digital) or [`Analog::ZERO`](crate::Analog)
/// forever, which is indistinguishable from a player not touching it. Nor is
/// the [`Reading`] a binding declares checked against what the control actually
/// reports -- nothing here reads a device, so there is nothing to check it
/// against.
///
/// Several controls may drive one action, and one control may drive several.
/// The first is how a game is playable with either hand; the second is how a
/// modifier and a chord are expressed without this table learning what either
/// word means.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize, ::serde::Deserialize))]
pub struct Bindings {
    /// Every button binding, in the order it was added.
    buttons: Vec<(Button, DigitalId)>,
    /// Every axis binding, in the order it was added.
    axes: Vec<AxisBinding>,
    /// Every pair of buttons standing in for an axis, in the order it was
    /// added.
    pairs: Vec<PairBinding>,
}

/// The controls [`Bindings::placeholder`] hands out, in order.
///
/// A key *and* the pad button a player would expect to do the same job, so the
/// table is worth something to somebody holding a controller rather than only
/// to somebody at a board. Both are bound to the one action, which is the same
/// union that makes "either shift" one action.
///
/// The keys are chosen so that the first few land under the left hand on a
/// board the player is already resting on, and so that no two are the same key
/// with a modifier. The pad side leads with the face buttons in the order a
/// player reaches for them and then the shoulders.
///
/// Two families are deliberately absent. The arrows and the d-pad are how a
/// player expects to *move*, and handing them out as the eleventh and
/// twelfth action of whatever order a declaration happens to be written in is
/// how a placeholder table turns into a game that walks when you shoot; a game
/// that wants them says so with [`Bindings::pair`]. [`PadButton::Guide`] is the
/// system button, which belongs to the platform rather than to any game.
const PLACEHOLDER_BUTTONS: &[(Key, PadButton)] = &[
    (Key::Space, PadButton::South),
    (Key::E, PadButton::West),
    (Key::Q, PadButton::North),
    (Key::R, PadButton::East),
    (Key::F, PadButton::RightBumper),
    (Key::C, PadButton::LeftBumper),
    (Key::V, PadButton::RightTrigger),
    (Key::Enter, PadButton::Start),
    (Key::Escape, PadButton::Select),
    (Key::Tab, PadButton::LeftTrigger),
];

/// Turns a literal into a span without a panic in sight.
///
/// The workspace denies `panic`, `unwrap`, `expect` and `unreachable` alike, so
/// a non-zero constant cannot be written down with any of the usual four. One
/// is the smallest span there is, so a constant somebody edited to zero becomes
/// the twitchiest binding in the table rather than a build that stops -- which
/// is the right trade for two literals in this module and would not be for a
/// number arriving from a file.
const fn span(units: u32) -> NonZeroU32 {
    match NonZeroU32::new(units) {
        Some(span) => span,
        None => NonZeroU32::MIN,
    }
}

/// How many pixels of mouse motion [`Bindings::placeholder`] treats as a full
/// sweep.
///
/// A sixth of the width of a 1920-pixel window, so a full reading is a sweep
/// across a sixth of the screen rather than a twitch. It is a guess at what
/// feels right and not a measurement of anything; a per-device sensitivity
/// curve is a game's own to apply.
const PLACEHOLDER_MOTION_SPAN: NonZeroU32 = span(320);

/// How many wheel detents [`Bindings::placeholder`] treats as a full sweep.
const PLACEHOLDER_SCROLL_SPAN: NonZeroU32 = span(8);

/// What [`Bindings::placeholder`] treats as a full stick deflection.
///
/// A pad reports a stick as a level already at the end of its own range, so
/// this is the span that leaves it alone: one whole unit in, one whole sweep
/// out. It is a span rather than nothing because [`axis`](Bindings::axis) takes
/// one, and a placeholder that halved a player's stick would look like a broken
/// pad rather than a table nobody wrote.
const PLACEHOLDER_STICK_SPAN: NonZeroU32 = span(1);

impl Bindings {
    /// A table that binds nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buttons: Vec::new(),
            axes: Vec::new(),
            pairs: Vec::new(),
        }
    }

    /// Makes `button` drive `action`.
    #[must_use]
    pub fn button(mut self, button: Button, action: DigitalId) -> Self {
        self.buttons.push((button, action));
        self
    }

    /// Makes `axis` drive `action`, with `span` of the device's own units to a
    /// full sweep, read as `reading` says.
    ///
    /// The `reading` is not a detail of the control alone. It says which of the
    /// snapshot's two analog accessors this action answers on, and therefore
    /// whether what a game reads is a rate to be multiplied by `dt` or a
    /// quantity that must not be -- which is the distinction [`Reading`] exists
    /// for.
    #[must_use]
    pub fn axis(
        mut self,
        axis: Axis,
        action: AnalogId,
        span: NonZeroU32,
        reading: Reading,
    ) -> Self {
        self.axes.push(AxisBinding {
            axis,
            action,
            span,
            reading,
        });
        self
    }

    /// Every button binding, in the order it was added.
    #[must_use]
    #[inline]
    pub fn buttons(&self) -> &[(Button, DigitalId)] {
        &self.buttons
    }

    /// Every axis binding, in the order it was added.
    #[must_use]
    #[inline]
    pub fn axes(&self) -> &[AxisBinding] {
        &self.axes
    }

    /// Makes `low` and `high` stand in for one component of `action`, the way
    /// a stick pushed either way would.
    ///
    /// Always a [`Reading::Deflection`], for the reason [`PairBinding`] gives:
    /// a held button is a rate, and there is no quantity in it for a
    /// displacement to be made of.
    ///
    /// Both held is centred, which is the same answer a stick gives when
    /// nobody is pushing it -- and it is exact, so a player pressing left and
    /// right together does not creep.
    ///
    /// ```
    /// use corvid_input::{AnalogId, Button, Key};
    /// use corvid_input::platform::{Bindings, Component};
    ///
    /// // The four keys every game has used for thirty years, as one stick.
    /// let table = Bindings::new()
    ///     .pair(Button::key(Key::A), Button::key(Key::D), AnalogId(0), Component::X)
    ///     .pair(Button::key(Key::S), Button::key(Key::W), AnalogId(0), Component::Y);
    /// assert_eq!(table.pairs().len(), 2);
    /// ```
    #[must_use]
    pub fn pair(
        mut self,
        low: Button,
        high: Button,
        action: AnalogId,
        component: Component,
    ) -> Self {
        self.pairs.push(PairBinding {
            low,
            high,
            action,
            component,
        });
        self
    }

    /// Every pair binding, in the order it was added.
    #[must_use]
    #[inline]
    pub fn pairs(&self) -> &[PairBinding] {
        &self.pairs
    }

    /// A table over `sets` that lets a game be played before anybody has bound
    /// anything.
    ///
    /// **This is a placeholder and is documented as one everywhere it is
    /// named.** It binds by *number*: the digital actions of the declaration
    /// take a fixed list of keys -- and the matching pad buttons -- in order,
    /// the first analog action takes mouse motion and the right stick, the
    /// second takes the wheel and the left stick, and everything past that is
    /// unbound. It therefore has no idea what any of those actions
    /// mean, and the key a player ends up pressing is an accident of where the
    /// action was declared.
    ///
    /// What it is for is the one thing it is honestly good for: a game with a
    /// window opens and something happens when a key is pressed, before its
    /// author has written a table. It is what
    /// [`Controller::bindings`] defaults to, and a game with a
    /// player in front of it overrides that with a table of its own, written
    /// out with [`button`](Self::button) and [`axis`](Self::axis) -- where each
    /// control means what a hundred other games have taught that player it
    /// means. `corvid_app` then reads the player's own file over the top.
    ///
    /// [`Controller::bindings`]: https://docs.rs/corvid_control
    ///
    /// Actions past the end of either list are left unbound rather than wrapped
    /// around, because a key that means two things is worse than a key that
    /// means nothing.
    #[must_use]
    pub fn placeholder(sets: &[SetDescriptor]) -> Self {
        let mut digital = 0u32;
        let mut analog = 0u32;
        for set in sets {
            digital =
                digital.max(u32::from(set.digital().first()) + u32::from(set.digital().count()));
            analog = analog.max(u32::from(set.analog().first()) + u32::from(set.analog().count()));
        }

        let mut table = Self::new();
        for (id, (key, pad)) in (0..digital).zip(PLACEHOLDER_BUTTONS.iter().copied()) {
            let Ok(id) = u16::try_from(id) else { break };
            table = table
                .button(Button::key(key), DigitalId(id))
                .button(Button::pad(pad), DigitalId(id));
        }
        // The mouse and the wheel report motion that already happened, so they
        // answer on `Input::delta`; a stick is a level and answers on
        // `Input::analog`. An action bound to both therefore answers on both,
        // by whichever device the player is actually touching -- so a game
        // reading this table on either kind of hardware reads *both* accessors
        // and adds them. That is the cost of a table that does not know what
        // the player is holding, and it is the reason a real game writes its
        // own rather than shipping this one.
        if analog > 0 {
            table = table
                .axis(
                    Axis::MouseMotion,
                    AnalogId(0),
                    PLACEHOLDER_MOTION_SPAN,
                    Reading::Displacement,
                )
                .axis(
                    Axis::RightStick,
                    AnalogId(0),
                    PLACEHOLDER_STICK_SPAN,
                    Reading::Deflection,
                );
        }
        if analog > 1 {
            table = table
                .axis(
                    Axis::Scroll,
                    AnalogId(1),
                    PLACEHOLDER_SCROLL_SPAN,
                    Reading::Displacement,
                )
                .axis(
                    Axis::LeftStick,
                    AnalogId(1),
                    PLACEHOLDER_STICK_SPAN,
                    Reading::Deflection,
                );
        }
        table
    }
}
