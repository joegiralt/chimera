//! Each reverb TYPE's panel (reverb TYPEs spec § Pages, § Each TYPE's
//! knobs): REV's knobs 1–4, REV › MORE, their words, values and steps.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::rev::sync::{FoldState, GATE_LENGTH, Tempo};
use chimera_core::dsp::reverb::{KnobMemory, REVERB_SPECS, RevSync, ReverbParams, ReverbType};
use chimera_core::params::ParamSnapshot;
use chimera_core::preset::Performance;
use chimera_core::project::PartId;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::reverb_panel::{
    RevPage, knob, label, retype, step, turn, types_on_screen, value_text,
};
use chimera_core::ui::view::{SlotCtx, View, view};
use chimera_core::ui::{components, draw, hint, part_page, theme};

const ALL: [ReverbType; 6] = ReverbType::ALL;

fn words(t: ReverbType, page: RevPage, n: u8) -> Vec<Option<&'static str>> {
    (0..n).map(|k| knob(t, page, k).map(|c| c.label)).collect()
}

fn ctx(t: ReverbType) -> SlotCtx {
    SlotCtx {
        rev: t,
        ..SlotCtx::read(&ParamSnapshot::default(), Op::A)
    }
}

#[test]
fn every_type_shows_its_cells() {
    use ReverbType::*;
    let main = |t| words(t, RevPage::Main, 4);
    let more = |t| words(t, RevPage::More, 6);
    let s = |w: &[&'static str]| {
        w.iter()
            .map(|&x| (x != "-").then_some(x))
            .collect::<Vec<_>>()
    };
    assert_eq!(main(Quad), s(&["SIZE", "TIME", "DAMP", "SMEAR"]));
    assert_eq!(more(Quad), s(&["-", "-", "-", "RATE", "-", "-"]));
    assert_eq!(main(Room), s(&["SIZE", "DECAY", "TONE", "DIFFUSE"]));
    assert_eq!(
        more(Room),
        s(&["PRE-DLY", "-", "SMEAR", "RATE", "SYNC", "-"])
    );
    assert_eq!(main(Hall), s(&["SIZE", "DECAY", "TONE", "DIFFUSE"]));
    assert_eq!(
        more(Hall),
        s(&["PRE-DLY", "DAMP", "SMEAR", "RATE", "SYNC", "-"])
    );
    assert_eq!(main(Gate), s(&["LENGTH", "TAIL", "TONE", "DENSITY"]));
    assert_eq!(more(Gate), s(&["-", "SHAPE", "SMEAR", "RATE", "SYNC", "-"]));
    assert_eq!(main(Reverse), s(&["LENGTH", "REGEN", "TONE", "DENSITY"]));
    assert_eq!(
        more(Reverse),
        s(&["-", "CURVE", "SMEAR", "RATE", "SYNC", "-"])
    );
    assert_eq!(main(Bloom), s(&["SWELL", "DECAY", "TONE", "DIFFUSE"]));
    assert_eq!(
        more(Bloom),
        s(&["PRE-DLY", "-", "SMEAR", "RATE", "SYNC", "-"])
    );
    // Each label sits on its slot: a cell's id is the spec table's.
    let id = |t, page, k| knob(t, page, k).unwrap().id;
    assert_eq!(id(Quad, RevPage::Main, 0), ReverbParams::SIZE);
    assert_eq!(id(Quad, RevPage::Main, 3), ReverbParams::SMEAR);
    assert_eq!(id(Gate, RevPage::Main, 1), ReverbParams::TIME);
    assert_eq!(id(Hall, RevPage::More, 1), ReverbParams::DAMPING);
    assert_eq!(id(Reverse, RevPage::More, 1), ReverbParams::SHAPE);
    assert_eq!(id(Bloom, RevPage::More, 4), ReverbParams::SYNC);
    // REV: TYPE, the panel's four, then MIX on every TYPE.
    for t in ALL {
        let v = |def: &BlockDef, i| view(def, i, &ctx(t));
        assert_eq!(v(&reg::EFX, 0).addr(), Some(addr(ReverbParams::TYPE)));
        assert_eq!(v(&reg::EFX, 5).addr(), Some(addr(ReverbParams::MIX)));
        for k in 0..4 {
            assert_eq!(
                v(&reg::EFX, k + 1).addr().map(|a| a.param),
                knob(t, RevPage::Main, k as u8).map(|c| c.id)
            );
        }
        for k in 0..6 {
            let want = knob(t, RevPage::More, k as u8);
            assert_eq!(v(&reg::EFX_MORE, k).label(), want.map_or("--", |c| c.label));
            assert_eq!(want.is_none(), v(&reg::EFX_MORE, k) == View::Empty);
        }
    }
}

