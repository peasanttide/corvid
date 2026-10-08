//! A host starts its lobby's session alone, its bot playing the empty seat;
//! a second run joins the seat, plays it a while and leaves; the bot keeps
//! the seat rather than it leaving the session; and a third run joins it
//! again, from where the bot had got to. Nobody's seat departs.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use std::sync::Arc;

use corvid_app::{App, Settings};
use corvid_behavior::{Command, ExitCode, PlayerState, ProfileId};
use corvid_control::net::{NetRequest, Stage};
use corvid_control::{Acting, Controller, Updating};
use corvid_replay::{Opening, Profile, Schema, Seed};
use corvid_time::{Clock, Tick, TickSpan, Ticks};
use serde::{Deserialize, Serialize};

/// The port the host's lobby is on.
const PORT: u16 = 47_953;

/// What the bot plays.
const BOT: u8 = 1;
/// What the first machine to join plays.
const FIRST: u8 = 5;
/// What the machine that joins again plays.
const AGAIN: u8 = 6;
/// Ends the run.
const QUIT: u8 = 3;

/// How many ticks each machine plays the seat before it is done.
const TURN: u64 = 45;

/// A count of the ticks each kind of player played the second seat.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Seats {
    now: u64,
    /// Ticks the bot played seat 1 after the first machine had.
    kept: u64,
    /// Ticks the first machine played it.
    first: u64,
    /// Ticks the machine that joined again played it.
    again: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Field;

impl corvid_behavior::Level for Field {
    type Error = core::convert::Infallible;
    type Edit = ();

    fn load(_name: &str) -> Result<Self, Self::Error> {
        Ok(Self)
    }
}

impl corvid_behavior::State for Seats {
    const NAME: &'static str = "rejoined";

    type Level = Field;
    type Rules = ();
    type Action = u8;

    fn tick(
        self,
        _level: &Field,
        players: &[PlayerState<u8>],
        (): &(),
        command: &mut impl Command,
    ) -> Self {
        if players.iter().any(|p| p.action == QUIT) {
            command.quit(ExitCode::SUCCESS);
        }
        let second = players.iter().find(|p| p.id.0 == 1).map(|p| p.action);
        Self {
            now: self.now + 1,
            kept: self.kept + u64::from(second == Some(BOT) && self.first > 0),
            first: self.first + u64::from(second == Some(FIRST)),
            again: self.again + u64::from(second == Some(AGAIN)),
        }
    }
}

impl corvid_replay::Opens for Seats {
    fn opening() -> Opening<Self> {
        Opening {
            level: "field".to_owned(),
            content: Arc::new(Field),
            rules: Arc::new(()),
            roster: (1..=2)
                .map(|account| Profile {
                    account: ProfileId(account),
                    joined: Tick::ZERO,
                    left: None,
                })
                .collect(),
            seed: Seed(9),
            first: Tick::ZERO,
            origin: None,
            schema: Schema::new("rejoined").digest(),
        }
    }
}

/// The bot: always [`BOT`].
#[derive(Debug, Default)]
struct Filler;

impl Controller<Seats> for Filler {
    type Config = ();
    type View = ();

    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    fn action(&self, _acting: Acting<'_, Seats>) -> u8 {
        BOT
    }

    fn update(&mut self, _updating: Updating<'_, Seats>) {}

    fn view(&self) -> &() {
        &()
    }

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}

/// Who a run is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Who {
    #[default]
    Host,
    /// Joins, plays [`TURN`] ticks and leaves.
    First,
    /// Joins after the first has left, and plays on.
    Again,
}

#[derive(Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a few things a test's player has done once each"
)]
struct Player {
    who: Who,
    asked: bool,
    started: bool,
    left: bool,
    /// Whether it has played linked yet.
    linked: bool,
    /// Frames spent alone since it was turned away, before asking again.
    waited: u32,
    next: u8,
}

impl Controller<Seats> for Player {
    type Config = u8;
    type View = ();

    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new(who: u8) -> Self {
        let mut player = Self::default();
        player.configure(who);
        player
    }

