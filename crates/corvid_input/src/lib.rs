#![doc = include_str!("../README.md")]
#![no_std]

// The snapshot holds one value per declared action, and the declaration is not
// known until a game writes it down, so the storage is a `Vec`. That is the
// only thing here that needs an allocator, and there is no `std` in this crate
// under any feature it currently has.
extern crate alloc;

mod cursor;
mod id;
mod scale;
mod sets;
mod snapshot;
mod source;
mod value;

#[cfg(feature = "platform")]
pub mod platform;

pub use cursor::Cursor;
pub use id::{AnalogId, DigitalId, PoseId, SetId};
pub use scale::{scale, scale_coarse};
pub use sets::{
    IdRange, SetDescriptor, SetNames, analog_name, analog_named, digital_name, digital_named,
    layout, pose_name, pose_named,
};
pub use snapshot::Input;
pub use value::{Analog, Digital, Viewport};
// The vocabulary a control is named in, which is *not* behind the `platform`
// feature and is re-exported from it for the paths that already say
// `platform::Button`. A `Key` names no device driver and needs no operating
// system -- it is data, exactly as a `DigitalId` is -- and `Input::captured`
// carries one, so a game asking "which control did the player just press" does
// not have to turn on the half that reads a keyboard to hear the answer. What
// the feature gates is the table and the accumulator: `Bindings` and `Devices`.
pub use source::{Axis, Button, Key, MouseButton, PadButton};
