//! Two runs of different builds -- the same game, another schema -- meet in
//! a lobby: they gather, but when the host starts, the guest sees the terms
//! name a schema not its own, leaves the lobby saying why, and plays on
//! alone; the host sees it go. Neither plays a session the other would
//! desync.

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

/// Every stage a run's controller passed through, by the run's tag.
static SEEN: Mutex<Vec<(u16, Stage)>> = Mutex::new(Vec::new());
/// Every note a run's controller was shown, by tag.
static NOTES: Mutex<Vec<(u16, String)>> = Mutex::new(Vec::new());
/// How many members a host saw while linked, by tag.
static MEMBERS: Mutex<Vec<(u16, usize)>> = Mutex::new(Vec::new());

/// A count of ticks. A seat's action 3 ends the run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Count {
    now: u64,
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

impl corvid_behavior::State for Count {
    const NAME: &'static str = "builds";

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
        if players.iter().any(|p| p.action == 3) {
            command.quit(ExitCode::SUCCESS);
        }
        Self { now: self.now + 1 }
    }
}

/// The opening, built with a schema whose one field says which build.
fn opening(build: &str) -> Opening<Count> {
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
        seed: Seed(7),
        first: Tick::ZERO,
        origin: None,
        schema: Schema::new("builds").field("Build", build).digest(),
    }
}

impl corvid_replay::Opens for Count {
    fn opening() -> Opening<Self> {
        opening("one")
    }
}

/// Which run a controller is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Script {
    tag: u16,
    port: u16,
    host: bool,
}

#[derive(Debug, Default)]
struct Builder {
    script: Script,
    asked: bool,
    stage: Stage,
    /// Whether it has been in a lobby yet.
    gathered: bool,
    /// Frames in the current stage.
    frames: u32,
    next: u8,
}

impl Controller<Count> for Builder {
    type Config = Script;
    type View = ();

    const SETS: &'static [corvid_input::SetDescriptor] = &[];

    fn new(script: Script) -> Self {
        Self {
            script,
            ..Self::default()
        }
    }

    fn configure(&mut self, script: Script) {
        self.script = script;
    }

    fn action(&self, _acting: Acting<'_, Count>) -> u8 {
        self.next
    }

    fn update(&mut self, updating: Updating<'_, Count>) {
        let net = updating.net;
        let script = self.script.clone();
        if net.stage != self.stage {
            SEEN.lock().unwrap().push((script.tag, net.stage));
            self.stage = net.stage;
            self.frames = 0;
        }
        if let Some(note) = &net.note {
            NOTES.lock().unwrap().push((script.tag, note.clone()));
        }
        self.frames += 1;
        self.next = 1;
        match net.stage {
            Stage::Alone if !self.asked => {
                self.asked = true;
                let name = if script.host { "host" } else { "guest" }.to_owned();
                updating.requests.push(if script.host {
                    NetRequest::Host {
                        port: script.port,
                        seats: 2,
                        name,
                        local: true,
                    }
                } else {
                    NetRequest::Join {
                        address: format!("127.0.0.1:{}", script.port),
                        name,
                    }
                });
            }
            // Alone again after the lobby: a moment, then done.
            Stage::Alone if self.gathered && self.frames > 20 => self.next = 3,
            Stage::Alone => {}
            Stage::Gathering => {
                self.gathered = true;
                let unready = net.members.iter().any(|m| m.me && !m.ready);
                if script.host && net.can_start && net.members.len() == 2 {
                    updating.requests.push(NetRequest::Start);
                } else if !script.host && unready && net.members.len() == 2 {
                    updating.requests.push(NetRequest::Ready(true));
                }
            }
            Stage::Linked => {
                MEMBERS
                    .lock()
                    .unwrap()
                    .push((script.tag, net.members.len()));
                // The guest gone, the host is done.
                if script.host && net.members.len() == 1 {
                    self.next = 3;
                }
            }
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
    struct Builds;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Count;
    type Controller = Builder;
}

fn play(script: Script, build: &str) -> corvid_app::Outcome<Builds> {
    App::<Builds>::new()
        .opening(opening(build))
        .clock(Clock::wall())
        .settings(Settings {
            controls: script,
            ..Settings::default()
        })
        .for_ticks(Ticks(3_000))
        .run()
        .expect("the run")
}

fn stages(tag: u16) -> Vec<Stage> {
    SEEN.lock()
        .unwrap()
        .iter()
        .filter(|(t, _)| *t == tag)
        .map(|(_, stage)| *stage)
        .collect()
}

#[test]
fn two_runs_of_different_builds_do_not_link() {
    let script = |tag, host| Script {
        tag,
        port: 47_951,
        host,
    };
    let host = std::thread::spawn(move || play(script(1, true), "one"));
    let guest = std::thread::spawn(move || play(script(2, false), "two"));
    host.join().unwrap();
    guest.join().unwrap();
    // The guest gathered, turned the session down and played on alone,
    // never linked, and was told why.
    assert_eq!(stages(2), [Stage::Gathering, Stage::Alone], "the guest");
    let notes = NOTES.lock().unwrap();
    assert!(
        notes
            .iter()
            .any(|(tag, note)| *tag == 2 && note.contains("different build")),
        "the guest was not told why: {notes:?}"
    );
    // The host started, and saw the guest leave its lobby.
    assert_eq!(stages(1), [Stage::Gathering, Stage::Linked], "the host");
    let members = MEMBERS.lock().unwrap();
    assert!(
        members.iter().any(|(tag, n)| *tag == 1 && *n == 1),
        "the host never saw the guest go: {members:?}"
    );
}
