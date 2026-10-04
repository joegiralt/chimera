//! Each reverb TYPE's panel (reverb TYPEs spec § Pages, § Each TYPE's
//! knobs): REV's knobs 1–4, REV › MORE, their words, values and steps.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::reverb::{KnobMemory, REVERB_SPECS, ReverbParams, ReverbType};
use chimera_core::params::ParamSnapshot;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::reverb_panel::{
    RevPage, knob, label, retype, shown, turn, types_on_screen, value_text,
};
use chimera_core::ui::view::{SlotCtx, View, view};
use chimera_core::ui::{draw, hint, theme};

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
    assert_eq!(main(Gate), s(&["LENGTH", "TAIL", "TONE", "DENSITY"]));
    assert_eq!(more(Gate), s(&["-", "SHAPE", "SMEAR", "RATE", "-", "-"]));
    assert_eq!(main(Reverse), s(&["LENGTH", "REGEN", "TONE", "DENSITY"]));
    assert_eq!(more(Reverse), s(&["-", "CURVE", "SMEAR", "RATE", "-", "-"]));
    // ROOM's, HALL's and BLOOM's land with their engines (Tasks 9–11):
    // until then each is QUAD's, which is what plays.
    for t in [Room, Hall, Bloom] {
        assert_eq!((main(t), more(t)), (main(Quad), more(Quad)), "{t:?}");
    }
    // Each label sits on its slot: a cell's id is the spec table's.
    let id = |t, page, k| knob(t, page, k).unwrap().id;
    assert_eq!(id(Quad, RevPage::Main, 0), ReverbParams::SIZE);
    assert_eq!(id(Quad, RevPage::Main, 3), ReverbParams::SMEAR);
    assert_eq!(id(Quad, RevPage::More, 3), ReverbParams::RATE);
    assert_eq!(id(Gate, RevPage::Main, 0), ReverbParams::SIZE);
    assert_eq!(id(Gate, RevPage::Main, 1), ReverbParams::TIME);
    assert_eq!(id(Reverse, RevPage::Main, 3), ReverbParams::DIFFUSE);
    assert_eq!(id(Reverse, RevPage::More, 1), ReverbParams::SHAPE);
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
    assert_eq!(label(ReverbType::Gate, ParamId(3)), "LENGTH");
    assert_eq!(label(ReverbType::Gate, ParamId(1)), "TAIL");
    assert_eq!(label(ReverbType::Reverse, ParamId(1)), "REGEN");
    assert_eq!(label(ReverbType::Reverse, ParamId(10)), "CURVE");
    assert_eq!(label(ReverbType::Gate, ParamId(8)), "DENSITY");
    assert_eq!(label(ReverbType::Quad, ParamId(3)), "SIZE");
    assert_eq!(label(ReverbType::Quad, ParamId(1)), "TIME");
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
    let p = ReverbParams::default();
    let q = turn(&p, ReverbParams::SIZE, 1, &mut KnobMemory::new());
    assert!((q.size - (0.5 + 1.0 / 31.0)).abs() < 1e-6);
}

