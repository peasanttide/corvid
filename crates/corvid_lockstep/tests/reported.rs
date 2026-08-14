//! What a desync report reads like, and the game it is read against.
//!
//! The seam against `desync.rs` is the finding: that file is two peers ceasing
//! to agree, and this is the shape of the report they are handed.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, which is what a test is for"
)]
#![allow(
    clippy::similar_names,
    reason = "two peers of one session are `here` and `there`, and the datagrams between them are named after them"
)]

mod common;

use core::convert::Infallible;
use std::sync::Arc;

#[cfg(feature = "dev")]
use common::{Action, Swarm, peer, push};
use corvid_behavior::{Command, PlayerState};
use corvid_behavior::{PlayerId, State};
use corvid_hash::Digest;
#[cfg(feature = "dev")]
use corvid_lockstep::Budget;
use corvid_lockstep::{Desync, FieldReport, Where};
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// Runs two peers over a link that loses nothing and delays nothing.
#[cfg(feature = "dev")]
fn perfect(ticks: u64) -> (corvid_lockstep::Peer<Swarm>, corvid_lockstep::Peer<Swarm>) {
    let mut here = peer(16, 2, 0, Budget::DEFAULT);
    let mut there = peer(16, 2, 1, Budget::DEFAULT);

    for at in 0..ticks {
        here.submit(if at.is_multiple_of(5) {
            push(3)
        } else {
            Action::Idle
        })
        .unwrap();
        there
            .submit(if at.is_multiple_of(3) {
                push(-1)
            } else {
                Action::Idle
            })
            .unwrap();

        let (from_here, from_there) = (here.outgoing(), there.outgoing());
        let _ = here.receive(&from_there).unwrap();
        let _ = there.receive(&from_here).unwrap();

        let _ = here.advance(&mut corvid_behavior::Discard::new()).unwrap();
        let _ = there.advance(&mut corvid_behavior::Discard::new()).unwrap();
    }
    (here, there)
}

#[test]
fn the_report_renders_the_layout_it_is_read_in() {
    let desync = Desync {
        at: Tick(4127),
        peer: PlayerId(2),
        agreed_through: Tick(4126),
        local: Digest::from_u64(0x8f21_0000_0000_0000),
        remote: Digest::from_u64(0x8f20_0000_0000_0000),
        fields: vec![
            FieldReport {
                probe: "state.creeps.velocity",
                agrees: false,
                local: Digest::from_u64(0x8f21_0000_0000_0000),
                remote: Digest::from_u64(0x8f20_0000_0000_0000),
            },
            FieldReport {
                probe: "state.creeps.position",
                agrees: true,
                local: Digest::from_u64(0x1111_0000_0000_0000),
                remote: Digest::from_u64(0x1111_0000_0000_0000),
            },
            FieldReport {
                probe: "state.towers",
                agrees: true,
                local: Digest::from_u64(0x2222_0000_0000_0000),
                remote: Digest::from_u64(0x2222_0000_0000_0000),
            },
        ],
        first_divergent: Some(Where {
            probe: "creep",
            index: 30_281,
            region: 44,
        }),
    };

    // Frozen, because a report nobody can read is a report nobody uses.
    assert_eq!(
        desync.to_string(),
        "desync at tick 4127, peer 2
  agreed through 4126
  state.creeps.velocity  differs   local 0x8f21\u{2026} remote 0x8f20\u{2026}
  state.creeps.position  agrees
  state.towers           agrees
  first divergent index  creep 30281, region 44"
    );
}

#[cfg(feature = "dev")]
mod dev {
    use super::{Swarm, perfect};
    use corvid_lockstep::{Bisect, Probes, TickProbes, bisect};
    use corvid_time::Tick;
    /// The state `at`, with one row of one column moved.
    fn parted(
        peer: &corvid_lockstep::Peer<Swarm>,
        at: Tick,
        row: usize,
    ) -> corvid_lockstep::TickProbes {
        let (_, mut state) = peer.restore(at).unwrap();
        state.velocity[row] = state.velocity[row].wrapping_add(1);
        TickProbes::of::<Swarm>(at, &state)
    }

    #[test]
    fn a_game_with_three_columns_has_all_three_in_declaration_order() {
        let (here, _) = perfect(12);
        let mut probes = Probes::default();
        Swarm::probe(here.state(), &mut probes);

        assert_eq!(
            probes
                .reports()
                .iter()
                .map(|report| report.probe)
                .collect::<Vec<_>>(),
            [
                "state.creeps.position",
                "state.creeps.velocity",
                "state.towers"
            ],
        );
    }

