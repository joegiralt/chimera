//! Frozen disk codes (ADR 0045): every stored enum's code is permanent.

use chimera_core::addr::{BlockRef, Blocks};
use chimera_core::block::{Block, ParamId, ParamKind, ValFmt};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::modulation::ModSource;
use chimera_core::params::{FilterParams, ParamSnapshot};
use chimera_core::part::PartParams;
use chimera_core::storage::{DiskValue, RETIRED, ValidAddr, read_value};
use chimera_core::ui::theme_settings::ThemeSettings;

/// One of each block's owner, so any stored `BlockRef` resolves.
struct Bank {
    snap: ParamSnapshot,
    fx: FxParams,
    part: PartParams,
    theme: ThemeSettings,
}

impl Bank {
    fn new() -> Self {
        Self {
            snap: ParamSnapshot::default(),
            fx: FxParams::default(),
            part: PartParams::default(),
            theme: ThemeSettings::DEFAULT,
        }
    }

    fn block(&mut self, b: BlockRef) -> &mut dyn Block {
        match b {
            BlockRef::Chorus => &mut self.fx.chorus,
            BlockRef::Delay => &mut self.fx.delay,
            BlockRef::Reverb => &mut self.fx.reverb,
            BlockRef::Tape => &mut self.fx.tape,
            BlockRef::Comp => &mut self.fx.comp,
            BlockRef::Part => &mut self.part,
            BlockRef::Theme => &mut self.theme,
            b => self.snap.block_mut(b).expect("a Sound block"),
        }
    }
}

fn stored() -> impl Iterator<Item = (BlockRef, u8)> {
    BlockRef::ALL
        .into_iter()
        .filter_map(|b| b.disk_code().map(|c| (b, c)))
}

/// The value as the UI shows it.
fn shown(fmt: ValFmt, v: u8) -> String {
    match fmt {
        ValFmt::Names(n) => n[v as usize].to_string(),
        ValFmt::OneBased(_) => (v + 1).to_string(),
        _ => v.to_string(),
    }
}

fn table() -> Vec<String> {
    let mut out = Vec::new();
    for (b, code) in stored() {
        let mut bank = Bank::new();
        for s in b.specs() {
            out.push(format!("B {code} {} {}", s.id.0, s.label));
            if s.kind != ParamKind::Enum {
                continue;
            }
            for v in 0..=s.max as u8 {
                let blk = bank.block(b);
                blk.set(s.id, f32::from(v));
                let c = blk.enum_code(s.id).expect("an Enum has a code");
                out.push(format!("E {code} {} {c} {}", s.id.0, shown(s.fmt, v)));
            }
        }
    }
    out
}

#[test]
fn table_matches_golden() {
    // Every frozen line must still be produced. New lines are appended to the
    // fixture by hand, never edited in and never removed.
    let golden = include_str!("fixtures/disk_codes_v1.txt");
    let now = table();
    for line in golden.lines() {
        assert!(
            now.iter().any(|l| l == line),
            "frozen line changed or gone: {line}"
        );
    }
}

#[test]
fn every_enum_param_has_codes() {
    for (b, _) in stored() {
        for s in b.specs().iter().filter(|s| s.kind == ParamKind::Enum) {
            for v in 0..=s.max as u8 {
                let mut a = Bank::new();
                a.block(b).set(s.id, f32::from(v));
                let c = a.block(b).enum_code(s.id);
                let c = c.unwrap_or_else(|| panic!("{b:?} {} has no code", s.label));
                let mut fresh = Bank::new();
                // Order matters for FORM (its MODE first), so load in table order.
                for earlier in b.specs().iter().filter(|e| e.id.0 < s.id.0) {
                    if earlier.kind == ParamKind::Enum {
                        let ec = a.block(b).enum_code(earlier.id).unwrap();
                        assert!(fresh.block(b).set_enum_code(earlier.id, ec));
                    }
                }
                assert!(
                    fresh.block(b).set_enum_code(s.id, c),
                    "{b:?} {} {c}",
                    s.label
                );
                assert_eq!(
                    fresh.block(b).get(s.id),
                    f32::from(v),
                    "{b:?} {} {c}",
                    s.label
                );
            }
        }
    }
}