/// Every word on every panel fits its bar, or has a short of ≤ 6 that
/// does (ADR 0053); every value QUAD's panel can show fits its cell and,
/// large, the focus band beside the cube.
#[test]
fn labels_and_values_fit_their_cells() {
    let w = |s: &str| draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING);
    for t in ALL {
        for (page, n) in [(RevPage::Main, 4), (RevPage::More, 6)] {
            for k in 0..n {
                let Some(c) = knob(t, page, k) else { continue };
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
    let room = theme::CUBE_X - 4 - theme::FOCUS_VALUE_X;
    let mut seen = Vec::new();
    for &t in types_on_screen() {
        for (page, n) in [(RevPage::Main, 4), (RevPage::More, 6)] {
            for k in 0..n {
                let Some(c) = knob(t, page, k) else { continue };
                for size in [0.0, 0.5, 1.0] {
                    for i in 0..=128 {
                        let mut p = ReverbParams {
                            kind: t,
                            size,
                            ..ReverbParams::default()
                        };
                        p.write(c.id, i as f32 / 128.0);
                        let s = value_text(c.id, &p).as_str().to_string();
                        let cell = draw::text_width(&theme::FONT_VALUE, &s, 0);
                        assert!(
                            cell <= theme::CELL_BAR_W,
                            "{t:?} {} {s}: {cell} px",
                            c.label
                        );
                        let focus = draw::text_width(&theme::FONT_FOCUS, &s, 0);
                        assert!(focus <= room, "{t:?} {} {s}: {focus} of {room} px", c.label);
                        seen.push(s);
                    }
                }
            }
        }
    }
    for want in [
        "663ms", "1.9s", "6.1k", "OFF", "0.22Hz", "40ms", "800ms", "0.80s", "2.5k", "12.0",
        "-18dB", "30dB", "2.00Hz", "1.0ms", "16s",
    ] {
        assert!(seen.iter().any(|s| s == want), "no cell shows {want}");
    }
}

/// Until its engine lands, a TYPE a CC or a newer project set shows
/// QUAD's panel, which is what plays; its code stays stored.
#[test]
fn an_off_screen_type_shows_quads_panel() {
    use chimera_core::addr::BlockRead;
    use chimera_core::block::Block as _;
    struct One(ReverbParams);
    impl BlockRead for One {
        fn block(&self, b: BlockRef) -> Option<&dyn chimera_core::block::Block> {
            (b == BlockRef::Reverb).then_some(&self.0 as &dyn chimera_core::block::Block)
        }
    }
    for t in ALL {
        let want = if types_on_screen().contains(&t) {
            t
        } else {
            ReverbType::Quad
        };
        assert_eq!(shown(t), want);
        let mut p = ReverbParams::default();
        p.write(ReverbParams::TYPE, t.index() as f32);
        assert_eq!(SlotCtx::read(&One(p), Op::A).rev, want);
        assert_eq!(
            SlotCtx::read(&ParamSnapshot::default(), Op::A)
                .on_rev(t)
                .rev,
            want
        );
    }
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
    // QUAD, GATE and REVERSE are on screen, in screen order: TYPE walks
    // them and stops at either end.
    assert_eq!(
        types_on_screen(),
        &[ReverbType::Quad, ReverbType::Gate, ReverbType::Reverse]
    );
    let q = ReverbParams::default();
    assert_eq!(turn(&q, ReverbParams::TYPE, -1, &mut mem), q);
    let g = turn(&q, ReverbParams::TYPE, 1, &mut mem);
    assert_eq!(g.kind, ReverbType::Gate);
    assert_eq!((g.size, g.time), (0.55, 0.3), "GATE's starting knobs");
    let r = turn(&g, ReverbParams::TYPE, 5, &mut mem);
    assert_eq!(r.kind, ReverbType::Reverse);
    assert_eq!(
        turn(&r, ReverbParams::TYPE, 1, &mut mem).kind,
        ReverbType::Reverse
    );
    // A TYPE off the list stays put, its knobs too.
    let h = ReverbParams::default().with_type(ReverbType::Hall, &KnobMemory::new());
    assert_eq!(turn(&h, ReverbParams::TYPE, -1, &mut mem), h);
}

/// GATE's and REVERSE's starting knobs, as their cells read (spec § Each
/// TYPE's knobs).
#[test]
fn burst_cells_read_their_starting_values() {
    let shows = |t: ReverbType, want: [(ParamId, &str); 7]| {
        let p = ReverbParams::default().with_type(t, &KnobMemory::new());
        for (id, w) in want {
            assert_eq!(value_text(id, &p).as_str(), w, "{t:?} {id:?}");
        }
    };
    use ReverbParams as P;
    shows(
        ReverbType::Gate,
        [
            (P::SIZE, "177ms"),
            (P::TIME, "0.25s"),
            (P::TONE, "7.6k"),
            (P::DIFFUSE, "10.2"),
            (P::SHAPE, "-4dB"),
            (P::SMEAR, "OFF"),
            (P::RATE, "0.45Hz"),
        ],
    );
    shows(
        ReverbType::Reverse,
        [
            (P::SIZE, "346ms"),
            (P::TIME, "OFF"),
            (P::TONE, "10k"),
            (P::DIFFUSE, "10.8"),
            (P::SHAPE, "24dB"),
            (P::SMEAR, "OFF"),
            (P::RATE, "0.45Hz"),
        ],
    );
    // TAIL 0 is the fast gate.
    let mut g = ReverbParams::default().with_type(ReverbType::Gate, &KnobMemory::new());
    g.time = 0.0;
    assert_eq!(value_text(P::TIME, &g).as_str(), "OFF");
}
