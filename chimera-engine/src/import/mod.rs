//! Other synths' dumps read into typed voices (ADR 0088): `no_std`, no
//! allocation, no I/O. The host maps them (`chimera-desktop`'s
//! `import`); SysEx receive (#263) would share the byte layouts.

pub mod tx81z;
