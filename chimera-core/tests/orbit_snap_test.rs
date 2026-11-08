//! Live snapping (spec § 5.2): a played key snaps per Part to ORBIT's key
//! or chord while ORBIT runs, a key-up releases what it started, and a
//! held key on a RE-SNAP Part glides to the new chord without a click.

mod common;
mod screen;

use chimera_core::addr::BlockRef;

use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{Instrument, RESNAP_GLIDE_MS};
use chimera_core::note_queue::RealTime;
use chimera_core::orbit::ring::Harmony;
use chimera_core::part::{Held, LiveSnap, PartParams};
use chimera_core::project::{NEW_PARTS, PartId, Project};
use chimera_core::storage::encode_block;
use chimera_core::ui::UiState;
use chimera_core::ui::settings::leaves::{LIVE_SNAP_LEAF, OnePage};
use chimera_hal::{BLOCK_SIZE, ButtonId, EncoderId};
use chimera_theory::{Chord, Key, Note, PitchClass, Quality, Scale, SnapTo, snap};
use common::inst_rig::InstRig;
use common::orbit::OrbitRig;
use common::sweep::{BPS, Bench, CLICK_RATIO, Patch, Tape, control_diff_ratio, gone_from};
use common::{SR, fundamental_hz};
use screen::{Input, feed, tap, to_leaf};

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];

fn harmony(root: PitchClass, q: Quality) -> Harmony {
    Harmony {
        key: Key::new(PitchClass::A, Scale::MINOR),
        chord: Chord::new(root, q),
    }
}

const AM: (PitchClass, Quality) = (PitchClass::A, Quality::Min);
const G: (PitchClass, Quality) = (PitchClass::G, Quality::Maj);
const E: (PitchClass, Quality) = (PitchClass::E, Quality::Maj);
const F: (PitchClass, Quality) = (PitchClass::F, Quality::Maj);

/// `raw` snapped to `h`'s chord, as a MIDI note number.
fn to_chord(raw: u8, h: Harmony) -> u8 {
    snap(Note::new(raw).unwrap(), SnapTo::Chord(h.chord)).get()
}

/// The sounding note of each held voice on Part `p`.
fn held_notes(inst: &Instrument, p: PartId) -> Vec<u8> {
    let mut n: Vec<u8> = inst
        .allocator()
        .slots()
        .iter()
        .filter(|s| s.held() && s.part() == Some(p.index() as u8))
        .filter_map(|s| s.note().map(|n| n.get()))
        .collect();
    n.sort_unstable();
    n
}

/// The glide ratio of Part `p`'s one held voice.
fn slide_of(inst: &Instrument, p: PartId) -> f32 {
    let slots = inst.allocator().slots();
    let v = (0..slots.len())
        .find(|&v| slots[v].held() && slots[v].part() == Some(p.index() as u8))
        .expect("a held voice");
    inst.slides()[v]
}

fn rig() -> InstRig {
    InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V))
}

/// Two Parts on one channel, SNAP CHORD and OFF: the first sounds the
/// snapped note and the second the raw one; a key-up releases both. A
/// key-up of the snapped note releases neither.
#[test]
fn live_snaps_per_part() {
    let mut r = rig();
    r.shared.parts[1].mix.channel = r.shared.parts[0].mix.channel;
    r.shared.parts[0].mix.snap = LiveSnap::Chord;
    r.shared.parts[1].mix.snap = LiveSnap::Off;
    let am = harmony(AM.0, AM.1);
    r.inst.set_harmony(Some(am));
    r.note_on(0, 66);
    r.render(2);
    assert_eq!(held_notes(&r.inst, P1), [to_chord(66, am)]);
    assert_eq!(to_chord(66, am), 64);
    assert_eq!(held_notes(&r.inst, P2), [66]);
    // SCALE: the key's note, here F# to E or G in A minor.
    r.shared.parts[1].mix.snap = LiveSnap::Scale;
    r.note_on(0, 61);
    assert_eq!(held_notes(&r.inst, P2), [60, 66]);
    r.note_off(0, 61);

    r.note_off(0, 64);
    assert_eq!(
        held_notes(&r.inst, P1),
        [64],
        "64 is not the key that started it"
    );
    r.note_off(0, 66);
    assert!(held_notes(&r.inst, P1).is_empty());
    assert!(held_notes(&r.inst, P2).is_empty());
}