    #[test]
    fn one_divergent_column_differs_and_the_other_two_agree() {
        let (here, _) = perfect(12);
        let agreed = Tick(5);
        let parted_at = Tick(6);
        let (_, was) = here.restore(agreed).unwrap();

        let remote = [
            TickProbes::of::<Swarm>(agreed, &was),
            parted(&here, parted_at, 3),
        ];
        let desync = bisect(&here, &remote).unwrap();

        assert_eq!(desync.at, parted_at);
        let verdicts: Vec<_> = desync
            .fields
            .iter()
            .map(|report| (report.probe, report.agrees))
            .collect();
        assert_eq!(
            verdicts,
            [
                ("state.creeps.position", true),
                ("state.creeps.velocity", false),
                ("state.towers", true),
            ],
        );

        // The length of the disagreement rather than the length of the session:
        // one tick simulated, from the last tick they agreed on to the first
        // they did not.
    }

    #[test]
    fn a_column_makes_locate_name_the_first_differing_row() {
        let (here, _) = perfect(12);
        let agreed = Tick(5);
        let parted_at = Tick(6);
        let (_, was) = here.restore(agreed).unwrap();
        let (_, mut wrong) = here.restore(parted_at).unwrap();

        // Two rows moved, and the report is about the first of them.
        for row in [9, 4] {
            wrong.velocity[row] = wrong.velocity[row].wrapping_add(1);
        }
        let remote = [
            TickProbes::of::<Swarm>(agreed, &was),
            TickProbes::of::<Swarm>(parted_at, &wrong),
        ];

        let desync = bisect(&here, &remote).unwrap();

        let found = desync
            .first_divergent
            .expect("the column was sent row-wise");
        assert_eq!(found.probe, "creep");
        assert_eq!(found.index, 4);
        assert_eq!(found.region, 0);
    }

    #[test]
    fn two_peers_that_agree_bisect_to_no_differing_field() {
        let (here, _) = perfect(12);
        let remote: Vec<_> = (4..=7)
            .map(|at| {
                let (_, state) = here.restore(Tick(at)).unwrap();
                TickProbes::of::<Swarm>(Tick(at), &state)
            })
            .collect();

        let desync = bisect(&here, &remote).unwrap();

        assert!(desync.fields.iter().all(|report| report.agrees));
        assert!(desync.first_divergent.is_none());
    }

    #[test]
    fn the_default_impl_reports_one_field_named_state() {
        let mut probes = Probes::default();
        super::Counted::probe(&super::Counted { count: 7 }, &mut probes);

        let reports = probes.reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].probe, "state");
        assert_eq!(
            reports[0].local,
            corvid_hash::digest(&super::Counted { count: 7 })
        );

        // Nothing was sent row-wise, so there is nothing to locate.
        assert_eq!(
            super::Counted::locate(&super::Counted { count: 7 }, "state", &[]),
            None
        );
    }

    #[test]
    fn probes_agree_until_they_are_compared_against_something_else() {
        let (here, _) = perfect(12);
        let mut probes = Probes::default();
        Swarm::probe(here.state(), &mut probes);
        assert!(probes.agrees(), "nothing has been compared against yet");

        probes.compare(&[corvid_hash::Digest::ZERO]);
        assert!(!probes.agrees());
    }
}

/// A game that implements nothing beyond the contract, so that the default
/// [`Bisect`](corvid_lockstep::Bisect) implementation has something to be
/// implemented for.
///
/// The state *is* the game now, so there is no marker beside this.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Counted {
    /// The only thing it counts.
    count: i64,
}

/// A level with nothing in it, for the game that has nothing in it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Nowhere;

impl corvid_behavior::Level for Nowhere {
    type Error = Infallible;
    fn load(_name: &str) -> Result<Self, Infallible> {
        Ok(Self)
    }
}

impl State for Counted {
    const NAME: &'static str = "plain";

    type Level = Nowhere;
    type Rules = ();
    type Action = ();

    fn tick(
        self,
        _level: &Nowhere,
        players: &[PlayerState<()>],
        _rules: &(),
        _command: &mut impl Command,
    ) -> Self {
        Self {
            count: self
                .count
                .wrapping_add(i64::try_from(players.len()).unwrap_or(0)),
        }
    }
}

impl corvid_lockstep::Bisect for Counted {}

#[test]
fn a_session_of_the_plain_game_still_opens() {
    // The other half of the default `Bisect` impl compiling: the game it is
    // implemented for is a game.
    let opening = corvid_replay::Opening::<Counted> {
        level: "nowhere".to_owned(),
        content: Arc::new(Nowhere),
        rules: Arc::new(()),
        roster: Vec::new(),
        seed: corvid_replay::Seed(0),
        first: Tick::ZERO,
        origin: Some(Arc::new(Counted::default())),
        schema: corvid_replay::Schema::new("plain").digest(),
    };
    assert!(corvid_replay::Session::new(opening).is_ok());
}
