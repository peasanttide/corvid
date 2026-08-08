#![doc = include_str!("../README.md")]

// No `no_std`, and the reason changed. This crate used to extend
// `corvid_render`, which opens a device — so a game that drew nothing still
// compiled a graphics stack in order to say what its camera was. It does not
// any more: a `Controller` names a `Camera`, an `Input` and a `State`, and none
// of those knows a device exists. What keeps `std` here is `corvid_input`'s
// `platform` feature, which reads a keyboard.

mod controller;

pub use controller::Controller;

// `update`'s last argument, and the only wall-clock quantity in the whole
// contract. Named through `core` rather than `std` because they are the same
// type and the shorter path says where it comes from: a duration is arithmetic
// on two integers and has never needed an operating system.
pub use core::time::Duration;
