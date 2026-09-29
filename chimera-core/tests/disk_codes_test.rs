//! Frozen disk codes (ADR 0045): every stored enum's code is permanent.

use chimera_core::addr::{BlockRef, Blocks, Op};
use chimera_core::block::{Block, ParamId, ValFmt};
use chimera_core::dsp::algo::algorithms::{ALGO_IDENTS, ALGO_NAMES};
use chimera_core::dsp::algo::waves::{WAVE_IDENTS, WAVE_NAMES};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::modulator::{EnvForm, EnvSlot, FuncMode, LfoForm};
use chimera_core::modulation::ModSource;
use chimera_core::params::EnvParams;
use chimera_core::params::{FilterParams, ParamSnapshot};
use chimera_core::part::PartParams;
use chimera_core::storage::{
    Crc32, DiskValue, RETIRED, RETIRED_BLOCKS, RETIRED_CODES, RETIRED_SOURCES, ValidAddr,
    read_value,
};
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

/// Every stored fact as `key # readable`. The key is frozen: a code and the
/// explicit identity (`disk_ident`, `ParamSpec::ident`) it stands for, so
/// swapping two codes changes a key. The readable part (a label, a display
/// text) isn't written to the card, isn't frozen, and the check ignores it.
fn table() -> Vec<String> {
    let mut out = Vec::new();
    for (b, code) in stored() {
        out.push(format!("K {code} {} # {b:?}", b.disk_ident().unwrap()));
        let mut bank = Bank::new();
        for a in ValidAddr::of_block(b) {
            let s = a.spec();
            out.push(format!("B {code} {} {} # {}", s.id.0, s.ident, s.label));
            if !a.coded() {
                continue;
            }
            for v in 0..=s.max as u8 {
                let blk = bank.block(b);
                blk.set(s.id, f32::from(v));
                let c = blk.enum_code(s.id).expect("an Enum has a code");
                let ident = blk.enum_ident(s.id).expect("an Enum has an ident");
                out.push(format!(
                    "E {code} {} {c} {ident} # {}",
                    s.id.0,
                    shown(s.fmt, v)
                ));
            }
        }
    }
    for s in ModSource::ALL {
        out.push(format!(
            "S {} {} # {}",
            s.disk_code(),
            s.disk_ident(),
            s.name()
        ));
    }
    out
}

fn key(line: &str) -> &str {
    line.split(" # ").next().unwrap()
}

/// A frozen key the RETIRED lists give up on.
fn retired(key: &str) -> bool {
    let t: Vec<u8> = key
        .split(' ')
        .skip(1)
        .map_while(|x| x.parse().ok())
        .collect();
    match (key.as_bytes()[0], t.as_slice()) {
        (b'K', [b, ..]) => RETIRED_BLOCKS.contains(b),
        (b'B', [b, id, ..]) => RETIRED.contains(&(*b, *id)),
        (b'E', [b, id, c, ..]) => {
            RETIRED.contains(&(*b, *id)) || RETIRED_CODES.contains(&(*b, *id, *c))
        }
        (b'S', [c, ..]) => RETIRED_SOURCES.contains(c),
        _ => false,
    }
}

/// The first `V1_LINES` keys of the fixture are frozen: appending lines
/// passes, any edit to a key inside them changes the CRC. The readable
/// column is outside it, so a typo in a note can be fixed.
const V1_LINES: usize = 606;
const V1_CRC: u32 = 0x8154_6aa2;

#[test]
fn table_matches_golden() {
    let golden = include_str!("fixtures/disk_codes_v1.txt");
    let frozen: Vec<&str> = golden.lines().map(key).collect();
    let mut crc = Crc32::new();
    for k in frozen.iter().take(V1_LINES) {
        crc.update(k.as_bytes());
        crc.update(b"\n");
    }
    assert!(
        frozen.len() >= V1_LINES && crc.finish() == V1_CRC,
        "a frozen v1 key was edited or removed from the fixture"
    );

    let now = table();
    for (i, k) in frozen.iter().enumerate() {
        assert!(!frozen[..i].contains(k), "fixture repeats {k}");
    }
    for line in &now {
        assert!(
            !retired(key(line)),
            "retired but produced again (codes are never reused): {line}"
        );
        assert!(
            frozen.contains(&key(line)),
            "append this line to the fixture: {line}"
        );
    }
    for k in &frozen {
        assert!(
            now.iter().any(|l| key(l) == *k) || retired(k),
            "frozen line no longer produced; if it is gone for good, add it to RETIRED*: {k}"
        );
    }
}

