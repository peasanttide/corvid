#![doc = include_str!("../README.md")]
#![no_std]

// No `extern crate alloc`. Nothing here grows: the star catalogue is a `const`
// array, a rise-and-set scan walks a fixed number of steps, and every series is
// a table whose length is written in its own type. An ephemeris that allocated
// would be an ephemeris that could fail, and this one cannot.

mod atmosphere;
mod catalogue;
mod coordinates;
mod deltat;
mod error;
pub mod frame;
mod math;
mod moon;
mod moon_table;
mod observer;
mod rise;
mod sky;
mod star;
mod sun;
mod time;

pub use atmosphere::{Atmosphere, henyey_greenstein, rayleigh_phase};
pub use catalogue::{BRIGHT_STARS, brighter_than};
pub use coordinates::{Equatorial, Horizontal};
pub use error::SkyError;
pub use moon::{Moon, Phase};
pub use observer::Observer;
pub use rise::RiseSet;
pub use sky::{Sky, Twilight};
pub use star::Star;
pub use sun::Sun;
pub use time::{Civil, Instant};