/// ORBIT stopped: a SNAP CHORD Part plays raw notes, and through a
/// running ORBIT it snaps.
#[test]
fn live_does_not_snap_when_stopped() {
    let mut r = rig();
    r.shared.parts[0].mix.snap = LiveSnap::Chord;
    r.inst.set_harmony(None);
    r.note_on(0, 66);
    assert_eq!(held_notes(&r.inst, P1), [66]);

    // Through the runner: stopped, then playing, then stopped again.
    let mut rig = OrbitRig::on_init();
    for ring in &mut rig.song().live_mut_for_test().rings {
        ring.ring.mute = true;
    }
    rig.project.edit_part(P1).mix.snap = LiveSnap::Chord;
    rig.key(0, 66, true);
    rig.run(2);
    assert_eq!(held_notes(rig.inst(), P1), [66]);
    rig.key(0, 66, false);
    rig.play(true);
    rig.run(4);
    rig.key(0, 66, true);
    rig.run(2);
    let chord = rig.view().chord;
    let want = snap(Note::new(66).unwrap(), SnapTo::Chord(chord)).get();
    assert_ne!(want, 66, "the scene's first chord moves F#");
    assert_eq!(held_notes(rig.inst(), P1), [want]);
    rig.key(0, 66, false);
    rig.play(false);
    rig.run(4);
    rig.key(0, 66, true);
    rig.run(2);
    assert_eq!(held_notes(rig.inst(), P1), [66]);
}

/// Blocks the re-snap glide is 95 % of the way in (`RESNAP_GLIDE_MS`).
fn glide_blocks() -> usize {
    (RESNAP_GLIDE_MS as usize * SR as usize / 1000).div_ceil(BLOCK_SIZE)
}

/// A chord change under two held keys: STAY keeps the pitch; RE-SNAP
/// glides to `snap(raw, new)`, 95 % of the way in 40 ms and there after;
/// five changes in a row never drift from the raw key.
#[test]
fn held_stay_and_resnap() {
    let mut r = rig();
    let (ch1, ch2) = (0, 1);
    for (p, held) in [(0, Held::Resnap), (1, Held::Stay)] {
        r.shared.parts[p].mix.snap = LiveSnap::Chord;
        r.shared.parts[p].mix.held = held;
    }
    let am = harmony(AM.0, AM.1);
    r.inst.set_harmony(Some(am));
    r.note_on(ch1, 66);
    r.note_on(ch2, 66);
    r.render(40);
    assert_eq!(held_notes(&r.inst, P1), [64]);

    let g = harmony(G.0, G.1);
    r.inst.set_harmony(Some(g));
    r.inst.resnap(&r.shared);
    assert_eq!(held_notes(&r.inst, P1), [to_chord(66, g)]);
    assert_eq!(held_notes(&r.inst, P2), [64], "STAY keeps the pitch");
    // The voice starts from where it sounded: E4 over G4.
    let start = slide_of(&r.inst, P1);
    assert!((start.log2() * 12.0 + 3.0).abs() < 1e-3, "{start}");
    r.render(glide_blocks());
    let left = slide_of(&r.inst, P1).log2() * 12.0;
    assert!(
        left.abs() <= 0.05 * 3.0 + 1e-4,
        "95 % of the way in {RESNAP_GLIDE_MS} ms: {left} st left"
    );
    r.render(BPS / 4);
    assert_eq!(slide_of(&r.inst, P1), 1.0, "landed");
    assert_eq!(slide_of(&r.inst, P2), 1.0);
    // And it sounds there: G4, Part 1's bus.
    let mut bus = Vec::new();
    for _ in 0..BPS / 4 {
        r.render(1);
        bus.extend_from_slice(r.inst.part_bus(0));
    }
    let g4 = 440.0 * 2f64.powf((67.0 - 69.0) / 12.0);
    let hz = fundamental_hz(&bus, g4);
    assert!((1200.0 * (hz / g4).log2()).abs() < 1.0, "{hz} Hz");

    // From the raw key each time: E after G would take G's B... from 66,
    // not from G's 67.
    let mut drifted = false;
    let mut last = to_chord(66, g);
    for (root, q) in [E, AM, F, E, G] {
        let h = harmony(root, q);
        r.inst.set_harmony(Some(h));
        r.inst.resnap(&r.shared);
        r.render(4);
        let want = to_chord(66, h);
        drifted |= to_chord(last, h) != want;
        assert_eq!(held_notes(&r.inst, P1), [want], "{root:?} {q:?}");
        assert_eq!(held_notes(&r.inst, P2), [64]);
        last = want;
    }
    assert!(
        drifted,
        "a change where snapping the last note would differ"
    );
    r.note_off(ch1, 66);
    r.note_off(ch2, 66);
    assert!(held_notes(&r.inst, P1).is_empty() && held_notes(&r.inst, P2).is_empty());
}

