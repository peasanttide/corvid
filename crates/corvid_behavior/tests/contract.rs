#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is a failed test, and the message a panic carries is more use than a Result nobody reads"
)]

//! The smallest game there is, as the thing the contract is asserted against.
//!
//! Four types and one function, which is what a game owes now: a `Level` that
//! reads itself out of a `Source`, a `State` that is the state rather than a
//! marker pointing at one, and a `Command` sink that a test can be a `Vec`.

use corvid_behavior::{Command, ExitCode, Level, Player, PlayerId, Presence, State, Time};
use corvid_files::{Malformed, Memory, Source};
use serde::{Deserialize, Serialize};

/// A level: one number, read out of one file's first byte.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Field {
    width: u16,
}

impl Level for Field {
    type Reference = String;

    fn load(reference: &String, files: &dyn Source) -> Result<Self, Malformed> {
        let bytes = files.read(reference)?;
        let width = u16::from(
            *bytes
                .first()
                .ok_or_else(|| Malformed::at(reference, "a field needs at least one byte"))?,
        );
        Ok(Self { width })
    }
}

/// A state: how far along the field something has walked.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Walk {
    at: u16,
    /// Which level was folded in, so `load_level` can be seen to have run.
    field: u16,
}

/// An action: whether this player stepped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Step(bool);

impl State for Walk {
    const NAME: &'static str = "walk";
    type Rules = ();
    type Level = Field;
    type Action = Step;

    fn load_level(self, old: Option<&Field>, new: &Field) -> Self {
        Self {
            // A walk keeps its distance across a level change unless the new
            // field is shorter, which is the sort of thing only the game knows.
            at: self.at.min(new.width),
            field: new.width + old.map_or(0, |old| old.width),
        }
    }

    fn tick(
        self,
        level: &Field,
        players: &[Player<'_, Step>],
        _rules: &(),
        command: &mut impl Command<Reference = String>,
    ) -> Self {
        let stepped = u16::try_from(players.iter().filter(|player| player.action.0).count())
            .unwrap_or(u16::MAX);
        let at = self.at.saturating_add(stepped).min(level.width);
        if at == level.width {
            command.quit(ExitCode::SUCCESS);
        }
        Self { at, ..self }
    }
}

/// A sink that keeps what it was told, which is the whole point of the trait.
#[derive(Debug, Default, PartialEq, Eq)]
struct Recorder {
    quits: Vec<ExitCode>,
    loads: Vec<String>,
}

impl Command for Recorder {
    type Reference = String;

    fn quit(&mut self, code: ExitCode) {
        self.quits.push(code);
    }

    fn load(&mut self, reference: String) {
        self.loads.push(reference);
    }
}

/// One player who always steps.
const fn walker(action: &Step) -> [Player<'_, Step>; 1] {
    [Player {
        id: PlayerId(0),
        presence: Presence::Active,
        action,
    }]
}

#[test]
fn a_level_loads_from_a_source() {
    let mut files = Memory::new();
    files.insert("field", vec![7]);
    assert_eq!(
        Field::load(&"field".to_owned(), &files).expect("just inserted"),
        Field { width: 7 },
    );
}

#[test]
fn a_level_that_is_not_there_is_malformed_rather_than_a_panic() {
    let files = Memory::new();
    let why = Field::load(&"field".to_owned(), &files).expect_err("nothing was inserted");
    assert_eq!(why.path.as_deref(), Some("field"));
}

#[test]
fn a_level_that_is_there_and_will_not_parse_says_so_differently() {
    let mut files = Memory::new();
    files.insert("field", vec![]);
    let why = Field::load(&"field".to_owned(), &files).expect_err("no bytes to read a width from");
    assert_eq!(why.why, "a field needs at least one byte");
}

#[test]
fn a_tick_advances_and_commands_through_the_sink() {
    let level = Field { width: 2 };
    let mut sink = Recorder::default();
    let step = Step(true);

    let one = Walk::default().tick(&level, &walker(&step), &(), &mut sink);
    assert_eq!(one.at, 1);
    assert_eq!(sink.quits, [], "not at the end yet");

    let two = one.tick(&level, &walker(&step), &(), &mut sink);
    assert_eq!(two.at, 2);
    assert_eq!(
        sink.quits,
        [ExitCode::SUCCESS],
        "the end quits, exactly once"
    );
}

/// The whole reason the sink is a trait rather than a returned `Vec`.
#[test]
fn a_sink_that_implements_nothing_compiles_and_drops_everything() {
    #[derive(Debug)]
    struct Deaf;
    impl Command for Deaf {
        type Reference = String;
    }

    let level = Field { width: 1 };
    let step = Step(true);
    let walked = Walk::default().tick(&level, &walker(&step), &(), &mut Deaf);
    assert_eq!(walked.at, 1, "the tick ran; only the request went nowhere");
}

/// A tick that commands nothing allocates nothing, which is what replacing the
/// returned `Vec<Command>` bought.
#[test]
fn a_tick_that_asks_for_nothing_records_nothing() {
    let level = Field { width: 10 };
    let mut sink = Recorder::default();
    let idle = Step(false);
    let walked = Walk::default().tick(&level, &walker(&idle), &(), &mut sink);
    assert_eq!(walked.at, 0);
    assert_eq!(sink, Recorder::default());
}

#[test]
fn loading_a_level_folds_it_into_the_state() {
    let first = Field { width: 5 };
    let second = Field { width: 3 };

    let opened = Walk::default().load_level(None, &first);
    assert_eq!(opened.field, 5, "the first level has no predecessor");

    let walked = Walk { at: 4, ..opened };
    let moved = walked.load_level(Some(&first), &second);
    assert_eq!(moved.at, 3, "clamped into a shorter field");
    assert_eq!(moved.field, 8);
}

/// `Default` is the opening state, which is what let `Opening::origin` become
/// optional.
#[test]
fn a_state_opens_from_its_own_default() {
    assert_eq!(Walk::default(), Walk { at: 0, field: 0 });
}

/// The two defaults on `State` mean the smallest possible game states four
/// types and a name and stops.
#[test]
fn a_game_that_does_nothing_needs_no_function_at_all() {
    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    struct Still;

    impl Level for Still {
        type Reference = String;
        fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> {
            Ok(Self)
        }
    }

    impl State for Still {
        const NAME: &'static str = "still";
        type Rules = ();
        type Level = Self;
        type Action = ();
    }

    let mut sink = Recorder::default();
    assert_eq!(
        Still.tick(&Still, &[], &(), &mut sink),
        Still,
        "the default tick is the identity",
    );
}

/// `Time` carries where the session is and nothing about interpolation.
///
/// The weight the GPU lerps with is `Render::draw`'s own argument, because it
/// goes straight into a uniform — and because `set_time_factor`, which says how
/// fast game time passes, was one word away from having the same name.
#[test]
fn time_is_a_tick_and_a_wall_clock() {
    let time = Time {
        tick: corvid_time::Tick(42),
        elapsed: core::time::Duration::from_secs(3),
    };
    assert_eq!(time.tick.0, 42);
    assert_eq!(time.elapsed.as_secs(), 3);
}
