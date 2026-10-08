//! A session played by two machines saves on a tick it asks to, and both
//! machines write the same state for it: the tick's own, final on both, under
//! a session that opens on it. Either slot loads; and a machine playing alone
//! plays on from one when its controller asks.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use corvid_app::{App, Settings};
use corvid_behavior::{Command, ExitCode, PlayerState, ProfileId, SaveSlot};
use corvid_control::net::{NetRequest, Stage};
use corvid_control::{Acting, Controller, Updating};
use corvid_replay::{Opening, Profile, Schema, Seed};
use corvid_time::{Clock, Tick, TickSpan, Ticks};
use serde::{Deserialize, Serialize};

/// The port the host's lobby is on.
const PORT: u16 = 47_955;

/// The slot the session saves into.
const SLOT: u16 = 4;

/// The tick that asks for the save.
const SAVE_AT: u64 = 60;

/// Ends a run.
const QUIT: u8 = 3;

/// A count of the ticks, and of what each seat sent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Sum {
    now: u64,
    /// What every seat has sent, added up, so the state depends on both.
    sent: u64,
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

impl corvid_behavior::State for Sum {
    const NAME: &'static str = "saved_together";

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
        if self.now == SAVE_AT {
            command.save(SaveSlot(SLOT));
        }
        let sent: u64 = players.iter().map(|p| u64::from(p.action)).sum();
        Self {
            now: self.now + 1,
            sent: self.sent + sent,
        }
    }
}

impl corvid_replay::Opens for Sum {
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
            schema: Schema::new("saved_together").digest(),
        }
    }
}

/// Who a run is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Who {
    #[default]
    Host,
    Guest,
    /// Alone, resuming the slot.
    Resumer,
}

#[derive(Debug, Default)]
struct Player {
    who: Who,
    asked: bool,
    started: bool,
    next: u8,
}

impl Controller<Sum> for Player {
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
            1 => Who::Guest,
            2 => Who::Resumer,
            _ => Who::Host,
        };
    }

    fn action(&self, _acting: Acting<'_, Sum>) -> u8 {
        self.next
    }

    fn update(&mut self, updating: Updating<'_, Sum>) {
        let net = updating.net;
        let now = updating.state.now;
        self.next = match self.who {
            Who::Host => 1,
            Who::Guest => 2,
            Who::Resumer => 0,
        };
        match (self.who, net.stage) {
            (Who::Resumer, _) if !self.asked => {
                self.asked = true;
                updating.requests.push(NetRequest::Resume { slot: SLOT });
            }
            (Who::Resumer, _) if net.resumed == Some(SLOT) && now >= SAVE_AT + 10 => {
                self.next = QUIT;
            }
            (Who::Host | Who::Guest, Stage::Alone) if !self.asked => {
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
            (Who::Host, Stage::Gathering)
                if net.can_start && net.members.len() == 2 && !self.started =>
            {
                self.started = true;
                updating.requests.push(NetRequest::Start);
            }
            // Once the host has seated it: said before, it would go nowhere.
            (Who::Guest, Stage::Gathering) if !self.started && net.members.iter().any(|m| m.me) => {
                self.started = true;
                updating.requests.push(NetRequest::Ready(true));
            }
            (Who::Host, Stage::Linked) if now >= SAVE_AT + 30 => self.next = QUIT,
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

/// The bot: idle.
#[derive(Debug, Default)]
struct Idle;

impl Controller<Sum> for Idle {
    type Config = ();
    type View = ();

    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new((): ()) -> Self {
        Self
    }

    fn configure(&mut self, (): ()) {}

    fn action(&self, _acting: Acting<'_, Sum>) -> u8 {
        0
    }

    fn update(&mut self, _updating: Updating<'_, Sum>) {}

    fn view(&self) -> &() {
        &()
    }

    fn look(&self) -> corvid_camera::Camera {
        corvid_camera::Camera::default()
    }
}

corvid_app::game! {
    struct Together;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Sum;
    type Controller = Player;
    type Bot = Idle;
}

/// A directory of the test's own, emptied.
fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "corvid-saved-together-{}-{name}",
        std::process::id()
    ));
    drop(std::fs::remove_dir_all(&path));
    path
}

fn play(who: u8, state: &Path) -> corvid_app::Outcome<Together> {
    App::<Together>::new()
        .opening(<Sum as corvid_replay::Opens>::opening())
        .clock(Clock::wall())
        .state(state)
        .settings(Settings {
            controls: who,
            ..Settings::default()
        })
        .for_ticks(Ticks(3_000))
        .run()
        .expect("the run")
}

/// The state a slot under `state` opens on.
fn opened(state: &Path) -> Sum {
    let outcome = App::<Together>::new()
        .headless()
        .opening(<Sum as corvid_replay::Opens>::opening())
        .state(state)
        .load(SaveSlot(SLOT))
        .for_ticks(Ticks(0))
        .run()
        .expect("the slot opens");
    Sum::clone(&outcome.state)
}

#[test]
fn two_machines_save_one_tick_and_a_machine_alone_plays_on_from_it() {
    let (host_dir, guest_dir) = (scratch("host"), scratch("guest"));
    let host = {
        let dir = host_dir.clone();
        std::thread::spawn(move || play(0, &dir))
    };
    std::thread::sleep(std::time::Duration::from_millis(300));
    let guest = {
        let dir = guest_dir.clone();
        std::thread::spawn(move || play(1, &dir))
    };
    let host = host.join().unwrap();
    let guest = guest.join().unwrap();
    assert!(
        host.state.now > SAVE_AT,
        "the session never reached the save"
    );
    assert!(guest.state.now > SAVE_AT);

    // Both saved the state the asking tick made, and it is the same.
    let (saved_host, saved_guest) = (opened(&host_dir), opened(&guest_dir));
    assert_eq!(saved_host.now, SAVE_AT + 1, "{saved_host:?}");
    assert_eq!(saved_host, saved_guest);
    assert!(saved_host.sent > 0, "neither seat's actions are in it");

    // Alone, a machine asked to resume the slot plays on from it.
    let resumed = play(2, &host_dir);
    assert!(
        resumed.state.now >= SAVE_AT + 10,
        "the run did not play on from the save: {:?}",
        resumed.state
    );
    assert!(resumed.state.sent >= saved_host.sent);
    drop(std::fs::remove_dir_all(&host_dir));
    drop(std::fs::remove_dir_all(&guest_dir));
}
