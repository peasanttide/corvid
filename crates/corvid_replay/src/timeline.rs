//! Which level a session is played on, tick by tick.
//!
//! A level changes during a session only because a tick asked it to -- an
//! [`edit`](corvid_behavior::Command::edit) or a
//! [`load`](corvid_behavior::Command::load) -- and the change holds from the
//! next tick on. A [`Timeline`] keeps each level the session has been played
//! on, from the tick it started: the level for any tick is a lookup, and a
//! rollback forgets what came after the tick it rolled back to (the
//! resimulation asks again).
//!
//! It also keeps the level the session first opened on and every change
//! since, which a session forgetting its past does not prune. That is what a
//! machine that needs the current level without replaying the session -- a
//! peer taking a state transfer, a guest joining a lobby's session -- is sent:
//! the changes are small where the level need not be, and every machine in
//! a session opened on the same level.
//!
//! None of it is written into a capture or a save: the log reproduces it
//! from the opening, and whatever reads a session back rebuilds it by
//! stepping.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use corvid_behavior::Level;
use corvid_time::Tick;
use serde::{Deserialize, Serialize};

/// One change a tick asked of the level.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Change<E> {
    /// An edit of the level being played.
    Edit(E),
    /// Another level, by the name [`Level::load`] reads.
    Load(String),
}

/// The changes a session's level went through: from each tick, what the tick
/// before it asked for.
pub type Changes<E> = Vec<(Tick, Vec<Change<E>>)>;

/// One level and the first tick played on it.
#[derive(Clone, Debug, PartialEq)]
struct Entry<L> {
    from: Tick,
    level: Arc<L>,
}

/// Every level a session has been played on, and from which tick.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline<L: Level> {
    /// The level the session first opened on, before any forgetting.
    origin: Arc<L>,
    /// Every change since, oldest first.
    history: Changes<L::Edit>,
    /// The levels to look a tick up in, oldest first; never empty.
    entries: Vec<Entry<L>>,
}

impl<L: Level> Timeline<L> {
    /// A session played on one level throughout, so far.
    #[must_use]
    pub fn new(opening: Arc<L>) -> Self {
        Self {
            entries: alloc::vec![Entry {
                from: Tick::ZERO,
                level: Arc::clone(&opening),
            }],
            origin: opening,
            history: Vec::new(),
        }
    }

    /// The level the session first opened on.
    #[must_use]
    pub const fn origin(&self) -> &Arc<L> {
        &self.origin
    }

    /// The level tick `at` is played on.
    #[must_use]
    pub fn at(&self, at: Tick) -> &Arc<L> {
        let index = self
            .entries
            .iter()
            .rposition(|entry| entry.from <= at)
            .unwrap_or(0);
        self.entries
            .get(index)
            .map_or(&self.origin, |entry| &entry.level)
    }

    /// The level played on last.
    #[must_use]
    pub fn current(&self) -> &Arc<L> {
        self.entries
            .last()
            .map_or(&self.origin, |entry| &entry.level)
    }

    /// Records that the ticks from `from` on are played on `level`, which
    /// `changes` made. Whatever was recorded from `from` on before is
    /// replaced.
    pub fn push(&mut self, from: Tick, level: Arc<L>, changes: Vec<Change<L::Edit>>) {
        self.forget_after(from.prev());
        self.entries.push(Entry { from, level });
        self.history.push((from, changes));
    }

    /// Forgets every level that began after tick `at`: a rollback to `at` is
    /// about to play the ticks after it again.
    pub fn forget_after(&mut self, at: Tick) {
        let keep = 1 + self
            .entries
            .iter()
            .skip(1)
            .take_while(|entry| entry.from <= at)
            .count();
        self.entries.truncate(keep);
        self.history.retain(|(from, _)| *from <= at);
    }

    /// Stops looking up ticks before `at`, keeping the level played there as
    /// the first: what a session keeps when it forgets its past. The origin
    /// and the history stay.
    pub fn forget_before(&mut self, at: Tick) {
        let level = Arc::clone(self.at(at));
        let later: Vec<Entry<L>> = self
            .entries
            .iter()
            .filter(|entry| entry.from > at)
            .cloned()
            .collect();
        self.entries = alloc::vec![Entry {
            from: Tick::ZERO,
            level,
        }];
        self.entries.extend(later);
    }

    /// Whether the level changed at all during the session.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.history.is_empty()
    }

    /// Every change since the session opened, with the tick it holds from.
    #[must_use]
    pub fn changes(&self) -> &Changes<L::Edit> {
        &self.history
    }

    /// The timeline a session opening on `origin` has after `changes`.
    ///
    /// A change the level refuses is dropped, as it was when it was first
    /// made, so this rebuilds exactly what the machine that sent the changes
    /// holds.
    #[must_use]
    pub fn rebuild(origin: Arc<L>, changes: &Changes<L::Edit>) -> Self {
        let mut timeline = Self::new(origin);
        for (from, asked) in changes {
            let level = crate::step::apply(timeline.current(), asked);
            timeline.push(*from, level, asked.clone());
        }
        timeline
    }
}