    fn configure(&mut self, who: u8) {
        self.who = match who {
            1 => Who::First,
            2 => Who::Again,
            _ => Who::Host,
        };
    }

    fn action(&self, _acting: Acting<'_, Seats>) -> u8 {
        self.next
    }

    fn update(&mut self, updating: Updating<'_, Seats>) {
        let net = updating.net;
        let state = updating.state;
        self.next = match self.who {
            Who::Host => 0,
            Who::First => FIRST,
            Who::Again => AGAIN,
        };
        if net.stage == Stage::Linked {
            self.linked = true;
        }
        match (self.who, net.stage) {
            // Turned away -- the seat still taken -- it asks again a moment
            // later.
            (Who::Again, Stage::Alone) if self.asked && !self.linked => {
                self.waited += 1;
                if self.waited > 30 {
                    self.waited = 0;
                    self.asked = false;
                }
            }
            (_, Stage::Alone) if !self.asked => {
                self.asked = true;
                updating.requests.push(if self.who == Who::Host {
                    NetRequest::Host {
                        port: PORT,
                        seats: 2,
                        name: "host".to_owned(),
                        local: true,
                    }
                } else {
                    NetRequest::Join {
                        address: format!("127.0.0.1:{PORT}"),
                        name: "guest".to_owned(),
                    }
                });
            }
            (Who::Host, Stage::Gathering) if net.can_start && !self.started => {
                self.started = true;
                updating.requests.push(NetRequest::Start);
            }
            // Played its turn: it goes, and once alone again it is done.
            (Who::First, Stage::Linked) if state.first >= TURN && !self.left => {
                self.left = true;
                updating.requests.push(NetRequest::Leave);
            }
            (Who::First, Stage::Alone) if self.left => self.next = QUIT,
            (Who::Host, Stage::Linked) if state.again >= TURN => self.next = QUIT,
            _ => {}
        }
    }

    fn view(&self) -> &() {
        &()
    }

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}

corvid_app::game! {
    struct Rejoined;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Seats;
    type Controller = Player;
    type Bot = Filler;
}

fn play(who: u8) -> corvid_app::Outcome<Rejoined> {
    App::<Rejoined>::new()
        .opening(<Seats as corvid_replay::Opens>::opening())
        .clock(Clock::wall())
        .settings(Settings {
            controls: who,
            ..Settings::default()
        })
        .for_ticks(Ticks(3_000))
        .retain(corvid_app::Retention::Everything)
        .run()
        .expect("the run")
}

#[test]
fn a_seat_whose_machine_left_is_kept_by_the_bot_and_joined_again() {
    let host = std::thread::spawn(|| play(0));
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    let first = std::thread::spawn(|| play(1));
    // The first plays three seconds of its turn and goes; well after that.
    std::thread::sleep(std::time::Duration::from_secs(7));
    let again = std::thread::spawn(|| play(2));
    let host = host.join().unwrap();
    let first = first.join().unwrap();
    let again = again.join().unwrap();
    drop(first);

    let state = &host.state;
    assert!(state.first >= TURN, "the first played {state:?}");
    assert!(state.again >= TURN, "the second played {state:?}");
    assert!(
        state.kept > 10,
        "the bot kept the seat between them for only {} ticks: {state:?}",
        state.kept
    );
    // Kept, never departed.
    assert!(
        host.session.opening.roster.iter().all(|p| p.left.is_none()),
        "a seat departed: {:?}",
        host.session.opening.roster
    );

    // From where the second came in, both machines agree.
    let last = host.session.last().min(again.session.last());
    let mut compared = 0;
    for at in again.session.first().0..last.0.saturating_sub(20) {
        let (Some(a), Some(b)) = (
            host.session.marks.get(Tick(at)),
            again.session.marks.get(Tick(at)),
        ) else {
            continue;
        };
        assert_eq!(a, b, "the machines disagree at tick {at}");
        compared += 1;
    }
    assert!(compared > 20, "only {compared} ticks compared");
}
