//! Each reverb TYPE's panel (reverb TYPEs spec § Pages, § Each TYPE's
//! knobs): REV's knobs 1–4, REV › MORE, their words, values and steps.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::rev::sync::{FoldState, Tempo};
use chimera_core::dsp::reverb::{KnobMemory, REVERB_SPECS, RevSync, ReverbParams, ReverbType};
use chimera_core::params::ParamSnapshot;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::reverb_panel::{
    RevPage, knob, label, retype, shown, turn, types_on_screen, value_text,
};
use chimera_core::ui::view::{SlotCtx, View, view};
use chimera_core::ui::{draw, hint, theme};

const ALL: [ReverbType; 6] = ReverbType::ALL;

/// Slot `id`'s text at 120 BPM, unfolded.
fn text(id: ParamId, p: &ReverbParams) -> FmtBuf {
    value_text(id, p, Tempo::DEFAULT, FoldState(0))
}

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
    assert_eq!(
        more(Gate),
        s(&["PRE-DLY", "SHAPE", "SMEAR", "RATE", "SYNC", "DAMP"])
    );
    assert_eq!(main(Reverse), s(&["LENGTH", "REGEN", "TONE", "DENSITY"]));
    assert_eq!(
        more(Reverse),
        s(&["ONSET", "CURVE", "SMEAR", "RATE", "SYNC", "-"])
    );
    assert_eq!(main(Room), s(&["SIZE", "DECAY", "TONE", "DIFFUSE"]));
    assert_eq!(
        more(Room),
        s(&["PRE-DLY", "DAMP", "SMEAR", "RATE", "SYNC", "LOOPS"])
    );
    assert_eq!(main(Hall), s(&["SIZE", "DECAY", "TONE", "DIFFUSE"]));
    assert_eq!(
        more(Hall),
        s(&["PRE-DLY", "DAMP", "SMEAR", "RATE", "SYNC", "SPREAD"])
    );
    // MORE 2 is HALL's DENSITY alone.
    let more2 = |t| words(t, RevPage::More2, 6);
    assert_eq!(more2(Hall), s(&["DENSITY", "-", "-", "-", "-", "-"]));
    for t in [Quad, Room, Bloom, Gate, Reverse] {
        assert_eq!(more2(t), s(&["-"; 6]), "{t:?}");
    }
    assert_eq!(
        knob(Hall, RevPage::More, 5).unwrap().id,
        ReverbParams::SHAPE
    );
    assert_eq!(
        knob(Hall, RevPage::More2, 0).unwrap().id,
        ReverbParams::DENSITY
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
    assert_eq!(id(Quad, RevPage::More, 3), ReverbParams::RATE);
    assert_eq!(id(Gate, RevPage::Main, 0), ReverbParams::SIZE);
    assert_eq!(id(Gate, RevPage::Main, 1), ReverbParams::TIME);
    assert_eq!(id(Reverse, RevPage::Main, 3), ReverbParams::DIFFUSE);
    assert_eq!(id(Reverse, RevPage::More, 1), ReverbParams::SHAPE);
    assert_eq!(id(Room, RevPage::Main, 1), ReverbParams::TIME);
    assert_eq!(id(Room, RevPage::More, 0), ReverbParams::PREDELAY);
    assert_eq!(id(Room, RevPage::More, 1), ReverbParams::DAMPING);
    assert_eq!(id(Room, RevPage::More, 5), ReverbParams::SHAPE);
    assert_eq!(id(Hall, RevPage::More, 1), ReverbParams::DAMPING);
    assert_eq!(id(Bloom, RevPage::Main, 0), ReverbParams::SIZE);
    assert_eq!(id(Bloom, RevPage::Main, 3), ReverbParams::DIFFUSE);
    assert_eq!(id(Bloom, RevPage::More, 0), ReverbParams::PREDELAY);
    for t in [Room, Hall, Bloom, Gate, Reverse] {
        assert_eq!(id(t, RevPage::More, 4), ReverbParams::SYNC);
    }
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
                assert!(view(def, k, &ctx(t)).hint(&ctx(t)).is_empty(), "{t:?} {k}");
                assert_eq!(view(def, k, &ctx(t)).hint(&ctx(t)), hint::NONE);
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
    assert_eq!(label(ReverbType::Room, ParamId(3)), "SIZE");
    assert_eq!(label(ReverbType::Room, ParamId(1)), "DECAY");
    assert_eq!(label(ReverbType::Room, ParamId(8)), "DIFFUSE");
    assert_eq!(label(ReverbType::Room, ParamId(10)), "LOOPS");
    assert_eq!(label(ReverbType::Room, ParamId(2)), "DAMP");
    assert_eq!(label(ReverbType::Bloom, ParamId(3)), "SWELL");
    assert_eq!(label(ReverbType::Bloom, ParamId(1)), "DECAY");
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
    let q = turn(
        &p,
        ReverbParams::SIZE,
        1,
        &mut KnobMemory::new(),
        Tempo::DEFAULT,
    );
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
                for (size, sync, fold) in [
                    (0.0, RevSync::Off, 0),
                    (0.5, RevSync::Off, 0),
                    (1.0, RevSync::Off, 0),
                    (0.5, RevSync::Time, 0),
                    (0.5, RevSync::All, 1),
                    (0.5, RevSync::All, -1),
                    (0.5, RevSync::All, 2),
                ] {
                    for i in 0..=128 {
                        let mut p = ReverbParams {
                            kind: t,
                            size,
                            sync,
                            ..ReverbParams::default()
                        };
                        p.write(c.id, i as f32 / 128.0);
                        let s = value_text(c.id, &p, Tempo::DEFAULT, FoldState(fold));
                        let s = s.as_str().to_string();
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
        "663ms", "1.9s", "6.1k", "OFF", "0.10Hz", "40ms", "800ms", "0.80s", "2.5k", "12.0",
        "-18dB", "30dB", "2.00Hz", "2.4ms", "16s", "80ms", "137ms", "0.20s", "2.0s", "0.50",
        "0.75", "0.45", "0.70", "10k", "90", "50ms", "0.6ms", "0.95Hz", "10s", "120ms", "600ms",
        "11k", "1.5k", "125ms", "1.5ms", "0.45Hz", "TIME", "1/32T", "1/4", "1/2T", "1/4D", "0",
        "1/128", "1/16", "▾1/8", "▴1/16T", "▾1/256", "▾1/192", "▾3/256", "▾32 BT", "▴1 BT", "8 BT",
    ] {
        assert!(seen.iter().any(|s| s == want), "no cell shows {want}");
    }
}

/// DECAY in beats marks a count the row clamps (spec § Tempo sync): at 120
/// BPM BLOOM's 1–3 beats play its 2 s floor (▴), 4–24 as read (24 is
/// its 12 s top exactly), 32 its top (▾).
#[test]
fn clamped_decay_beats_are_marked() {
    use chimera_core::dsp::rev::sync::{DECAY_BEATS, p_of};
    let p = ReverbParams {
        sync: RevSync::All,
        ..ReverbParams::default().with_type(ReverbType::Bloom, &KnobMemory::new())
    };
    let want = [
        "▴1 BT", "▴2 BT", "▴3 BT", "4 BT", "6 BT", "8 BT", "12 BT", "16 BT", "24 BT", "▾32 BT",
    ];
    for (i, w) in want.iter().enumerate() {
        let q = ReverbParams {
            time: p_of(i, DECAY_BEATS.len()),
            ..p
        };
        assert_eq!(text(ReverbParams::TIME, &q).as_str(), *w);
    }
}

/// Every TYPE has its engine, so each shows its own panel, however it
/// was set: a CC, a project, the TYPE cell.
#[test]
fn every_type_shows_its_own_panel() {
    use chimera_core::addr::BlockRead;
    use chimera_core::block::Block as _;
    struct One(ReverbParams);
    impl BlockRead for One {
        fn block(&self, b: BlockRef) -> Option<chimera_core::addr::BlockView<'_>> {
            (b == BlockRef::Reverb).then_some(chimera_core::addr::BlockView::Plain(&self.0))
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
                .on_rev(&ReverbParams {
                    kind: t,
                    ..ReverbParams::default()
                })
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
    assert_eq!(h.size, 0.921878, "Hall's starting SIZE");
    let back = retype(&h, ReverbType::Room, &mut mem);
    assert!((back.size - 0.9).abs() <= 1.0 / 65_534.0);
    assert!((back.tone - 0.8).abs() <= 1.0 / 65_534.0);
    assert_eq!(back.mix, 0.6);
    // Every TYPE is on screen, in screen order: TYPE walks them and stops
    // at either end.
    assert_eq!(types_on_screen(), &ReverbType::ALL);
    let q = ReverbParams::default();
    assert_eq!(
        turn(&q, ReverbParams::TYPE, -1, &mut mem, Tempo::DEFAULT),
        q
    );
    let room = turn(&q, ReverbParams::TYPE, 1, &mut mem, Tempo::DEFAULT);
    assert_eq!(room.kind, ReverbType::Room);
    let hall = turn(&room, ReverbParams::TYPE, 1, &mut mem, Tempo::DEFAULT);
    assert_eq!(hall.kind, ReverbType::Hall);
    let bloom = turn(&hall, ReverbParams::TYPE, 1, &mut mem, Tempo::DEFAULT);
    assert_eq!(bloom.kind, ReverbType::Bloom);
    let g = turn(&bloom, ReverbParams::TYPE, 1, &mut mem, Tempo::DEFAULT);
    assert_eq!(g.kind, ReverbType::Gate);
    assert_eq!((g.size, g.time), (0.585, 0.633), "GATE's starting knobs");
    let r = turn(&g, ReverbParams::TYPE, 5, &mut mem, Tempo::DEFAULT);
    assert_eq!(r.kind, ReverbType::Reverse);
    assert_eq!(
        turn(&r, ReverbParams::TYPE, 1, &mut mem, Tempo::DEFAULT).kind,
        ReverbType::Reverse
    );
}

/// GATE's, REVERSE's, ROOM's and BLOOM's starting knobs, as their cells read
/// (spec § Each TYPE's knobs).
#[test]
fn cells_read_their_starting_values() {
    let shows = |t: ReverbType, want: &[(ParamId, &str)]| {
        let p = ReverbParams::default().with_type(t, &KnobMemory::new());
        for &(id, w) in want {
            assert_eq!(text(id, &p).as_str(), w, "{t:?} {id:?}");
        }
    };
    use ReverbParams as P;
    // MidiVerb II program 35's fitted knobs (ADR 0095).
    shows(
        ReverbType::Gate,
        &[
            (P::SIZE, "195ms"),
            (P::TIME, "0.43s"),
            (P::TONE, "4.6k"),
            (P::DIFFUSE, "8.0"),
            (P::SHAPE, "-22dB"),
            (P::PREDELAY, "9ms"),
            (P::DAMPING, "2.0k"),
            (P::SMEAR, "OFF"),
            (P::RATE, "0.95Hz"),
        ],
    );
    // Program 40's: ONSET on PRE-DLY's slot, DENSITY the allpasses' g.
    shows(
        ReverbType::Reverse,
        &[
            (P::SIZE, "478ms"),
            (P::TIME, "OFF"),
            (P::TONE, "8.6k"),
            (P::DIFFUSE, "0.63"),
            (P::SHAPE, "33dB"),
            (P::PREDELAY, "89ms"),
            (P::SMEAR, "OFF"),
            (P::RATE, "0.95Hz"),
        ],
    );
    let p = ReverbParams::default().with_type(ReverbType::Room, &KnobMemory::new());
    // MidiVerb II program 5's fitted knobs (ADR 0095).
    for (id, w) in [
        (P::SIZE, "117ms"),
        (P::TIME, "1.0s"),
        (P::TONE, "OFF"),
        (P::DIFFUSE, "0.45"),
        (P::PREDELAY, "4ms"),
        (P::DAMPING, "4.5k"),
        (P::SMEAR, "OFF"),
        (P::RATE, "0.95Hz"),
        (P::SHAPE, "57"),
    ] {
        assert_eq!(text(id, &p).as_str(), w, "ROOM {id:?}");
    }
    let b = ReverbParams::default().with_type(ReverbType::Bloom, &KnobMemory::new());
    // MidiVerb II program 45's fitted knobs (ADR 0095).
    for (id, w) in [
        (P::SIZE, "538ms"),
        (P::TIME, "11s"),
        (P::TONE, "3.3k"),
        (P::DIFFUSE, "0.48"),
        (P::PREDELAY, "17ms"),
        (P::SMEAR, "OFF"),
        (P::RATE, "0.95Hz"),
    ] {
        assert_eq!(text(id, &b).as_str(), w, "BLOOM {id:?}");
    }
    // DAMP's top opens the dampers; TONE's the input filter, and its next
    // step reads its cutoff.
    let open = ReverbParams { damping: 0.0, ..p };
    assert_eq!(text(P::DAMPING, &open).as_str(), "OFF");
    let open = ReverbParams { tone: 0.0, ..p };
    assert_eq!(text(P::TONE, &open).as_str(), "OFF");
    // Inside the ramp it reads the filter as it is, opening: above the
    // knob's own cutoff, falling to it at the ramp's foot.
    let khz = |tone: f32| {
        let t = text(P::TONE, &ReverbParams { tone, ..p });
        t.as_str()
            .trim_end_matches('k')
            .parse::<f32>()
            .expect("kHz")
    };
    let ramp: Vec<f32> = [0.01, 0.02, 0.03, 0.04, 0.05].map(khz).to_vec();
    assert!(
        ramp[0] > 12.0 && ramp.windows(2).all(|w| w[1] <= w[0]),
        "{ramp:?}"
    );
    assert_eq!(
        text(P::TONE, &ReverbParams { tone: 0.06, ..p }).as_str(),
        "9.2k"
    );
    let p = ReverbParams::default().with_type(ReverbType::Hall, &KnobMemory::new());
    // MidiVerb II program 23's fitted knobs (ADR 0095), SMEAR 0.
    for (id, w) in [
        (P::SIZE, "563ms"),
        (P::TIME, "2.0s"),
        (P::TONE, "12k"),
        (P::DIFFUSE, "0.49"),
        (P::DAMPING, "3.2k"),
        (P::PREDELAY, "7ms"),
        (P::SMEAR, "OFF"),
        (P::RATE, "0.95Hz"),
        (P::SHAPE, "107ms"),
        (P::DENSITY, "4.0"),
    ] {
        assert_eq!(text(id, &p).as_str(), w, "HALL {id:?}");
    }
    // In the plate zone the cells show what plays: the plate's 120 ms,
    // DIFFUSE 0.80, TONE OFF, no pre-delay, the diffuser at its shortest.
    let plate = ReverbParams { size: 0.0, ..p };
    for (id, w) in [
        (P::SIZE, "120ms"),
        (P::DIFFUSE, "0.80"),
        (P::TONE, "OFF"),
        (P::PREDELAY, "0ms"),
        (P::SHAPE, "6ms"),
    ] {
        assert_eq!(text(id, &plate).as_str(), w, "PLATE {id:?}");
    }
    // TAIL 0 is the fast gate.
    let mut g = ReverbParams::default().with_type(ReverbType::Gate, &KnobMemory::new());
    g.time = 0.0;
    assert_eq!(text(P::TIME, &g).as_str(), "OFF");
}

/// HALL's SIZE cell reads `PLATE` in the plate zone (SIZE < 0.05; ADR
/// 0085: the label, not the value), on the page and under any context;
/// MIDI MAP keeps the slot's word.
#[test]
fn hall_size_cell_reads_plate_in_its_zone() {
    use chimera_core::addr::BlockRead;
    struct One(ReverbParams);
    impl BlockRead for One {
        fn block(&self, b: BlockRef) -> Option<chimera_core::addr::BlockView<'_>> {
            (b == BlockRef::Reverb).then_some(chimera_core::addr::BlockView::Plain(&self.0))
        }
    }
    let w = |s: &str| draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING);
    assert!(w("PLATE") <= theme::CELL_BAR_W);
    for (kind, size, want) in [
        (ReverbType::Hall, 0.0, "PLATE"),
        (ReverbType::Hall, 0.049, "PLATE"),
        (ReverbType::Hall, 0.05, "SIZE"),
        (ReverbType::Hall, 0.65, "SIZE"),
        (ReverbType::Room, 0.0, "SIZE"),
    ] {
        let p = ReverbParams {
            kind,
            size,
            ..ReverbParams::default()
        };
        let base = SlotCtx::read(&ParamSnapshot::default(), Op::A);
        for c in [base.on_rev(&p), SlotCtx::read(&One(p), Op::A)] {
            assert_eq!(view(&reg::EFX, 1, &c).label(), want, "{kind:?} {size}");
            // Only the SIZE cell.
            assert_eq!(view(&reg::EFX, 2, &c).label(), "DECAY");
        }
    }
    assert_eq!(label(ReverbType::Hall, ParamId(3)), "SIZE");
}