/// The runner re-snaps at each segment change: a held F# on a RE-SNAP Part
/// follows NEW's progression, each time from the raw key, and a STAY Part
/// keeps its first.
#[test]
fn the_runner_resnaps_at_a_segment_change() {
    let mut rig = OrbitRig::on_init();
    for ring in &mut rig.song().live_mut_for_test().rings {
        ring.ring.mute = true;
    }
    for (p, held) in [(P1, Held::Resnap), (P2, Held::Stay)] {
        let mix = &mut rig.project.edit_part(p).mix;
        mix.snap = LiveSnap::Chord;
        mix.held = held;
    }
    rig.play(true);
    rig.run(4);
    rig.key(0, 66, true);
    rig.key(1, 66, true);
    rig.run(2);
    let first = snap(Note::new(66).unwrap(), SnapTo::Chord(rig.view().chord)).get();
    let mut changes = 0;
    let mut seg = rig.view().seg;
    while changes < 3 {
        rig.block();
        if rig.view().seg != seg {
            seg = rig.view().seg;
            changes += 1;
            let want = snap(Note::new(66).unwrap(), SnapTo::Chord(rig.view().chord)).get();
            assert_eq!(held_notes(rig.inst(), P1), [want], "segment {seg}");
            assert_eq!(held_notes(rig.inst(), P2), [first], "segment {seg}");
        }
    }
}

/// NEW's scene on muted rings, Part 1 SNAP CHORD and HELD RE-SNAP, played
/// into its second segment with F# held: the rig, segment 0's chord and
/// the held note.
fn held_into_segment_1() -> (OrbitRig, Chord, u8) {
    let mut rig = OrbitRig::on_init();
    for ring in &mut rig.song().live_mut_for_test().rings {
        ring.ring.mute = true;
    }
    let mix = &mut rig.project.edit_part(P1).mix;
    mix.snap = LiveSnap::Chord;
    mix.held = Held::Resnap;
    rig.play(true);
    rig.run(4);
    let first = rig.view().chord;
    while rig.view().seg == 0 {
        rig.block();
    }
    rig.key(0, 66, true);
    rig.run(2);
    let now = snap(Note::new(66).unwrap(), SnapTo::Chord(rig.view().chord)).get();
    assert_eq!(held_notes(rig.inst(), P1), [now]);
    let back = snap(Note::new(66).unwrap(), SnapTo::Chord(first)).get();
    assert_ne!(now, back, "segment 1's chord moves F# elsewhere");
    (rig, first, back)
}

/// A MIDI Start goes back to bar 0, segment 0's chord, with no segment
/// step: the held key re-snaps to it.
#[test]
fn a_midi_start_back_to_bar_0_resnaps() {
    let (mut rig, first, back) = held_into_segment_1();
    rig.rt(0, RealTime::Start);
    rig.run(2);
    assert_eq!(rig.view().seg, 0);
    assert_eq!(rig.view().chord, first);
    assert_eq!(held_notes(rig.inst(), P1), [back]);
}

/// B6 stops, the key held, and starts again at bar 0: the held key
/// re-snaps to segment 0's chord.
#[test]
fn a_b6_restart_resnaps() {
    let (mut rig, _, back) = held_into_segment_1();
    rig.play(false);
    rig.run(8);
    rig.play(true);
    rig.run(2);
    assert_eq!(rig.view().seg, 0);
    assert_eq!(held_notes(rig.inst(), P1), [back]);
}