fn is_token(i: &str) -> bool {
    !i.is_empty() && !i.contains([' ', '#'])
}

/// Idents are one token, a param's is its own within its block, and so is
/// each value's within its param.
#[test]
fn idents_are_tokens_and_unique() {
    for (b, _) in stored() {
        let mut seen: Vec<&str> = Vec::new();
        let mut bank = Bank::new();
        for a in ValidAddr::of_block(b) {
            let s = a.spec();
            assert!(is_token(s.ident), "{b:?} {:?}", s.ident);
            assert!(!seen.contains(&s.ident), "{b:?} repeats {}", s.ident);
            seen.push(s.ident);
            if !a.coded() {
                continue;
            }
            let mut values: Vec<&str> = Vec::new();
            for v in 0..=s.max as u8 {
                let blk = bank.block(b);
                blk.set(s.id, f32::from(v));
                let i = blk.enum_ident(s.id).expect("an Enum has an ident");
                assert!(is_token(i), "{b:?} {} value {v}: {i:?}", s.ident);
                assert!(!values.contains(&i), "{b:?} {} repeats {i}", s.ident);
                values.push(i);
            }
        }
    }
}

/// WAVE and ALG's idents are their own literals, one per table entry, not the
/// label tables the UI shows.
#[test]
fn wave_and_algo_idents_are_their_own_tables() {
    assert_eq!(WAVE_IDENTS.len(), WAVE_NAMES.len());
    assert_eq!(ALGO_IDENTS.len(), ALGO_NAMES.len());
    let mut bank = Bank::new();
    for (i, want) in WAVE_IDENTS.iter().enumerate() {
        let blk = bank.block(BlockRef::AlgoOp(Op::A));
        blk.set(ParamId(0), i as f32);
        assert_eq!(blk.enum_ident(ParamId(0)), Some(*want));
    }
    for (i, want) in ALGO_IDENTS.iter().enumerate() {
        let blk = bank.block(BlockRef::Algo);
        blk.set(ParamId(1), i as f32);
        assert_eq!(blk.enum_ident(ParamId(1)), Some(*want));
    }
}

/// A live param is a view: never stored, and everything it reads is.
#[test]
fn live_params_are_backed_by_stored_slots() {
    let backing: Vec<_> = EnvSlot::ALL
        .into_iter()
        .map(|e| {
            (
                BlockRef::Env(e),
                EnvParams::FORM,
                [
                    EnvParams::FORM_ENV,
                    EnvParams::FORM_LFO,
                    EnvParams::FORM_BURST,
                ],
            )
        })
        .collect();
    let live: Vec<(BlockRef, ParamId)> = BlockRef::ALL
        .into_iter()
        .flat_map(|b| {
            b.specs()
                .iter()
                .filter(|s| !s.stored)
                .map(move |s| (b, s.id))
        })
        .collect();
    for (b, id) in &live {
        assert!(
            backing.iter().any(|(bb, l, _)| bb == b && l == id),
            "{b:?} {id:?} is live with no declared backing"
        );
    }
    for (b, l, slots) in backing {
        assert!(ValidAddr::find(b, l).is_none());
        for slot in slots {
            assert!(
                ValidAddr::of_block(b).any(|a| a.spec().id == slot),
                "{slot:?}"
            );
        }
    }
}

/// Load `from`'s stored values into a fresh block the way a loader does: in
/// `ValidAddr::of_block` order, coded params by their code.
fn load(b: BlockRef, from: &mut Bank) -> Bank {
    let mut to = Bank::new();
    for a in ValidAddr::of_block(b) {
        let id = a.spec().id;
        match read_value(from.block(b), a) {
            DiskValue::Code(c) => {
                assert!(
                    to.block(b).set_enum_code(id, c),
                    "{b:?} {} {c}",
                    a.spec().label
                )
            }
            DiskValue::Real(v) => to.block(b).write(id, v),
        }
    }
    to
}

