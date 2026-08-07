//! A game names its subsystems, so a desync report names them too.

use alloc::vec::Vec;
use core::hash::Hash;

use corvid_behavior::State;
use corvid_hash::{Digest, digest};
use corvid_time::Tick;

use crate::{FieldReport, Where};

/// A game names its subsystems, so a desync report names them too.
///
/// There is no reflection over a `State`, because reflection is a second
/// serialization format that can disagree with the first. The default probes
/// the whole state as one field, which is true and much less useful.
///
/// ```
/// # use corvid_lockstep::{Bisect, Probes};
/// # use corvid_behavior::{Level, Malformed, Source, State};
/// # #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// # struct Ground;
/// # impl Level for Ground {
/// #     type Reference = String;
/// #     fn load(_: &String, _: &dyn Source) -> Result<Self, Malformed> { Ok(Self) }
/// # }
/// #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// struct Towers { position: Vec<i32>, velocity: Vec<i32>, towers: Vec<i32> }
/// # impl State for Towers {
/// #     const NAME: &'static str = "towers";
/// #     type Level = Ground;
/// #     type Rules = ();
/// #     type Action = ();
/// # }
/// impl Bisect for Towers {
///     fn probe(state: &Towers, out: &mut Probes) {
///         out.column("state.creeps.position", &state.position);
///         out.column("state.creeps.velocity", &state.velocity);
///         out.column("state.towers", &state.towers);
///     }
/// }
///
/// let mut probes = Probes::default();
/// Towers::probe(&Towers::default(), &mut probes);
/// assert_eq!(probes.reports().len(), 3);
/// ```
pub trait Bisect: State {
    /// Digests each subsystem separately, in a stable order.
    ///
    /// The order is the order of the calls, and it is what the report is
    /// printed in and what a remote peer's probes are matched against. Two
    /// peers running the same build make the same calls, so the two lists line
    /// up by position and no names have to go on the wire.
    fn probe(state: &Self, out: &mut Probes) {
        out.field("state", state);
    }

    /// Which row first differs, given the remote's per-row digests.
    ///
    /// Optional because it is only answerable after a state transfer: the two
    /// peers have to have exchanged the column for either of them to say where
    /// inside it they parted. [`Probes::locate`] is the whole of a typical
    /// implementation.
    fn locate(_state: &Self, _probe: &str, _remote: &[Digest]) -> Option<Where> {
        None
    }
}

/// What a [`Bisect`] implementation digests into.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Probes {
    /// One entry per probe, in declaration order. `local` is filled in here and
    /// `remote` starts equal to it, so a `Probes` that has been compared
    /// against nothing reports that everything agrees — which is true.
    reports: Vec<FieldReport>,
    /// Per-row digests for each probe, in the same order, and empty for a probe
    /// declared with [`field`](Self::field) rather than
    /// [`column`](Self::column).
    rows: Vec<Vec<Digest>>,
}

impl Probes {
    /// One named subsystem, digested.
    pub fn field<T: Hash + ?Sized>(&mut self, name: &'static str, value: &T) {
        let mark = digest(value);
        self.reports.push(FieldReport {
            probe: name,
            agrees: true,
            local: mark,
            remote: mark,
        });
        self.rows.push(Vec::new());
    }

    /// One named column, digested per row as well as whole — which is what
    /// makes `first divergent index` findable.
    pub fn column<T: Hash>(&mut self, name: &'static str, rows: &[T]) {
        let mark = digest(rows);
        self.reports.push(FieldReport {
            probe: name,
            agrees: true,
            local: mark,
            remote: mark,
        });
        self.rows.push(rows.iter().map(digest).collect());
    }

    /// What every probe found, in declaration order.
    #[must_use]
    pub fn reports(&self) -> &[FieldReport] {
        &self.reports
    }

    /// The per-row digests one named column produced, and an empty slice for
    /// anything declared with [`field`](Self::field).
    #[must_use]
    pub fn rows(&self, probe: &str) -> &[Digest] {
        self.reports
            .iter()
            .position(|report| report.probe == probe)
            .and_then(|at| self.rows.get(at))
            .map_or(&[], Vec::as_slice)
    }

    /// Every probe's whole-subsystem digest, in declaration order — what a peer
    /// sends and what [`TickProbes`] holds.
    #[must_use]
    pub fn marks(&self) -> Vec<Digest> {
        self.reports.iter().map(|report| report.local).collect()
    }

    /// The first row of `probe` whose digest differs from the remote's.
    ///
    /// The *first*, rather than any: a column compared row by row usually
    /// disagrees in a run, and the row that started it is the one worth
    /// printing.
    #[must_use]
    pub fn locate(&self, probe: &str, remote: &[Digest]) -> Option<u32> {
        let local = self.rows(probe);
        let index = local
            .iter()
            .zip(remote)
            .position(|(here, there)| here != there)
            .or_else(|| (local.len() != remote.len()).then_some(local.len().min(remote.len())))?;
        u32::try_from(index).ok()
    }

