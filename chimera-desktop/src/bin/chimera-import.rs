// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Host importers: files from outside into Chimera's own. `tape` (TAPE
//! spec § 6): a WAV to a sealed source on the desktop sim's card
//! (`CHIMERA_CARD`, else `chimera-card/`). Never the unit's card: a
//! source reaches it only by `put`. `tx` (showcase spec § 6): a TX81Z
//! bank's voices to private port Sources in `--out` (default
//! `$CHIMERA_TX_PORTS`), never inside a work tree unless git ignores it.

use chimera_core::storage::Card;
use chimera_desktop::factory_sounds::build;
use chimera_desktop::import::tape::{TapeOpts, import};
use chimera_desktop::import::tx81z;
use chimera_desktop::store::{DirStore, sim_card};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: chimera-import tape <file.wav> [--rate 24k] [--root C3] \
                     [--transwave <cycle> [--top 1024] [--frames 32]] [--name CHOIR] [--channel mix|L|R]\n\
                     \x20      chimera-import tx <bank.syx> [--voices 1,4,9-12] [--out DIR] [--force]";

/// `tx <bank> [--voices LIST] [--out DIR] [--force]`.
fn tx_args(rest: &[String]) -> Result<(PathBuf, Vec<u8>, PathBuf, bool), String> {
    let (mut bank, mut voices, mut out, mut force) = (None, Vec::new(), None, false);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--voices" => voices = tx81z::voices_arg(it.next().ok_or("--voices LIST")?)?,
            "--out" => out = Some(PathBuf::from(it.next().ok_or("--out DIR")?)),
            "--force" => force = true,
            f if bank.is_none() && !f.starts_with("--") => bank = Some(PathBuf::from(f)),
            f => return Err(format!("tx: {f}?")),
        }
    }
    let out = out.unwrap_or_else(|| build::ports_dir(std::env::var_os(build::PORTS_VAR)));
    Ok((bank.ok_or("tx <bank.syx>")?, voices, out, force))
}

fn tx(rest: &[String]) -> ExitCode {
    let (bank, voices, out, force) = match tx_args(rest) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("chimera-import: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match tx81z::import(&bank, &voices, &out, force) {
        Ok(report) => {
            print!("{report}");
            println!("{}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("chimera-import tx: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let o = match args.split_first() {
        Some((cmd, rest)) if cmd == "tape" => TapeOpts::from_args(rest),
        Some((cmd, rest)) if cmd == "tx" => return tx(rest),
        _ => Err("tape or tx".into()),
    };
    let o = match o {
        Ok(o) => o,
        Err(e) => {
            eprintln!("chimera-import: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let card = sim_card();
    let mut store = DirStore::new(card.clone());
    let out = Card::new()
        .run(&mut store, |s, r| import(&o, s, r))
        .and_then(|o| o.result);
    match out {
        Ok(report) => {
            println!("SIM CARD {}", card.display());
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("chimera-import tape {}: {e}", o.wav.display());
            ExitCode::FAILURE
        }
    }
}
