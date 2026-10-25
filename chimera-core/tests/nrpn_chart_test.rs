//! The NRPN address space (ADR 0216): derived from the frozen disk identity,
//! and pinned by `fixtures/nrpn_v1.txt`, which only ever grows.

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::addr::{BlockViewMut, Blocks};
use chimera_core::block::{Block, DiskCode, ParamId};
use chimera_core::dsp::filter::{FilterKind, FilterMode};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::midi_map::Mappable;
use chimera_core::nrpn::{self, ChartLine, NrpnAddr};
use chimera_core::orbit::scene::Scene;
use chimera_core::params::FilterParams;
use chimera_core::params::ParamSnapshot;
use chimera_core::part::PartParams;
use chimera_core::storage::{RETIRED, ValidAddr};
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_core::ui::theme_settings::ThemeSettings;
use chimera_midi::law::ValueLaw;
use chimera_midi::value::{U7, U14};

const FIXTURE: &str = "tests/fixtures/nrpn_v1.txt";

fn chart() -> Vec<String> {
    nrpn::chart().map(|l| l.to_string()).collect()
}

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

fn wire(msb: u8, lsb: u8) -> NrpnAddr {
    NrpnAddr::new(U14::from_parts(u7(msb), u7(lsb)))
}

/// Every address any `ParamAddr` can name: each block's params, stored or not.
fn every_addr() -> impl Iterator<Item = ParamAddr> {
    BlockRef::ALL
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
}

/// `msb lsb` of a chart line, `N` or `X`.
fn place(line: &str) -> (&str, &str) {
    let mut t = line.split(' ').skip(1);
    (t.next().unwrap(), t.next().unwrap())
}

/// Spec § 5: a line never changes or disappears, and keeps its place among
/// the others; new lines may land anywhere (the chart is block-ordered, so a
/// new param of an old block lands mid-chart). A param that retires leaves
/// its `N` line only for an `X` line at the same address.
#[test]
fn nrpn_chart_is_append_only() {
    let golden =
        std::fs::read_to_string(FIXTURE).expect("the fixture: run write_nrpn_fixture once");
    let now = chart();
    let mut at = 0;
    for line in golden.lines() {
        match now[at..].iter().position(|l| l == line) {
            Some(i) => at += i + 1,
            None if line.starts_with("N ") => {
                let (m, l) = place(line);
                assert!(
                    now.contains(&format!("X {m} {l}")),
                    "a frozen NRPN line changed, moved or disappeared (a change of law is a \
                     renumber; a retired param goes on storage::RETIRED): {line}"
                );
            }
            None => panic!("a frozen NRPN line moved or disappeared: {line}"),
        }
    }
    for line in &now {
        assert!(
            golden.lines().any(|g| g == line),
            "append this line to {FIXTURE} (or rerun write_nrpn_fixture and check the diff only adds): {line}"
        );
    }
}

#[test]
fn every_mappable_has_an_address() {
    for a in every_addr() {
        assert_eq!(nrpn::writable(a), Mappable::new(a), "{a:?}");
        if Mappable::new(a).is_some() {
            assert!(
                NrpnAddr::of(a).is_some(),
                "{a:?} is mappable but has no address"
            );
        }
    }
}

#[test]
fn addresses_round_trip() {
    let mut n = 0;
    for b in BlockRef::ALL {
        for v in ValidAddr::of_block(b) {
            let a = v.addr();
            let x = NrpnAddr::of(a).expect("a stored param has an address");
            assert_eq!(x.get().msb().get(), b.disk_code().unwrap());
            assert_eq!(x.get().lsb().get(), a.param.0);
            assert_eq!(x.addr(), Some(a));
            n += 1;
        }
    }
    assert!(n > 300, "{n} stored params");
    // And from the wire: whatever names a param names it back.
    for v in 0..=16383 {
        let x = NrpnAddr::new(U14::new(v).unwrap());
        if let Some(a) = x.addr() {
            assert_eq!(NrpnAddr::of(a), Some(x), "{v}");
        }
    }
    // A block that is never stored has no address.
    let a = ParamAddr::new(BlockRef::Tempo, ParamId(0));
    assert_eq!(NrpnAddr::of(a), None);
    // Nor does a live view of stored params.
    for a in every_addr() {
        if a.spec().is_some_and(|s| !s.stored) {
            assert_eq!(NrpnAddr::of(a), None, "{a:?}");
        }
    }
}

