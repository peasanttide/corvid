//! The entity inspector, over a game's own view of its state.

use core::fmt;

/// A game's own view of its state, for the inspector.
///
/// The game names its rows; this crate renders them. There is no reflection,
/// because reflection over a `State` is a second serialization format that can
/// disagree with the first -- and the first is the one two peers compare.
///
/// ```
/// use corvid_dev::{Inspect, Rows};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// struct Nowhere;
///
/// impl corvid_behavior::Level for Nowhere {
///     type Error = core::convert::Infallible;
///     fn load(_: &str) -> Result<Self, Self::Error> { Ok(Self) }
/// }
///
/// #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// struct Swarm {
///     wave: u32,
///     towers: Vec<String>,
/// }
///
/// impl corvid_behavior::State for Swarm {
///     const NAME: &'static str = "swarm";
///     type Level = Nowhere;
///     type Rules = ();
///     type Action = ();
/// }
///
/// /// The rows this game shows, in the order it wants them read.
/// impl Inspect for Swarm {
///     fn inspect(state: &Self, out: &mut Rows) {
///         out.field("wave", state.wave);
///         let mut towers = out.group("towers", state.towers.len());
///         for (index, tower) in state.towers.iter().enumerate() {
///             towers.field("kind", (index, tower));
///         }
///     }
/// }
///
/// let mut rows = Rows::new();
/// let swarm = Swarm { wave: 3, towers: vec!["arc".to_owned(), "flame".to_owned()] };
/// Swarm::inspect(&swarm, &mut rows);
/// let names: Vec<_> = rows.rows().iter().map(|row| row.name).collect();
/// assert_eq!(names, ["wave", "towers", "kind", "kind"]);
/// ```
pub trait Inspect: corvid_behavior::State {
    /// One entry per named thing, in a stable order.
    fn inspect(state: &Self, out: &mut Rows);
}

/// One line of the inspector.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Row {
    /// What the game called it.
    pub name: &'static str,
    /// Its `Debug`, which is the one rendering of a value a game already has.
    pub value: String,
    /// How far in it is nested, counted from zero.
    pub depth: u8,
    /// How many things are under it, for a row that opened a group.
    pub count: Option<usize>,
}

/// What [`Inspect::inspect`] fills in.
///
/// ```
/// use corvid_dev::Rows;
///
/// let mut rows = Rows::new();
/// rows.field("tick", 40_u32);
/// {
///     let mut towers = rows.group("towers", 2);
///     towers.field("first", "arc");
///     towers.field("second", "flame");
/// }
/// rows.field("wave", 3_u32);
///
/// // Declaration order, and the group reports its count.
/// let names: Vec<_> = rows.rows().iter().map(|row| row.name).collect();
/// assert_eq!(names, ["tick", "towers", "first", "second", "wave"]);
/// assert_eq!(rows.rows()[1].count, Some(2));
/// assert_eq!(rows.rows()[2].depth, 1);
/// assert_eq!(rows.rows()[4].depth, 0);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rows {
    rows: Vec<Row>,
    depth: u8,
}

impl Rows {
    /// No rows.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            rows: Vec::new(),
            depth: 0,
        }
    }

    /// One named value.
    pub fn field(&mut self, name: &'static str, value: impl fmt::Debug) {
        self.rows.push(Row {
            name,
            value: format!("{value:?}"),
            depth: self.depth,
            count: None,
        });
    }

    /// A named group of `count` things. Rows added through the returned
    /// [`Group`] are one level further in.
    pub fn group(&mut self, name: &'static str, count: usize) -> Group<'_> {
        self.rows.push(Row {
            name,
            value: String::new(),
            depth: self.depth,
            count: Some(count),
        });
        self.depth = self.depth.saturating_add(1);
        Group { rows: self }
    }

    /// Everything named, in declaration order.
    #[must_use]
    #[inline]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// How many rows there are.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether nothing was named.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The rows a state produces.
    #[must_use]
    pub fn of<S: Inspect>(state: &S) -> Self {
        let mut rows = Self::new();
        S::inspect(state, &mut rows);
        rows
    }
}

impl From<Rows> for Vec<Row> {
    #[inline]
    fn from(rows: Rows) -> Self {
        rows.rows
    }
}

/// One level of nesting, open until it is dropped.
#[derive(Debug)]
pub struct Group<'a> {
    rows: &'a mut Rows,
}

impl Group<'_> {
    /// One named value, inside this group.
    pub fn field(&mut self, name: &'static str, value: impl fmt::Debug) {
        self.rows.field(name, value);
    }

    /// A group inside this one.
    pub fn group(&mut self, name: &'static str, count: usize) -> Group<'_> {
        self.rows.group(name, count)
    }
}

/// Closes the group, so the next row is back at the outer level.
impl Drop for Group<'_> {
    fn drop(&mut self) {
        self.rows.depth = self.rows.depth.saturating_sub(1);
    }
}
