#![doc = include_str!("../README.md")]
#![no_std]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    reason = "narrowing between the widths a fixed-point product needs and the widths it is stored in is this crate's subject matter; every cast is preceded by a range check, a saturating conversion, or a shift whose bound is stated"
)]

// A mesh is a growable list of triangles and a growable grid over them, so
// `alloc` is the one thing past `core` this crate needs -- and the whole of it.
extern crate alloc;

mod colour;
mod cords;
mod diffuse;
mod error;
mod grid;
mod inside;
mod linear;
mod mesh;
mod plane;
mod resolve;
mod scaled;
mod seam;
mod step;
mod stitch;
mod tri;

pub use colour::{MAX_COLOURS, NavColours};
pub use cords::{MAX_HEIGHT, NavCords, NavState, NavTriRef};
pub use diffuse::diffuse_step;
pub use error::NavError;
pub use grid::{NavCell, NavGrid};
pub use inside::EDGE_MARGIN;
pub use linear::{Affine3, Linear3};
pub use mesh::NavMesh;
pub use plane::NavPlane;
pub use scaled::Scaled3;
pub use seam::NavTriEdge;
pub use step::{
    NavEvent, Tune, apply_drag, apply_gravity, calc_collision_vs_plane, calc_next_nav_tri,
    kinematic_step, pick_next_event,
};
pub use tri::{MAX_EDGE, NavTri};