#[test]
fn retired_never_reused() {
    let now = chart();
    for &(b, id) in RETIRED {
        assert_eq!(wire(b, id).addr(), None, "{b}:{id}");
        assert!(
            now.contains(&format!("X {b} {id}")),
            "{b}:{id} has no X line"
        );
        assert!(
            !now.iter().any(|l| l.starts_with(&format!("N {b} {id} "))),
            "{b}:{id} is retired but named again"
        );
    }
    let mut seen = std::collections::HashSet::new();
    for l in &now {
        assert!(seen.insert(place(l)), "address repeated: {l}");
    }
}

#[test]
fn msb_below_64_lsb_below_128() {
    for l in nrpn::chart() {
        let a = l.addr().get();
        assert!(a.msb().get() < 64, "{l}");
        assert!(a.lsb().get() < 128, "{l}");
    }
    // 127/127 is the null, and MSB 64.. is reserved: nothing there.
    for v in 64 << 7..=16383 {
        assert_eq!(NrpnAddr::new(U14::new(v).unwrap()).addr(), None);
    }
}

fn law_of(b: BlockRef, id: ParamId) -> ValueLaw {
    nrpn::law(b, ParamAddr::new(b, id).spec().unwrap())
}

#[test]
fn law_of_cutoff_is_place14() {
    assert_eq!(
        law_of(BlockRef::Filter, FilterParams::CUTOFF),
        ValueLaw::Place14
    );
    assert!(chart().contains(&"N 10 0 FILTER CUTOFF Place14 W".to_string()));
}

#[test]
fn law_of_filter_type_is_code7() {
    assert_eq!(
        law_of(BlockRef::Filter, FilterParams::KIND),
        ValueLaw::Code7
    );
    assert!(chart().contains(&"N 10 6 FILTER KIND Code7 W".to_string()));
}

#[test]
fn law_of_tune_is_index14() {
    assert_eq!(
        law_of(BlockRef::Midi, MidiSettings::TUNE),
        ValueLaw::Index14
    );
    assert!(chart().contains(&"N 33 1 MIDI TUNE Index14 R".to_string()));
    // PART LEVEL is stepped, ≤ 128 values: Index7, writable.
    assert!(
        chart()
            .iter()
            .any(|l| l.starts_with("N 25 3 PART LEVEL ") && l.ends_with(" W"))
    );
}

/// Code7's lookup: a frozen code to the index the live block shows it at,
/// read through the block's own decoder, the block left as it was.
#[test]
fn code7_lookup_follows_the_live_block() {
    let mut f = FilterParams::default();
    let before: Vec<f32> = f.specs().iter().map(|s| f.get(s.id)).collect();
    for k in FilterKind::BUILT {
        let i = nrpn::index_of_code(&mut f, FilterParams::KIND, u7(k.disk_code()));
        assert_eq!(
            i,
            Some(FilterKind::BUILT.iter().position(|x| *x == k).unwrap() as u16)
        );
    }
    let after: Vec<f32> = f.specs().iter().map(|s| f.get(s.id)).collect();
    assert_eq!(before, after, "the probe left the block changed");
    assert_eq!(
        nrpn::index_of_code(&mut f, FilterParams::KIND, u7(127)),
        None
    );

    // MODE is read through the live KIND: the same code, another index.
    for k in FilterKind::BUILT {
        assert!(f.set_enum_code(FilterParams::KIND, k.disk_code()));
        let before: Vec<f32> = f.specs().iter().map(|s| f.get(s.id)).collect();
        for (i, _) in k.mode_labels().iter().enumerate() {
            let mut g = f;
            g.set(FilterParams::MODE, i as f32);
            let c = g.enum_code(FilterParams::MODE).unwrap();
            assert_eq!(
                nrpn::index_of_code(&mut f, FilterParams::MODE, u7(c)),
                Some(i as u16),
                "{k:?} mode {i}"
            );
        }
        let after: Vec<f32> = f.specs().iter().map(|s| f.get(s.id)).collect();
        assert_eq!(before, after, "{k:?}: the probe left the block changed");
        // A mode this KIND lacks names nothing.
        for m in FilterMode::ALL {
            if !k.modes().contains(&m) {
                assert_eq!(
                    nrpn::index_of_code(&mut f, FilterParams::MODE, u7(m.disk_code())),
                    None
                );
            }
        }
    }
}

/// One of each block's owner, so any stored `BlockRef` resolves (as
/// `disk_codes_test`'s).
struct Bank {
    snap: ParamSnapshot,
    fx: FxParams,
    part: PartParams,
    theme: ThemeSettings,
    midi: MidiSettings,
    scene: Scene,
}

