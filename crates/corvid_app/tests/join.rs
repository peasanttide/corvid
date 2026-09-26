//! A host starts its lobby's session alone, its bot playing the empty seat,
//! and a second run joins the session while it is being played: it takes
//! the bot's seat at a tick the host names, and from there the two machines
//! play one session.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use std::sync::{Arc, Mutex};

use corvid_app::{App, Settings};
use corvid_behavior::{Command, ExitCode, PlayerState, ProfileId};
use corvid_control::net::{NetRequest, Stage};
use corvid_control::{Acting, Controller, Updating};
use corvid_replay::{Opening, Profile, Schema, Seed};
use corvid_time::{Clock, Tick, TickSpan, Ticks};
use serde::{Deserialize, Serialize};

/// Every stage the joiner's controller passed through.
static SEEN: Mutex<Vec<Stage>> = Mutex::new(Vec::new());

/// The port the host's lobby is on.
const PORT: u16 = 47_951;

/// What the bot plays.
const BOT: u8 = 1;
/// What the machine that joins plays.
const JOINER: u8 = 5;
/// Ends the run.
const QUIT: u8 = 3;

/// A count of the ticks each kind of player played the second seat.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Seats {
    now: u64,
    /// Ticks the bot played seat 1.
    bot: u64,
    /// Ticks the joiner played it.
    joined: u64,
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
    const NAME: &'static str = "joined";

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
            bot: self.bot + u64::from(second == Some(BOT)),
            joined: self.joined + u64::from(second == Some(JOINER)),
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
            schema: Schema::new("joined").digest(),
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

#[derive(Debug, Default)]
struct Player {
    host: bool,
    asked: bool,
    started: bool,
    stage: Stage,
    next: u8,
}

impl Controller<Seats> for Player {
    type Config = bool;
    type View = ();

    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new(host: bool) -> Self {
        Self {
            host,
            ..Self::default()
        }
    }

    fn configure(&mut self, host: bool) {
        self.host = host;
    }

    fn action(&self, _acting: Acting<'_, Seats>) -> u8 {
        self.next
    }

    fn update(&mut self, updating: Updating<'_, Seats>) {
        let net = updating.net;
        if net.stage != self.stage {
            if !self.host {
                SEEN.lock().unwrap().push(net.stage);
            }
            self.stage = net.stage;
        }
        self.next = if self.host { 0 } else { JOINER };
        match net.stage {
            Stage::Alone if !self.asked => {
                self.asked = true;
                updating.requests.push(if self.host {
                    NetRequest::Host {
                        port: PORT,
                        seats: 2,
                        name: "host".to_owned(),
                    }
                } else {
                    NetRequest::Join {
                        address: format!("127.0.0.1:{PORT}"),
                        name: "late".to_owned(),
                    }
                });
            }
            Stage::Gathering if self.host && net.can_start && !self.started => {
                self.started = true;
                updating.requests.push(NetRequest::Start);
            }
            // Enough of the joiner's play to be sure of it: done.
            Stage::Linked if self.host && updating.state.joined >= 120 => self.next = QUIT,
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
    struct Joined;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Seats;
    type Controller = Player;
    type Bot = Filler;
}

fn play(host: bool) -> corvid_app::Outcome<Joined> {
    App::<Joined>::new()
        .opening(<Seats as corvid_replay::Opens>::opening())
        .clock(Clock::wall())
        .settings(Settings {
            controls: host,
            ..Settings::default()
        })
        .for_ticks(Ticks(3_000))
        .retain(corvid_app::Retention::Everything)
        .run()
        .expect("the run")
}

#[test]
fn a_run_joins_a_session_in_progress_in_the_seat_the_bot_was_playing() {
    let host = std::thread::spawn(|| play(true));
    // Late enough that the host has started and played a while alone.
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    let guest = std::thread::spawn(|| play(false));
    let host = host.join().unwrap();
    let guest = guest.join().unwrap();

    assert_eq!(*SEEN.lock().unwrap(), [Stage::Gathering, Stage::Linked]);
    assert!(
        host.state.bot > 10,
        "the bot played {} ticks: {:?} {:?}",
        host.state.bot,
        host.state,
        guest.state
    );
    assert!(
        host.state.joined >= 120,
        "the joiner played {} ticks",
        host.state.joined
    );
    // One seat, one player at a time: every tick but the first few, before
    // anybody's first action is due, was the bot's or the joiner's.
    let played = host.state.bot + host.state.joined;
    assert!(
        played + 8 >= host.state.now,
        "{played} of {} ticks played",
        host.state.now
    );

    // From where the joiner came in, both machines agree.
    let last = host.session.last().min(guest.session.last());
    let mut compared = 0;
    for at in guest.session.first().0..last.0.saturating_sub(20) {
        let (Some(a), Some(b)) = (
            host.session.marks.get(Tick(at)),
            guest.session.marks.get(Tick(at)),
        ) else {
            continue;
        };
        assert_eq!(a, b, "the machines disagree at tick {at}");
        compared += 1;
    }
    assert!(compared > 100, "only {compared} ticks compared");
}
