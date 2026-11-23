// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Other synths' dumps read into typed voices (ADR 0088): `no_std`, no
//! allocation, no I/O. The host maps them (`chimera-desktop`'s
//! `import`); SysEx receive (#263) would share the byte layouts.

pub mod tx81z;