impl Bank {
    fn new() -> Self {
        Self {
            snap: ParamSnapshot::default(),
            fx: FxParams::default(),
            part: PartParams::default(),
            theme: ThemeSettings::DEFAULT,
            midi: MidiSettings::DEFAULT,
            scene: Scene::init(),
        }
    }

    fn block(&mut self, b: BlockRef) -> BlockViewMut<'_> {
        BlockViewMut::Plain(match b {
            BlockRef::Chorus => &mut self.fx.chorus,
            BlockRef::Delay => &mut self.fx.delay,
            BlockRef::Reverb => &mut self.fx.reverb,
            BlockRef::Tape => &mut self.fx.tape,
            BlockRef::Comp => &mut self.fx.comp,
            BlockRef::Part => &mut self.part,
            BlockRef::Theme => &mut self.theme,
            BlockRef::Midi => &mut self.midi,
            BlockRef::OrbitChords => &mut self.scene.chords,
            BlockRef::OrbitRing(r) => &mut self.scene.rings[r.get() as usize],
            b => return self.snap.block_mut(b).expect("a Sound block"),
        })
    }
}

/// A choice is `Code7` exactly when every code it stores fits an MSB;
/// `WIDE_CODES` lists the others and nothing else.
#[test]
fn code7_codes_fit_an_msb() {
    for b in BlockRef::ALL
        .into_iter()
        .filter(|b| b.disk_code().is_some())
    {
        let mut bank = Bank::new();
        for v in ValidAddr::of_block(b).filter(|v| v.coded()) {
            let s = v.spec();
            let wide = (0..=s.max as u8).any(|i| {
                let mut blk = bank.block(b);
                blk.set(s.id, f32::from(i));
                blk.enum_code(s.id).unwrap() > 127
            });
            bank.block(b).set(s.id, s.default);
            let law = nrpn::law(b, s);
            assert_eq!(law == ValueLaw::Code7, !wide, "{b:?} {}", s.ident);
            assert_eq!(
                nrpn::WIDE_CODES.contains(&(b, s.id)),
                wide,
                "{b:?} {}",
                s.ident
            );
        }
    }
    assert_eq!(
        law_of(BlockRef::Theme, ThemeSettings::BLACK),
        ValueLaw::Index7
    );
}

fn gets(b: &dyn Block) -> Vec<f32> {
    b.specs().iter().map(|s| b.get(s.id)).collect()
}

/// The probe leaves every block as it was, from a state off the defaults
/// (each param near the top of its range, TAPE SLICE at S3), whatever code
/// it tries on whichever choice.
#[test]
fn code7_probe_never_changes_the_block() {
    use chimera_core::dsp::tape_engine::params::TapeEngineParams;
    for b in BlockRef::ALL
        .into_iter()
        .filter(|b| b.disk_code().is_some())
    {
        let mut bank = Bank::new();
        let coded: Vec<_> = ValidAddr::of_block(b).filter(|v| v.coded()).collect();
        for v in ValidAddr::of_block(b) {
            let s = v.spec();
            bank.block(b).set(s.id, s.min + (s.max - s.min) * 0.7);
        }
        if b == BlockRef::TapeEngine {
            bank.block(b).write(TapeEngineParams::SLICE, 3.0);
            assert_eq!(bank.block(b).get(TapeEngineParams::SLICE), 3.0);
        }
        let before = gets(&*bank.block(b));
        for v in &coded {
            for c in 0..=127 {
                let mut blk = bank.block(b);
                nrpn::index_of_code(&mut *blk, v.spec().id, u7(c));
                assert_eq!(gets(&*blk), before, "{b:?} {} code {c}", v.spec().ident);
            }
        }
    }
}

/// Labelled lines for the docs: `chimera-utils nrpn-chart` prints the same.
#[test]
fn labelled_lines_name_the_range() {
    let l = nrpn::chart()
        .find(|l| matches!(l, ChartLine::Live { .. }) && l.to_string().starts_with("N 33 1 "))
        .unwrap();
    let s = l.labelled().to_string();
    assert!(s.starts_with("N 33 1 MIDI TUNE Index14 R"), "{s}");
    assert!(s.contains("-100..100"), "{s}");
}

/// Writes the fixture from today's chart: once, to create it, and after an
/// append (check the diff only adds lines).
#[test]
#[ignore]
fn write_nrpn_fixture() {
    let mut s = chart().join("\n");
    s.push('\n');
    std::fs::write(FIXTURE, s).unwrap();
}