#[test]
fn every_enum_param_round_trips_in_loader_order() {
    for (b, _) in stored() {
        for e in ValidAddr::of_block(b).filter(|a| a.coded()) {
            let s = e.spec();
            for v in 0..=s.max as u8 {
                let mut a = Bank::new();
                a.block(b).set(s.id, f32::from(v));
                assert!(a.block(b).enum_code(s.id).is_some(), "{b:?} {}", s.label);
                let mut to = load(b, &mut a);
                for x in ValidAddr::of_block(b) {
                    let id = x.spec().id;
                    assert_eq!(
                        to.block(b).get(id),
                        a.block(b).get(id),
                        "{b:?} {} after setting {} to {v}",
                        x.spec().label,
                        s.label
                    );
                }
            }
        }
    }
}

/// A param whose decoding needs another param loaded first comes after it.
#[test]
fn dependents_follow_their_parents() {
    let deps = [(BlockRef::Filter, FilterParams::MODE, FilterParams::KIND)];
    for (b, child, parent) in deps {
        let order: Vec<ParamId> = ValidAddr::of_block(b).map(|a| a.spec().id).collect();
        let at = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(at(parent) < at(child), "{b:?}");
    }
}

#[test]
fn modulator_forms_are_stored_per_mode() {
    let env = BlockRef::Env(EnvSlot::Env1);
    // The live FORM is a view of the current MODE's slot, not stored.
    assert!(ValidAddr::find(env, EnvParams::FORM).is_none());
    let mut e = EnvParams::default();
    e.set(EnvParams::FORM_ENV, 2.0);
    e.set(EnvParams::FORM_LFO, 1.0);
    e.set(EnvParams::FORM_BURST, 1.0);
    for (mode, want) in [(0.0, 2.0), (1.0, 1.0), (2.0, 1.0)] {
        e.set(EnvParams::MODE, mode);
        assert_eq!(e.get(EnvParams::FORM), want);
    }
    e.set(EnvParams::FORM, 0.0); // MODE is Burst
    assert_eq!(e.get(EnvParams::FORM_BURST), 0.0);
    assert_eq!(e.get(EnvParams::FORM_LFO), 1.0);
}

#[test]
fn unknown_mode_code_keeps_every_form() {
    let env = BlockRef::Env(EnvSlot::Env1);
    let mut src = EnvParams::default();
    src.set(EnvParams::MODE, 1.0);
    src.set(EnvParams::FORM_ENV, 2.0);
    src.set(EnvParams::FORM_LFO, 1.0);
    src.set(EnvParams::FORM_BURST, 1.0);
    let mut dst = EnvParams::default();
    for a in ValidAddr::of_block(env).filter(|a| a.coded()) {
        let id = a.spec().id;
        let code = if id == EnvParams::MODE {
            250
        } else {
            src.enum_code(id).unwrap()
        };
        assert_eq!(dst.set_enum_code(id, code), id != EnvParams::MODE);
    }
    assert_eq!(dst.func.mode, FuncMode::Env, "MODE keeps its default");
    assert_eq!(dst.func.env_form, EnvForm::Cycle);
    assert_eq!(dst.func.lfo_form, LfoForm::Sync);
    assert_eq!(dst.func.burst_form, EnvForm::Ahr);
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
        for s in ValidAddr::of_block(b)
            .filter(|a| a.coded())
            .map(|a| a.spec())
        {
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
    assert!(RETIRED_CODES.is_empty() && RETIRED_SOURCES.is_empty() && RETIRED_BLOCKS.is_empty());
    for &(block, id) in RETIRED {
        for b in BlockRef::ALL
            .into_iter()
            .filter(|b| b.disk_code() == Some(block))
        {
            assert!(
                ValidAddr::find(b, ParamId(id)).is_none(),
                "{b:?} {id} is live"
            );
        }
    }
    for &(block, id, code) in RETIRED_CODES {
        for b in BlockRef::ALL
            .into_iter()
            .filter(|b| b.disk_code() == Some(block))
        {
            let mut bank = Bank::new();
            assert!(
                !bank.block(b).set_enum_code(ParamId(id), code),
                "{b:?} {id} {code}"
            );
        }
    }
    for &c in RETIRED_SOURCES {
        assert_eq!(ModSource::from_disk_code(c), None);
    }
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "block does not match")]
fn read_value_checks_the_block() {
    let a = ValidAddr::find(BlockRef::Drive, ParamId(0)).unwrap();
    read_value(&FilterParams::default(), a);
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