fn addr(id: ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::Reverb, id)
}

#[test]
fn room_sync_steps_off_and_time_only() {
    let mut mem = KnobMemory::new();
    let r = ReverbParams::default().with_type(ReverbType::Room, &mem);
    let a = turn(&r, ReverbParams::SYNC, 1, Tempo::DEFAULT, &mut mem);
    assert_eq!(a.sync, RevSync::Time);
    let b = turn(&a, ReverbParams::SYNC, 1, Tempo::DEFAULT, &mut mem);
    assert_eq!(b.sync, RevSync::Time, "ROOM has no ALL");
    let c = turn(&b, ReverbParams::SYNC, -1, Tempo::DEFAULT, &mut mem);
    assert_eq!(c.sync, RevSync::Off);
    let mut stored = r;
    stored.sync = RevSync::All;
    let text = value_text(
        ReverbType::Room,
        ReverbParams::SYNC,
        &stored,
        Tempo::DEFAULT,
        FoldState(0),
    );
    assert_eq!(text.as_str(), "TIME");
    // HALL walks all three.
    let h = ReverbParams::default().with_type(ReverbType::Hall, &mem);
    let h = turn(&h, ReverbParams::SYNC, 2, Tempo::DEFAULT, &mut mem);
    assert_eq!(h.sync, RevSync::All);
}