/// Hold C, tap D, which snaps to C (a tie goes down) and joins C's voice:
/// D's key-up leaves C sounding, and C's own key-up lets it go.
#[test]
fn a_key_up_keeps_a_voice_another_key_holds() {
    let mut r = rig();
    r.shared.parts[0].mix.snap = LiveSnap::Chord;
    let am = harmony(AM.0, AM.1);
    r.inst.set_harmony(Some(am));
    r.note_on(0, 60);
    r.render(4);
    r.note_on(0, 62);
    assert_eq!(to_chord(62, am), 60);
    assert_eq!(held_notes(&r.inst, P1), [60], "D joins C's voice");
    r.render(4);
    r.note_off(0, 62);
    r.render(4);
    assert_eq!(held_notes(&r.inst, P1), [60], "C still down");
    assert!(r.inst.active()[..].iter().any(|&a| a), "and sounding");
    r.note_off(0, 60);
    assert!(held_notes(&r.inst, P1).is_empty());
}

/// A held note on `patch`, RE-SNAP or STAY, snapped from `raw` to chord
/// `from`, then at block `AT` to `to`; with the glide's ratio each block
/// from `AT` on, in semitones.
fn retune_tape(
    patch: Patch,
    raw: u8,
    (from, to): (Harmony, Harmony),
    held: Held,
) -> (Tape, Vec<f32>) {
    let mix = PartParams {
        snap: LiveSnap::Chord,
        held,
        ..PartParams::default()
    };
    let mut b = Bench::new(patch, mix, FxParams::OFF);
    b.harmony(Some(from));
    let mut t = Tape::default();
    let mut glide = Vec::new();
    for i in 0..AT + 80 {
        if i == 0 {
            b.note(raw, Some(100));
        }
        if i == AT {
            b.harmony(Some(to));
            b.resnap();
        }
        b.render(&mut t);
        if i >= AT {
            glide.push(b.slides().iter().product::<f32>().log2() * 12.0);
        }
    }
    (t, glide)
}

const AT: usize = 60;

/// A re-snap glides, and nothing clicks at it, on INIT Algo, every Modal
/// model and NEW's factory Sounds, up and down, low to high. The glide:
/// a block in it is still near where it sounded, 40 ms in 95 % there.
/// The click, judged twice: the render alone (`Tape::click`), and the
/// STAY render less the RE-SNAP one against its own steady state, as a
/// steal's fade is.
#[test]
fn retune_glides_without_a_click() {
    let mut patches = vec![
        Patch::AlgoInit,
        Patch::ModalInit(ResonatorMode::String),
        Patch::ModalInit(ResonatorMode::Bowed),
        Patch::ModalInit(ResonatorMode::Sympathetic),
        Patch::ModalInit(ResonatorMode::Modal),
    ];
    patches.extend(NEW_PARTS.map(|s| Patch::Factory(s.index() as u8)));
    let (am, g) = (harmony(AM.0, AM.1), harmony(G.0, G.1));
    let mut worst = (0.0f32, String::new());
    for patch in patches {
        for raw in [42, 66, 78] {
            for (way, (from, to)) in [("up", (am, g)), ("down", (g, am))] {
                let name = format!("{} F#{} {way}", patch.name(), raw / 12 - 1);
                let (t, glide) = retune_tape(patch, raw, (from, to), Held::Resnap);
                assert!(t.finite(), "{name}");
                let span = f32::from(to_chord(raw, from)) - f32::from(to_chord(raw, to));
                assert!(span.abs() >= 1.0, "{name}: the chord moves the note");
                assert!(
                    glide[0] / span >= 0.8,
                    "{name}: a block in, {} of {span} st left",
                    glide[0]
                );
                let left = glide[glide_blocks()] / span;
                assert!(
                    (0.0..=0.05).contains(&left),
                    "{name}: 40 ms in, {left} left"
                );

                assert_eq!(t.click(0, AT, (0, usize::MAX), true), None, "{name}");
                let (control, _) = retune_tape(patch, raw, (from, to), Held::Stay);
                let r = control_diff_ratio(&gone_from(&control, &t), AT);
                if r > worst.0 {
                    worst = (r, name.clone());
                }
                assert!(r <= CLICK_RATIO, "{name}: the retune clicks, {r:.1}×");
            }
        }
    }
    println!("worst retune: {:.2}× ({})", worst.0, worst.1);
}