#[test]
fn non_enum_params_have_no_code() {
    let mut bank = Bank::new();
    let blk = bank.block(BlockRef::Filter);
    assert_eq!(blk.enum_code(FilterParams::CUTOFF), None);
    assert!(!blk.set_enum_code(FilterParams::CUTOFF, 1));
}

#[test]
fn unknown_code_writes_nothing() {
    let mut f = FilterParams::default();
    let before = f.get(FilterParams::MODE);
    assert!(!f.set_enum_code(FilterParams::MODE, 250));
    assert_eq!(f.get(FilterParams::MODE), before);
    assert!(!f.set_enum_code(FilterParams::KIND, 250));
    // 250 and 200 (two's complement −6, −56) are no enum's value: refused, no panic.
    for (b, _) in stored() {
        for s in b.specs().iter().filter(|s| s.kind == ParamKind::Enum) {
            let mut bank = Bank::new();
            let blk = bank.block(b);
            let was = blk.get(s.id);
            assert!(!blk.set_enum_code(s.id, 250), "{b:?} {}", s.label);
            assert!(!blk.set_enum_code(s.id, 200), "{b:?} {}", s.label);
            assert_eq!(blk.get(s.id), was);
        }
    }
}

#[test]
fn block_codes_unique_and_round_trip() {
    let mut seen = Vec::new();
    for (b, c) in stored() {
        assert_eq!(BlockRef::from_disk_code(c), Some(b));
        assert!(!seen.contains(&c), "code {c} twice");
        seen.push(c);
    }
    assert_eq!(seen.len(), BlockRef::ALL.len() - 1);
    assert_eq!(BlockRef::Channels.disk_code(), None);
    assert_eq!(BlockRef::from_disk_code(0), None);
    assert_eq!(BlockRef::from_disk_code(27), None);
    assert_eq!(BlockRef::from_disk_code(255), None);
}

#[test]
fn mod_source_codes_round_trip() {
    let want = [
        (ModSource::Env1, 0),
        (ModSource::Lfo1, 1),
        (ModSource::Env2, 2),
        (ModSource::Env3, 3),
        (ModSource::Lfo2, 4),
        (ModSource::Lfo3, 5),
        (ModSource::Vel, 6),
        (ModSource::Note, 7),
    ];
    assert_eq!(want.len(), ModSource::ALL.len());
    for (s, c) in want {
        assert_eq!(s.disk_code(), c);
        assert_eq!(ModSource::from_disk_code(c), Some(s));
    }
    assert_eq!(ModSource::from_disk_code(8), None);
    assert_eq!(ModSource::from_disk_code(255), None);
}

#[test]
fn retired_never_live() {
    assert_eq!(RETIRED, &[(10, 3), (10, 4), (10, 5)]);
    for &(block, id) in RETIRED {
        let b = BlockRef::from_disk_code(block).expect("a stored block");
        assert!(b.specs().iter().all(|s| s.id.0 != id), "{b:?} {id} is live");
        assert!(ValidAddr::find(b, ParamId(id)).is_none());
    }
}

#[test]
fn valid_addr_only_from_specs() {
    assert!(ValidAddr::find(BlockRef::Filter, ParamId(4)).is_none());
    assert!(ValidAddr::find(BlockRef::Filter, ParamId(6)).is_some());
    let ids: Vec<u8> = ValidAddr::of_block(BlockRef::Filter)
        .map(|a| a.addr().param.0)
        .collect();
    assert_eq!(ids, [0, 1, 2, 6, 7]);
    let kind = ValidAddr::find(BlockRef::Filter, FilterParams::KIND).unwrap();
    assert!(kind.coded());
    assert_eq!(kind.spec().label, "KIND");
    assert!(
        !ValidAddr::find(BlockRef::Filter, FilterParams::CUTOFF)
            .unwrap()
            .coded()
    );
}

#[test]
fn read_value_is_code_or_real() {
    let f = FilterParams::default();
    let mode = ValidAddr::find(BlockRef::Filter, FilterParams::MODE).unwrap();
    // LP24's own code is 2, though it is index 0 in the SVF's list.
    assert!(matches!(read_value(&f, mode), DiskValue::Code(2)));
    let cut = ValidAddr::find(BlockRef::Filter, FilterParams::CUTOFF).unwrap();
    assert!(matches!(read_value(&f, cut), DiskValue::Real(v) if v == 1000.0));
}