/// Every word and value a reverb cell can show, under every TYPE, SYNC and
/// a slow and a fast tempo.
fn every_text() -> Vec<(ReverbType, ParamId, String)> {
    let mut out = Vec::new();
    let bpm = |b: f32| Tempo(Tempo::RATE * 60.0 / b);
    for t in ALL {
        for sync in [RevSync::Off, RevSync::Time, RevSync::All] {
            for tempo in [bpm(30.0), Tempo::DEFAULT, bpm(90.0), bpm(300.0)] {
                for page in [RevPage::Main, RevPage::More] {
                    for k in 0..6 {
                        let Some(c) = knob(t, page, k) else { continue };
                        for i in 0..=128 {
                            let mut p = ReverbParams::default().with_type(t, &KnobMemory::new());
                            p.sync = sync;
                            p.write(c.id, i as f32 / 128.0);
                            if c.id == ReverbParams::SYNC {
                                p.write(c.id, (i % 3) as f32);
                            }
                            let s = value_text(t, c.id, &p, tempo, FoldState(0));
                            out.push((t, c.id, s.as_str().to_string()));
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn labels_and_values_fit_their_cells() {
    // A label over 6 letters, or wider than the bar, has a short form
    // that is neither (ADR 0053's rule).
    for t in ALL {
        for page in [RevPage::Main, RevPage::More] {
            for k in 0..6 {
                let Some(c) = knob(t, page, k) else { continue };
                let w = |s| draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING);
                let shown = match c.short {
                    Some(s) if w(c.label) > theme::CELL_BAR_W => s,
                    _ => c.label,
                };
                assert!(w(shown) <= theme::CELL_BAR_W, "{t:?} {}", c.label);
                if c.label.len() > 6 {
                    let s = c
                        .short
                        .unwrap_or_else(|| panic!("{} has no short", c.label));
                    assert!(s.len() <= 6, "{s}");
                }
            }
        }
    }
    let texts = every_text();
    let room = theme::CUBE_X - 4 - theme::FOCUS_VALUE_X;
    for (t, id, s) in &texts {
        let cell = draw::text_width(components::cell_font(s), s, 0);
        assert!(cell <= theme::CELL_VALUE_W, "{t:?} {id:?} {s}: {cell} px");
        let focus = components::focus_font(s, room);
        let fw = draw::text_width(focus, s, 0);
        assert!(fw <= room, "{t:?} {id:?} {s}: {fw} of {room} px");
    }
    let has = |want: &str| texts.iter().any(|(_, _, s)| s == want);
    for want in [
        "▾1/8",
        "PLATE 120ms",
        "OFF",
        "TIME",
        "141ms",
        "0.55s",
        "6.6k",
    ] {
        assert!(has(want), "no cell shows {want}");
    }
    let shorts: Vec<_> = ALL
        .iter()
        .flat_map(|&t| {
            (0..6).flat_map(move |k| [knob(t, RevPage::Main, k), knob(t, RevPage::More, k)])
        })
        .flatten()
        .filter_map(|c| c.short)
        .collect();
    for short in ["PRE", "DENS", "DIFF"] {
        assert!(shorts.contains(&short), "{short}");
    }
}

/// The ▾ of a fold, from the spec's example: GATE's 1/4 at 90 BPM.
#[test]
fn a_folded_division_shows_its_mark() {
    let mut p = ReverbParams::default().with_type(ReverbType::Gate, &KnobMemory::new());
    p.sync = RevSync::Time;
    p.size = 1.0; // GATE_LENGTH's last: 1/4
    assert_eq!(GATE_LENGTH.len(), 11);
    let slow = Tempo(Tempo::RATE * 60.0 / 90.0);
    let text = value_text(ReverbType::Gate, ReverbParams::SIZE, &p, slow, FoldState(0));
    assert_eq!(text.as_str(), "▾1/8");
    let text = value_text(
        ReverbType::Gate,
        ReverbParams::SIZE,
        &p,
        Tempo::DEFAULT,
        FoldState(0),
    );
    assert_eq!(text.as_str(), "1/4");
}

#[test]
fn no_reverb_cell_has_a_hint() {
    for t in ALL {
        for def in [&reg::EFX, &reg::EFX_MORE] {
            for k in 0..6 {
                assert!(view(def, k, &ctx(t)).hint().is_empty(), "{t:?} {k}");
                assert_eq!(view(def, k, &ctx(t)).hint(), hint::NONE);
            }
        }
    }
}

#[test]
fn label_follows_the_type() {
    assert_eq!(label(ReverbType::Room, ParamId(3)), "SIZE");
    assert_eq!(label(ReverbType::Gate, ParamId(3)), "LENGTH");
    assert_eq!(label(ReverbType::Reverse, ParamId(1)), "REGEN");
    assert_eq!(label(ReverbType::Quad, ParamId(6)), "TYPE");
    assert_eq!(label(ReverbType::Quad, ParamId(4)), "MIX");
    // A slot the TYPE lacks still names its spec.
    assert_eq!(label(ReverbType::Quad, ParamId(7)), "TONE");
}

#[test]
fn quad_size_still_ticks_by_one_table_step() {
    let spec = REVERB_SPECS
        .iter()
        .find(|s| s.id == ReverbParams::SIZE)
        .unwrap();
    assert_eq!(spec.step, 1.0 / 128.0);
    assert_eq!(
        knob(ReverbType::Quad, RevPage::Main, 0).unwrap().step,
        1.0 / 31.0
    );
    let mut perf = Performance::new();
    part_page::apply_encoder(&reg::EFX, 1, 1, &mut perf.edit(PartId::ALL[0]), &mut Op::A);
    assert!((perf.fx.reverb.size - (0.5 + 1.0 / 31.0)).abs() < 1e-6);
    // Through the panel's turn too.
    let p = ReverbParams::default();
    let q = turn(
        &p,
        ReverbParams::SIZE,
        1,
        Tempo::DEFAULT,
        &mut KnobMemory::new(),
    );
    assert!((q.size - (0.5 + 1.0 / 31.0)).abs() < 1e-6);
}

#[test]
fn a_synced_knob_steps_one_division() {
    let mut p = ReverbParams::default().with_type(ReverbType::Gate, &KnobMemory::new());
    assert_eq!(step(&p, ReverbParams::SIZE), 1.0 / 128.0);
    p.sync = RevSync::Time;
    assert_eq!(step(&p, ReverbParams::SIZE), 1.0 / 10.0);
    let mut h = ReverbParams::default().with_type(ReverbType::Hall, &KnobMemory::new());
    h.sync = RevSync::All;
    assert_eq!(step(&h, ReverbParams::TIME), 1.0 / 9.0);
    assert_eq!(step(&h, ReverbParams::PREDELAY), 1.0 / 8.0);
}

/// An accidental TYPE nudge does not wipe the tweaks: leaving a TYPE
/// keeps its knobs, coming back restores them.
#[test]
fn a_type_nudge_keeps_the_tweaks() {
    let mut mem = KnobMemory::new();
    let mut r = ReverbParams::default().with_type(ReverbType::Room, &mem);
    (r.size, r.tone, r.mix) = (0.9, 0.8, 0.6);
    let h = retype(&r, ReverbType::Hall, &mut mem);
    assert_eq!(h.size, 0.65, "Hall's starting SIZE");
    let back = retype(&h, ReverbType::Room, &mut mem);
    assert!((back.size - 0.9).abs() <= 1.0 / 65_534.0);
    assert!((back.tone - 0.8).abs() <= 1.0 / 65_534.0);
    assert_eq!(back.mix, 0.6);
    // Only QUAD is on screen yet: TYPE stays put.
    assert_eq!(types_on_screen(), &[ReverbType::Quad]);
    let q = ReverbParams::default();
    assert_eq!(turn(&q, ReverbParams::TYPE, 1, Tempo::DEFAULT, &mut mem), q);
}
