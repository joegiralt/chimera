//! MIDI vocabulary and pure decisions: no hardware, no allocation.
#![no_std]
#![forbid(unsafe_code)]

pub mod ctl;
pub mod law;
pub mod learn;
pub mod out;
pub mod rel;
pub mod sense;
pub mod stream;
pub mod sxmap;
pub mod sysex;
pub mod take;
pub mod uac;
pub mod uac_link;
pub mod usb;
pub mod usb_class;
pub mod value;