    /// Compares against a remote peer's probes for the same tick, by position.
    ///
    /// Position rather than name, because the names are `&'static str` from one
    /// build and the two peers are running the same one. A remote list of a
    /// different length is a build mismatch, and the probes past the shorter of
    /// the two are left saying they agree, which is all that can honestly be
    /// said about a subsystem the other side did not report.
    pub fn compare(&mut self, remote: &[Digest]) {
        for (report, mark) in self.reports.iter_mut().zip(remote) {
            report.remote = *mark;
            report.agrees = report.local == *mark;
        }
    }

    /// Whether every compared probe agreed.
    #[must_use]
    pub fn agrees(&self) -> bool {
        self.reports.iter().all(|report| report.agrees)
    }

    /// Forgets everything, keeping the room the reports were held in.
    ///
    /// This is what lets one `Probes` serve every tick of a bisection rather
    /// than one per tick.
    pub fn clear(&mut self) {
        self.reports.clear();
        self.rows.clear();
    }
}

/// One remote peer's probes for one tick.
///
/// Digests in declaration order rather than named, for the reason
/// [`Probes::compare`] gives.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TickProbes {
    /// Which tick these are the probes of.
    pub at: Tick,
    /// Each probe's whole-subsystem digest, in declaration order.
    pub fields: Vec<Digest>,
    /// Each probe's per-row digests, in the same order, and empty for a probe
    /// the sender declared with [`Probes::field`] or did not send rows for.
    pub rows: Vec<Vec<Digest>>,
}

impl TickProbes {
    /// Probes a state, which is what the peer on the other side sends.
    #[must_use]
    pub fn of<S: Bisect>(at: Tick, state: &S) -> Self {
        let mut probes = Probes::default();
        S::probe(state, &mut probes);
        Self {
            at,
            fields: probes.marks(),
            rows: probes.rows.clone(),
        }
    }

    /// The per-row digests the probe at `index` carried.
    #[must_use]
    pub fn rows_at(&self, index: usize) -> &[Digest] {
        self.rows.get(index).map_or(&[], Vec::as_slice)
    }
}

#[cfg(feature = "dev")]
pub use with_dev::bisect;

#[cfg(feature = "dev")]
mod with_dev {
    use alloc::vec::Vec;

    use corvid_time::Tick;

    use super::{Bisect, Probes, TickProbes};
    use crate::{Desync, Halt, Peer, predict::row_at, rollback::step};

    /// Re-simulates from the last agreed snapshot with probes on, and reports
    /// the first tick at which any named probe differs.
    ///
    /// The work is the length of the disagreement rather than the length of the
    /// session: it starts at the oldest tick `remote` carries probes for, which
    /// is the last tick the two peers agreed on, and stops at the first tick
    /// they do not. A two-thousand-tick session whose peers parted at 1 999
    /// therefore bisects in one tick.
    ///
    /// # Errors
    ///
    /// [`Halt::Refused`] if the log cannot answer for a tick `remote` names,
    /// and [`Halt::Unreachable`] if the peer holds no state at or before the
    /// oldest tick it names.
    pub fn bisect<S: Bisect>(peer: &Peer<S>, remote: &[TickProbes]) -> Result<Desync, Halt> {
        let Some(first) = remote.first() else {
            return Ok(peer.desync_at(peer.tick(), Vec::new(), None));
        };

        let (mut at, mut state) = peer.restore(first.at)?;
        let mut row: Vec<S::Action> = Vec::new();
        let mut probes = Probes::default();
        let mut last = None;

        for expected in remote {
            while at < expected.at {
                row_at(&peer.session.log, &peer.frontier, at, &mut row);
                // The commands go nowhere. A bisector is re-simulating ticks
                // this peer has already played in order to find where two
                // machines stopped agreeing, and a tick replayed by an
                // investigation is not a tick asking the runtime for anything a
                // second time.
                state = step::<S>(
                    &peer.session,
                    &state,
                    at,
                    &row,
                    &mut corvid_behavior::Discard::new(),
                );
                at = at.next();
            }

            probes.clear();
            S::probe(&state, &mut probes);
            probes.compare(&expected.fields);
            if !probes.agrees() {
                let first_divergent = probes
                    .reports()
                    .iter()
                    .position(|report| !report.agrees)
                    .and_then(|index| {
                        let probe = probes.reports().get(index)?.probe;
                        S::locate(&state, probe, expected.rows_at(index))
                    });
                let desync = peer.desync_at(at, probes.reports().to_vec(), first_divergent);
                return Ok(desync);
            }
            last = Some(at);
        }

        let at = last.unwrap_or(Tick::ZERO);
        let desync = peer.desync_at(at, probes.reports().to_vec(), None);
        Ok(desync)
    }
}
