//! The composer: twelve musical parameters in, a bar of music out.

mod analysis;
mod arrange;
mod bar;
mod chord;
mod composer;
mod cost;
mod melody;
mod mode;
mod motif;
mod ornament;
mod params;
mod phrase;
mod search;
mod tension;
mod voicing;

pub use analysis::{contour_similarity, parallel_perfects};
pub use bar::{Bar, Note, Ornament, Role, Voice};
pub use chord::{Cadence, Chord, Quality};
pub use composer::Composer;
pub use mode::{Mode, Step};
pub use motif::{Event, Motif, MotifId, MotifPool, Subject, Transform, transform};
pub use params::Parameters;
