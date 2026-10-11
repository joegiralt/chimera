//! The showcase's factory Sounds (spec § 3, ADR 0134): committed text
//! Sources in `tools/factory-sounds/`, parsed and checked on the host and
//! built into card files through the firmware's own encoder. `source` is
//! pure; `build` reads and writes the files.

pub mod build;
pub mod source;
