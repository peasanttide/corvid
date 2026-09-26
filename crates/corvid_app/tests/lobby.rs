//! Machines gather in a lobby over real sockets on this machine: a host seats
//! whoever says hello, turns away the wrong game and the one too many, and
//! starts once every seat is taken and every guest is ready.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "a failed unwrap or assertion in a test is a failed test, which is what a test is for"
)]

use std::time::{Duration, Instant};

use corvid_app::lobby::{Lobby, Stage};
use corvid_behavior::PlayerId;

/// Polls every lobby until `done` says so, or fails after a few seconds.
fn until(lobbies: &mut [&mut Lobby], what: &str, done: impl Fn(&[&mut Lobby]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(lobbies) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        for lobby in lobbies.iter_mut() {
            lobby.poll();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn address(host: &Lobby) -> String {
    let port = host.local().expect("a host has a socket").port();
    format!("127.0.0.1:{port}")
}

#[test]
fn a_guest_is_seated_readies_and_starts_with_the_host() {
    let mut host = Lobby::host(0, "hosty", "pong", 2).expect("the host binds");
    let mut guest = Lobby::join(&address(&host), "guesty", "pong").expect("the guest binds");
    assert!(host.hosting() && !guest.hosting());

    until(
        &mut [&mut host, &mut guest],
        "the guest to be seated",
        |l| l[1].members().len() == 2,
    );
    let seated = &guest.members()[1];
    assert_eq!(seated.name, "guesty");
    assert_eq!(seated.seat, PlayerId(1));
    assert_eq!(guest.seats(), 2);
    assert!(!host.can_start(), "started before the guest was ready");
    assert!(!host.start());

    guest.set_ready(true);
    until(&mut [&mut host, &mut guest], "the guest to be ready", |l| {
        l[0].can_start() && l[1].ready()
    });
    assert!(host.start());
    assert_eq!(host.stage(), &Stage::Started);
    // The host's loop sends the opening, and there is no loop here: the
    // guest starts in `linked` below, where there is.
    guest.poll();
    assert_eq!(guest.stage(), &Stage::Gathering);
}

#[test]
fn the_wrong_game_and_one_too_many_are_turned_away() {
    let mut host = Lobby::host(0, "hosty", "pong", 2).expect("the host binds");
    let at = address(&host);
    let mut stranger = Lobby::join(&at, "stranger", "chess").expect("binds");
    until(
        &mut [&mut host, &mut stranger],
        "the stranger to be refused",
        |l| matches!(l[1].stage(), Stage::Refused(_)),
    );
    assert_eq!(host.members().len(), 1);

    let mut first = Lobby::join(&at, "first", "pong").expect("binds");
    until(
        &mut [&mut host, &mut first],
        "the first guest to be seated",
        |l| l[0].members().len() == 2,
    );
    let mut second = Lobby::join(&at, "second", "pong").expect("binds");
    until(
        &mut [&mut host, &mut first, &mut second],
        "the second guest to be refused",
        |l| matches!(l[2].stage(), Stage::Refused(_)),
    );
    assert_eq!(host.members().len(), 2);
}

/// The whole way through two real runs: each starts alone, meets the other
/// in a lobby over a socket on this machine, and from the start plays one
/// session with it in lockstep.
mod linked {
    use std::sync::Arc;

    use corvid_app::lobby::{Lobby, Stage};
    use corvid_app::{App, Settings};
    use corvid_behavior::{Command, PlayerState, ProfileId};
    use corvid_control::{Acting, Controller, Updating};
    use corvid_replay::{Opening, Profile, Schema, Seed};
    use corvid_time::{Clock, Tick, TickSpan, Ticks};
    use serde::{Deserialize, Serialize};

    /// Where the host of this test listens.
    const PORT: u16 = 47_917;

    /// How many ticks each run plays.
    const TICKS: u64 = 240;

    /// A sum of every seat's action, weighted by seat, and a tick count.
    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub(super) struct Sum {
        pub(super) total: i64,
        pub(super) now: u64,
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub(super) struct Field;

    impl corvid_behavior::Level for Field {
        type Error = core::convert::Infallible;
        type Edit = ();

        fn load(_name: &str) -> Result<Self, Self::Error> {
            Ok(Self)
        }
    }

    impl corvid_behavior::State for Sum {
        const NAME: &'static str = "sum";

        type Level = Field;
        type Rules = ();
        type Action = i8;

        fn tick(
            self,
            _level: &Field,
            players: &[PlayerState<i8>],
            (): &(),
            _command: &mut impl Command,
        ) -> Self {
            let add: i64 = players
                .iter()
                .map(|p| i64::from(p.action) * (i64::from(p.id.0) + 1))
                .sum();
            Self {
                total: self.total + add,
                now: self.now + 1,
            }
        }
    }

    /// Hosts, or joins, a lobby, and readies or starts as soon as it can.
    #[derive(Debug, Default)]
    pub(super) struct Lobbyist {
        host: bool,
        lobby: Option<Lobby>,
        done: bool,
    }

    impl Controller<Sum> for Lobbyist {
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

        fn action(&self, _acting: Acting<'_, Sum>) -> i8 {
            1
        }

        fn update(&mut self, _updating: Updating<'_, Sum>) {
            if self.done {
                return;
            }
            if self.lobby.is_none() {
                let at = format!("127.0.0.1:{PORT}");
                self.lobby = if self.host {
                    Lobby::host(PORT, "host", "sum", 2).ok()
                } else {
                    Lobby::join(&at, "guest", "sum").ok()
                };
            }
            let Some(lobby) = self.lobby.as_mut() else {
                return;
            };
            lobby.poll();
            if lobby.hosting() {
                if lobby.can_start() {
                    lobby.start();
                }
            } else if lobby.members().len() == 2 && !lobby.ready() {
                lobby.set_ready(true);
            }
            if lobby.stage() == &Stage::Started {
                self.lobby = None;
                self.done = true;
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
        pub(super) struct Summing;
        const PERIOD: TickSpan = TickSpan::CRADLE;
        type State = Sum;
        type Controller = Lobbyist;
    }

    impl corvid_replay::Opens for Sum {
        fn opening() -> Opening<Self> {
            opening()
        }
    }

    fn opening() -> Opening<Sum> {
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
            schema: Schema::new("sum").digest(),
        }
    }

    fn play(host: bool) -> corvid_app::Result<corvid_app::Outcome<Summing>> {
        App::<Summing>::new()
            .opening(opening())
            // The wall clock: a lobby meets over a real socket, in real time.
            .clock(Clock::wall())
            .settings(Settings {
                controls: host,
                ..Settings::default()
            })
            .for_ticks(Ticks(TICKS))
            .retain(corvid_app::Retention::Everything)
            .run()
    }

    #[test]
    fn two_runs_meet_in_a_lobby_and_play_one_session() {
        let host = std::thread::spawn(|| play(true));
        let guest = std::thread::spawn(|| play(false));
        let host = host
            .join()
            .expect("the host's thread")
            .expect("the host's run");
        let guest = guest
            .join()
            .expect("the guest's thread")
            .expect("the guest's run");
        // Both seats played in both sessions: three a tick once linked, where
        // a run alone adds one.
        let (h, g) = (&host.state, &guest.state);
        assert!(
            h.total > i64::try_from(h.now).unwrap_or(0) * 2,
            "the host played alone: {h:?}"
        );
        assert!(
            g.total > i64::try_from(g.now).unwrap_or(0) * 2,
            "the guest played alone: {g:?}"
        );
        let overlap = h.now.min(g.now).saturating_sub(20);
        assert!(overlap >= TICKS / 2, "barely overlapped: {h:?} {g:?}");
        let mut compared = 0;
        for at in 0..=overlap {
            let (Some(mine), Some(theirs)) = (
                host.session.marks.get(Tick(at)),
                guest.session.marks.get(Tick(at)),
            ) else {
                continue;
            };
            assert_eq!(mine, theirs, "the two disagree at tick {at}");
            compared += 1;
        }
        assert!(compared > 0, "no tick was compared");
    }
}
