// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(
        out.join("waves.rs"),
        chimera_waves::emit_rust(std::env::var_os("CARGO_FEATURE_BOOT_WAVES").is_none()),
    )
    .expect("write waves.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
