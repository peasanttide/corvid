//! The client-local half: what a player presses, what the ears hear, and the
//! opening every run in this crate starts from.

use std::sync::Arc;

use corvid::{
    Acting, Auralizer, Camera, Controller, Cue, Digest, Extract, Extracting, FinePoint, Hearing,
    I16F16, Opening, Opens, Profile, ProfileId, Schema, Seed, SoundId, Tick, Updating,
    sound::Listener,
};

use crate::table::{COURT, Contact, Court, Move, Play, SEATS, Table};

/// The one place this game's actions are named.
///
/// Two of them, which is what pong is. A game declares what it can be asked to
/// do and never sees a key code; which control is bound to [`UP`](action::UP) is
/// the player's business and the platform's.
pub mod action {
    use corvid::platform::{Bindings, Button, Key, PadButton};

    corvid::action_sets! {
        pub set Playing {
            digital UP, DOWN, RELEASE;
        }
    }

    /// The table this game ships.
    ///
    /// `W`/`S`, the arrows, and a pad's left stick pressed as a d-pad — three
    /// ways to move one paddle, because
    /// [`Devices::snapshot`](corvid::platform::Devices::snapshot)
    /// unions them and the game reads one answer. Two players at one keyboard
    /// would want two sets and this game has one: the second player is on
    /// another machine, which is the entire point of it.
    #[must_use]
    pub fn bindings() -> Bindings {
        Bindings::new()
            .button(Button::key(Key::W), UP)
            .button(Button::key(Key::ArrowUp), UP)
            .button(Button::pad(PadButton::PadUp), UP)
            .button(Button::key(Key::S), DOWN)
            .button(Button::key(Key::ArrowDown), DOWN)
            .button(Button::pad(PadButton::PadDown), DOWN)
            .button(Button::key(Key::Escape), RELEASE)
            .button(Button::pad(PadButton::Start), RELEASE)
    }
}

/// What a paddle makes.
pub const KNOCK: SoundId = SoundId(1);

/// What a wall makes.
pub const THUD: SoundId = SoundId(2);

/// What a goal makes.
pub const CHIME: SoundId = SoundId(3);

/// How long the flash after a goal lasts, in seconds.
pub const FLASH: f32 = 0.6;

/// The player, and the paddle they move.
///
/// A person at a keyboard, and nothing else: a paddle nobody is behind is
/// [`Opponent`](crate::Opponent), which is a type of its own rather than a mode
/// of this one. What decides between them is the seat — this one plays the seat
/// the run claimed, and the bot plays whichever seats `--bots N` filled.
///
/// There is no camera: a court seen from above needs no eye to look through it,
/// so `look` answers the default. The flash after a goal is the renderer's,
/// because a flash is a picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hands;

impl Controller<Table> for Hands {
    /// Nothing to set. Which control raises which action is the binding table,
    /// which is a file of its own.
    type Config = ();

    /// What this game can be asked to do.
    const SETS: &'static [corvid::SetDescriptor] = action::SETS;

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    /// And which control does which of them, before the player has edited the
    /// file.
    fn bindings() -> corvid::platform::Bindings {
        action::bindings()
    }

    /// One tick's intent, from what is held down.
    ///
    /// **This is the whole of what goes on the wire.** It reads no camera and
    /// no window size, so a headless peer and a windowed one submit the same
    /// action for the same input — which is what makes the tests in this crate
    /// say anything about the game a player plays.
    ///
    /// Both directions held is [`Still`](Move::Still) rather than one of them
    /// winning, because a player rolling their hand across two keys should stop
    /// rather than lurch.
    fn action(&self, acting: Acting<'_, Table>) -> Move {
        match (
            acting.input.digital(action::UP).held,
            acting.input.digital(action::DOWN).held,
        ) {
            (true, false) => Move::Up,
            (false, true) => Move::Down,
            _ => Move::Still,
        }
    }

    /// Nothing accumulates: there is no camera to smooth and no cursor to cast.
    fn update(&mut self, _updating: Updating<'_, Table>) {}

    fn look(&self) -> Camera {
        Camera::default()
    }
}

/// The blip, the thud and the chime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ears {
    /// What the newest extracted state said had just happened.
    contact: Option<Contact>,
    /// Which tick it happened on, so a cue is numbered the same on every peer.
    at: Tick,
}

impl Extract<Table> for Ears {
    fn extract(&mut self, extracting: Extracting<'_, Table>) {
        self.contact = extracting.state.contact;
        self.at = extracting.state.now;
    }
}

