// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ed describe`'s default is INIT's value (ADR 0337): on every engine,
//! every param a page lists defaults to what INIT stores there, so the
//! editor's double-click to default lands where a fresh INIT Sound is.
mod common;
mod screen;
use common::console_fake::{Fake, ask};

use chimera_core::project::PartId;
use chimera_engine::block::DiskCode;
use chimera_engine::params::EngineType;

const ENGINES: [EngineType; 5] = [
    EngineType::Algo,
    EngineType::Modal,
    EngineType::Va,
    EngineType::Tape,
    EngineType::Drum,
];

/// `ed get`'s stored value.
fn stored(f: &mut Fake, addr: &str) -> f32 {
    let got = ask(f, &format!("ed get {addr}\n"));
    let line = got.lines().next().unwrap();
    let w: Vec<&str> = line.split(' ').collect();
    assert_eq!(w[0], "val", "{addr}: {got}");
    w[3].parse().unwrap()
}

#[test]
fn every_engines_describe_defaults_to_init() {
    let mut bad = Vec::new();
    for e in ENGINES {
        let mut f = Fake::new();
        screen::init_part_engine(&mut f.ui, PartId::ALL[0], e);
        let pages = chimera_core::console::ed_pages(e);
        let mut seen = std::collections::BTreeSet::new();
        for n in 0..pages {
            let d = ask(&mut f, &format!("ed describe {} {n}\n", e.disk_ident()));
            for l in d.lines().filter(|l| l.starts_with("param ")) {
                let (head, rest) = l.split_once(" \"").unwrap();
                let addr = head.split(' ').nth(1).unwrap().to_string();
                let (_, tail) = rest.split_once("\" ").unwrap();
                let w: Vec<&str> = tail.split(' ').collect();
                let def: f32 = w[5].parse().unwrap();
                if !seen.insert(addr.clone()) {
                    continue;
                }
                let init = stored(&mut f, &addr);
                if (def - init).abs() > 1e-4 * init.abs().max(1.0) {
                    bad.push(format!("{e:?} {addr}: describe {def}, INIT {init}"));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} defaults off INIT:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
