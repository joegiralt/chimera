//! Host importers: files from outside into Chimera's own. One subcommand,
//! `tape` (TAPE spec § 6): a WAV to a sealed source on the desktop sim's
//! card (`CHIMERA_CARD`, else `chimera-card/`). Never the unit's card: a
//! source reaches it only by `put`.

use chimera_core::storage::Card;
use chimera_desktop::import::tape::{TapeOpts, import};
use chimera_desktop::store::{DirStore, sim_card};
use std::process::ExitCode;

const USAGE: &str = "usage: chimera-import tape <file.wav> [--rate 24k] [--root C3] \
                     [--transwave <cycle> [--top 1024]] [--name CHOIR] [--channel mix|L|R]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let o = match args.split_first() {
        Some((cmd, rest)) if cmd == "tape" => TapeOpts::from_args(rest),
        _ => Err("one subcommand: tape".into()),
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