/// ROOM's SYNC walks OFF and TIME only (spec: 7 of 10 beat values would
/// clamp at its 1.5 s); a stored ALL reads, and turns, as TIME. The others
/// walk OFF, TIME, ALL.
#[test]
fn room_sync_steps_off_and_time_only() {
    let mem = KnobMemory::new();
    let room = ReverbParams::default().with_type(ReverbType::Room, &mem);
    let sync = |p: &ReverbParams, d| {
        turn(
            p,
            ReverbParams::SYNC,
            d,
            &mut KnobMemory::new(),
            Tempo::DEFAULT,
        )
    };
    let mut seen = vec![room.sync];
    let mut p = room;
    for d in [1, 1, 1, -1, -1, -1] {
        p = sync(&p, d);
        seen.push(p.sync);
    }
    use RevSync::*;
    assert_eq!(seen, [Off, Time, Time, Time, Off, Off, Off]);
    let stored = ReverbParams { sync: All, ..room };
    assert_eq!(text(ReverbParams::SYNC, &stored).as_str(), "TIME");
    assert_eq!(sync(&stored, 1), stored, "ALL is TIME's end");
    assert_eq!(sync(&stored, -1).sync, Off);
    let hall = ReverbParams::default().with_type(ReverbType::Hall, &mem);
    let all = sync(&sync(&hall, 1), 1);
    assert_eq!(
        (all.sync, text(ReverbParams::SYNC, &all).as_str()),
        (All, "ALL")
    );
    // A SYNC turn converts the slot (`with_sync`): HALL's 7.3 ms PRE-DLY
    // reads its nearest note, none.
    assert_eq!(text(ReverbParams::PREDELAY, &all).as_str(), "0");
    assert_eq!(text(ReverbParams::TIME, &all).as_str(), "4 BT");
}

