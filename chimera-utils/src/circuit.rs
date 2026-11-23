// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `circuit list|render`: audition any `chimera-circuits` model (ADR 0196)
//! on unitab's scripts, through unitab's lab source (a saw at ½ gated by
//! note, CUTOFF and RES from the script's CCs), so its files line up with
//! unitab's `lab_*` and `eng_*` renders and `unitab cmp` measures them.
//!
//! The filter-alone harness is research, not a page source (`ab-method.md`):
//! these models have no place in the engine yet, so the engine render is
//! not possible for them. Levels are raw, every file at the model's own
//! level, 32-bit float, 48 kHz; and every render first passes the method's
//! known-pair check on the same source and writer, so the pipeline is
//! shown not to normalise.

use crate::unitab::{self, RES_CC, SCRIPTS};
use chimera_hal::SAMPLE_RATE;
use std::path::{Path, PathBuf};

fn list() {
    for e in chimera_circuits::models() {
        println!("{:28} {}", e.name, e.about);
    }
}

/// `circuit render MODEL OUTDIR [EVENTS]`: `ckt_<model>_<script>_r<res>.wav`
/// for every script × RES (19, 64, 95, 114), after the known pair.
fn render(model: &str, dir: &Path, events: &Path) -> Result<(), String> {
    let Some(entry) = chimera_circuits::models()
        .into_iter()
        .find(|e| e.name == model)
    else {
        return Err(format!("no model {model}: `chimera-utils circuit list`"));
    };
    print!("{}", unitab::check_known_pair(dir, unitab::lab)?);
    let tag = model.replace('/', "_");
    use rayon::prelude::*;
    let jobs: Vec<(&str, u8)> = SCRIPTS
        .iter()
        .flat_map(|s| RES_CC.map(|r| (*s, r)))
        .collect();
    jobs.par_iter()
        .try_for_each(|&(s, r)| -> Result<(), String> {
            let sc = unitab::script(events, s)?;
            let (x, c) = unitab::lab_input(&sc, r);
            let y: Vec<f32> = (entry.make)()
                .render(&x, &c)
                .iter()
                .map(|&v| v as f32)
                .collect();
            let p = dir.join(format!("ckt_{tag}_{s}_r{r}.wav"));
            crate::reference::write_f32(&p, &y, SAMPLE_RATE)?;
            println!("{}", p.display());
            Ok(())
        })
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args {
        [c] if c == "list" => {
            list();
            Ok(())
        }
        [c, m, d] if c == "render" => render(m, Path::new(d), &unitab::events()),
        [c, m, d, e] if c == "render" => render(m, Path::new(d), &PathBuf::from(e)),
        _ => Err("circuit list | circuit render MODEL OUTDIR [EVENTS]".into()),
    }
}
