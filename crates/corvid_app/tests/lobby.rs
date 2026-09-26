//! Two runs meet in a lobby over a socket on this machine, driven by nothing
//! but their controllers' requests: they gather, play the lobby's session in
//! lockstep, come back to the lobby when a tick says so, play again, and a
//! run that leaves goes back to playing alone while the other sees it go.

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
/// How many members a host saw once its guest had left, by tag.
static AFTER: Mutex<Vec<(u16, usize)>> = Mutex::new(Vec::new());

/// A sum of the seats present. A seat's action is 1 for present, 2 to ask
/// everyone back to the lobby, 3 to end the run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Sum {
    total: i64,
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

impl corvid_behavior::State for Sum {
    const NAME: &'static str = "lobbied";

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
        if players.iter().any(|p| p.action == 2) {
            command.lobby();
        }
        if players.iter().any(|p| p.action == 3) {
            command.quit(ExitCode::SUCCESS);
        }
        let add: i64 = players
            .iter()
            .map(|p| i64::from(p.action.min(1)) * (i64::from(p.id.0) + 1))
            .sum();
        Self {
            total: self.total + add,
            now: self.now + 1,
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
            seed: Seed(7),
            first: Tick::ZERO,
            origin: None,
            schema: Schema::new("lobbied").digest(),
        }
    }
}

/// Which run a controller is, and what it does.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Script {
    /// A tag for what it records.
    tag: u16,
    port: u16,
    host: bool,
    /// The guest leaves 60 frames into the first match, rather than both
    /// going back to the lobby and playing a second.
    leave: bool,
}

#[derive(Debug, Default)]
struct Lobbyist {
    script: Script,
    asked: bool,
    stage: Stage,
    matches: u32,
    /// Frames in the current stage, for waiting a moment alone.
    frames: u32,
    /// The action for the next tick.
    next: u8,
    /// Whether it asked to leave.
    left: bool,
}

impl Lobbyist {
    fn linked(&mut self, members: usize, now: u64) {
        let script = &self.script;
        if script.leave {
            if script.host && members == 1 {
                AFTER.lock().unwrap().push((script.tag, members));
                self.next = 3;
            }
        } else if script.host && self.matches == 1 && now >= 80 {
            self.next = 2;
        } else if script.host && self.matches == 2 && now >= 120 {
            self.next = 3;
        }
    }
}

impl Controller<Sum> for Lobbyist {
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

    fn action(&self, _acting: Acting<'_, Sum>) -> u8 {
        self.next
    }

    fn update(&mut self, updating: Updating<'_, Sum>) {
        let net = updating.net;
        if net.stage != self.stage {
            SEEN.lock().unwrap().push((self.script.tag, net.stage));
            if net.stage == Stage::Linked {
                self.matches += 1;
            }
            self.stage = net.stage;
            self.frames = 0;
        }
        self.frames += 1;
        self.next = 1;
        let script = self.script.clone();
        match net.stage {
            Stage::Alone if !self.asked => {
                self.asked = true;
                let name = if script.host { "host" } else { "guest" }.to_owned();
                updating.requests.push(if script.host {
                    NetRequest::Host {
                        port: script.port,
                        seats: 2,
                        name,
                    }
                } else {
                    NetRequest::Join {
                        address: format!("127.0.0.1:{}", script.port),
                        name,
                    }
                });
            }
            // Alone again, after leaving: done.
            Stage::Alone if self.frames > 20 => self.next = 3,
            Stage::Alone => {}
            Stage::Gathering => {
                let unready = net.members.iter().any(|m| m.me && !m.ready);
                if script.host && net.can_start && net.members.len() == 2 && self.matches < 2 {
                    updating.requests.push(NetRequest::Start);
                } else if !script.host && unready && net.members.len() == 2 {
                    updating.requests.push(NetRequest::Ready(true));
                }
            }
            Stage::Linked => {
                if script.leave && !script.host && updating.state.now == 60 && !self.left {
                    self.left = true;
                    updating.requests.push(NetRequest::Leave);
                }
                self.linked(net.members.len(), updating.state.now);
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
    struct Lobbied;
    const PERIOD: TickSpan = TickSpan::CRADLE;
    type State = Sum;
    type Controller = Lobbyist;
}

fn play(script: Script) -> corvid_app::Outcome<Lobbied> {
    App::<Lobbied>::new()
        .opening(<Sum as corvid_replay::Opens>::opening())
        .clock(Clock::wall())
        .settings(Settings {
            controls: script,
            ..Settings::default()
        })
        .for_ticks(Ticks(3_000))
        .retain(corvid_app::Retention::Everything)
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
fn two_runs_play_go_back_to_the_lobby_and_play_again() {
    let script = |tag, host| Script {
        tag,
        port: 47_941,
        host,
        leave: false,
    };
    let host = std::thread::spawn(move || play(script(1, true)));
    let guest = std::thread::spawn(move || play(script(2, false)));
    let host = host.join().unwrap();
    let guest = guest.join().unwrap();
    let round = [
        Stage::Gathering,
        Stage::Linked,
        Stage::Gathering,
        Stage::Linked,
    ];
    assert_eq!(stages(1), round, "the host");
    assert_eq!(stages(2), round, "the guest");
    // The second match: both seats played it, and both machines agree.
    assert!(host.state.total > i64::try_from(host.state.now).unwrap() * 2);
    let last = host.session.last().min(guest.session.last());
    let mut compared = 0;
    for at in 0..last.0.saturating_sub(20) {
        let (Some(a), Some(b)) = (
            host.session.marks.get(Tick(at)),
            guest.session.marks.get(Tick(at)),
        ) else {
            continue;
        };
        assert_eq!(a, b, "the second match disagrees at tick {at}");
        compared += 1;
    }
    assert!(compared > 60, "only {compared} ticks compared");
}

#[test]
fn a_run_that_leaves_plays_alone_and_the_other_sees_it_go() {
    let script = |tag, host| Script {
        tag,
        port: 47_942,
        host,
        leave: true,
    };
    let host = std::thread::spawn(move || play(script(3, true)));
    let guest = std::thread::spawn(move || play(script(4, false)));
    host.join().unwrap();
    guest.join().unwrap();
    assert_eq!(
        stages(4),
        [Stage::Gathering, Stage::Linked, Stage::Alone],
        "the guest"
    );
    assert!(
        AFTER
            .lock()
            .unwrap()
            .iter()
            .any(|(tag, n)| *tag == 3 && *n == 1),
        "the host never saw the guest go"
    );
}