/// B3, then SETTINGS › PART › LIVE SNAP: SNAP and HELD edit Part 3's, and
/// the leaf is one page.
#[test]
fn live_snap_leaf_edits_the_active_part() {
    let _: &OnePage = &LIVE_SNAP_LEAF;
    let def = LIVE_SNAP_LEAF.def();
    assert_eq!(def.id, 85);
    let mut ui = UiState::new();
    tap(&mut ui, ButtonId::B3);
    to_leaf(&mut ui, &["PART", "LIVE SNAP"]);
    let at = ui.location();
    feed(&mut ui, Input::turn(EncoderId::A, 2));
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    let p3 = ui.project().part(PartId::ALL[2]).mix;
    assert_eq!((p3.snap, p3.held), (LiveSnap::Chord, Held::Resnap));
    for p in [0, 1, 3, 4, 5] {
        let m = ui.project().part(PartId::ALL[p]).mix;
        assert_eq!(
            (m.snap, m.held),
            (LiveSnap::Off, Held::Stay),
            "Part {}",
            p + 1
        );
    }
    // One page: EDIT steps nowhere.
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.location(), at);
}

/// A PART block written before SNAP and HELD (ids 8 and 9) decodes to OFF
/// and STAY, whatever the Part held before.
#[test]
fn part_snap_default_is_neutral() {
    let d = PartParams::default();
    assert_eq!((d.snap, d.held), (LiveSnap::Off, Held::Stay));
    // A whole PART block, less ids 8 and 9 (and BEND, 10, and TUNING, 11).
    let mut buf = chimera_core::storage::RecordBuf::new();
    encode_block(BlockRef::Part, &d, None, &mut buf);
    let old: Vec<u8> = {
        let b = buf.as_slice();
        let mut v = vec![b[0]];
        for e in b[1..].chunks(5) {
            if ![
                PartParams::SNAP,
                PartParams::HELD,
                PartParams::BEND,
                PartParams::TUNING,
            ]
            .map(|i| i.0)
            .contains(&e[0])
            {
                v.extend_from_slice(e);
            }
        }
        v
    };
    assert_eq!(old.len(), 1 + 5 * 5);
    // The v1 project fixture, written before them, into Parts that snapped.
    let (mut p, _) = Project::boxed();
    for id in PartId::ALL {
        let mix = &mut p.edit_part(id).mix;
        mix.snap = LiveSnap::Chord;
        mix.held = Held::Resnap;
        mix.bend = 12;
    }
    let f = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1/project.prj"),
    )
    .unwrap();
    common::project::decode(&f, &mut p).unwrap();
    for id in PartId::ALL {
        let m = p.part(id).mix;
        assert_eq!((m.snap, m.held), (LiveSnap::Off, Held::Stay), "{id:?}");
        assert_eq!(m.bend, chimera_core::part::INIT_BEND, "{id:?}");
    }
    // And the old block alone, decoded into a fresh Part.
    let mut fresh = PartParams::for_part(0);
    chimera_core::storage::decode_block(&old, &[], &[], Some(&mut One(&mut fresh))).unwrap();
    assert_eq!((fresh.snap, fresh.held), (LiveSnap::Off, Held::Stay));
}

/// A lone PART block as `Blocks`.
struct One<'a>(&'a mut PartParams);

impl chimera_core::addr::BlockRead for One<'_> {
    fn block(&self, b: BlockRef) -> Option<chimera_core::addr::BlockView<'_>> {
        (b == BlockRef::Part).then_some(chimera_core::addr::BlockView::Plain(&*self.0))
    }
}

impl chimera_core::addr::Blocks for One<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<chimera_core::addr::BlockViewMut<'_>> {
        (b == BlockRef::Part).then_some(chimera_core::addr::BlockViewMut::Plain(&mut *self.0))
    }
}