impl Auralizer<Table> for Ears {
    type Config = ();

    fn new((): ()) -> Self {
        Self::default()
    }

    fn configure(&mut self, (): ()) {}

    /// Every sound is read out of [`Table::contact`], which is in the hashed
    /// state — so two peers play the same sounds on the same ticks, and a
    /// client that recomputed a hit from two ball positions would have to
    /// guess.
    fn hear(&mut self, hearing: Hearing<'_>) {
        // The listener is wherever the eye is, which for this game is the
        // middle of the court looking at it.
        hearing.out.listen(Listener::new(hearing.camera.pose));

        let Some(contact) = self.contact else {
            return;
        };
        let (sound, at) = match contact {
            Contact::Paddle { at, .. } => (KNOCK, at),
            Contact::Wall { at } => (THUD, at),
            Contact::Goal { .. } => (
                CHIME,
                FinePoint::new(I16F16::ZERO, I16F16::ZERO, I16F16::ZERO),
            ),
        };
        if let Some(offset) = hearing.camera.pose.to_fine_global(at.to_global_fine()) {
            let id = hearing.out.next_id(self.at);
            hearing.out.cue(Cue::new(id, sound).at(offset));
        }
    }
}

/// How this build describes its own types, which a capture records and a load
/// compares.
#[must_use]
pub fn schema() -> Digest {
    Schema::new("pong")
        .field("Table.ball", "Ball{at,velocity}")
        .field("Table.paddles", "[Paddle{at}; 2]")
        .field("Table.scores", "[u16; 2]")
        .field("Table.serve", "u16")
        .field("Table.towards", "bool")
        .field("Table.contact", "Option<Wall|Paddle|Goal>")
        .field("Table.now", "Tick")
        .field("Table.over", "Option<u8>")
        .field("Move", "Still | Up | Down")
        .digest()
}

/// The court every run in this crate is played on.
#[must_use]
pub const fn court() -> Court {
    Court {
        half: FinePoint::new(I16F16::from_f64(8.0), I16F16::from_f64(5.0), I16F16::ZERO),
        inset: I16F16::from_f64(0.5),
        paddle: FinePoint::new(I16F16::from_f64(0.15), I16F16::from_f64(0.9), I16F16::ZERO),
        ball: I16F16::from_f64(0.15),
        // A second at this game's rate, which is long enough to see the score
        // change and short enough not to be a wait.
        serve: 30,
    }
}

/// The tuning every run in this crate is played under.
#[must_use]
pub const fn rules() -> Play {
    Play {
        // A paddle crosses its half of the court in about a second and a half
        // at thirty ticks a second, which is where pong sits: fast enough to
        // reach a corner shot and slow enough that being out of position costs
        // the point.
        paddle_speed: I16F16::from_f64(0.22),
        serve_speed: I16F16::from_f64(0.20),
        serve_lift: I16F16::from_f64(0.07),
        speed_up: I16F16::from_f64(0.012),
        top_speed: I16F16::from_f64(0.45),
        spin: I16F16::from_f64(0.14),
        target: 5,
    }
}

/// The state a session opens on: the ball parked, about to be served.
#[must_use]
pub fn origin() -> Table {
    Table {
        serve: court().serve,
        towards: true,
        ..Table::default()
    }
}

/// The session every run in this crate starts from.
///
/// Two seats, always. The roster is what decides which paddle a
/// [`PlayerId`](corvid::PlayerId) moves, and it is fixed here rather than
/// assembled by a lobby because both peers are the same binary started twice.
#[must_use]
pub fn opening() -> Opening<Table> {
    Opening {
        level: COURT.to_owned(),
        content: Arc::new(court()),
        rules: Arc::new(rules()),
        roster: (0..SEATS)
            .map(|seat| Profile {
                account: ProfileId(seat as u64 + 1),
                joined: Tick::ZERO,
                left: None,
            })
            .collect(),
        // Nothing in this game reads the seed: there is no randomness in it,
        // which is deliberate. A desync in a game with an RNG is a hunt for
        // which peer drew a different number; a desync here can only be the
        // arithmetic.
        seed: Seed(0x00b0_a11d),
        first: Tick::ZERO,
        // `Some`, because pong's opening is not `Table::default()`: the ball
        // is parked with a serve pending. A game whose opening *is* its default
        // writes `None` here and states nothing.
        origin: Some(Arc::new(origin())),
        schema: schema(),
    }
}

/// The one thing this game states about how a run starts.
impl Opens for Table {
    fn opening() -> Opening<Self> {
        opening()
    }
}