/// A synced knob walks its notes, one a tick, fine or not; its cell shows
/// the note that plays, marked when the engine folds it (GATE's 1/4 at
/// 90 BPM plays ▾1/8).
#[test]
fn synced_knobs_step_by_note_and_show_the_fold() {
    use chimera_core::ui::reverb_panel::{step, whole_ticks};
    let g = ReverbParams {
        sync: RevSync::Time,
        ..ReverbParams::default().with_type(ReverbType::Gate, &KnobMemory::new())
    };
    let t = |p: &ReverbParams, d| {
        turn(
            p,
            ReverbParams::SIZE,
            d,
            &mut KnobMemory::new(),
            Tempo::DEFAULT,
        )
    };
    let mut p = ReverbParams { size: 0.0, ..g };
    let mut notes = vec![text(ReverbParams::SIZE, &p).as_str().to_string()];
    for _ in 0..12 {
        p = t(&p, 1);
        notes.push(text(ReverbParams::SIZE, &p).as_str().to_string());
    }
    assert_eq!(
        notes,
        [
            "1/32T", "1/32", "1/16T", "1/32D", "1/16", "1/8T", "1/16D", "1/8", "1/4T", "1/8D",
            "1/4", "1/4", "1/4"
        ]
    );
    assert_eq!(step(&g, ReverbParams::SIZE), 0.1);
    assert!(whole_ticks(&g, ReverbParams::SIZE));
    assert!(!whole_ticks(&g, ReverbParams::TONE));
    let free = ReverbParams {
        sync: RevSync::Off,
        ..g
    };
    assert!(!whole_ticks(&free, ReverbParams::SIZE));
    assert_eq!(step(&free, ReverbParams::SIZE), 1.0 / 128.0);
    // The fold the engine reports marks the note that plays.
    let quarter = ReverbParams { size: 1.0, ..g };
    let at90 = Tempo::from_bpm(90.0);
    let shown = |f| value_text(ReverbParams::SIZE, &quarter, at90, FoldState(f));
    assert_eq!(shown(0).as_str(), "1/4");
    assert_eq!(shown(1).as_str(), "▾1/8");
    let fast = ReverbParams { size: 0.0, ..g };
    let shown = value_text(ReverbParams::SIZE, &fast, at90, FoldState(-1));
    assert_eq!(shown.as_str(), "▴1/16T");
}
