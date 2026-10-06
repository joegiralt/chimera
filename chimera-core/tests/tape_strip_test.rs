//! TAPE's strip (TAPE Task 16, spec § 3): the source's overview, min/max
//! a column, and the heads the audio stores once a block. The drawing is
//! locked by the screen goldens (`tape_strip_*`).

mod common;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::tape_engine::Heads;
use chimera_core::dsp::tape_engine::params::TapeModel;
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, HEADS, Instrument};
use chimera_core::modulation::ModState;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::ui::tape_strip::{COLS, Overview};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::tape::{OneSource, held_plain, sine};
use common::{SR, XorShift, scope_writer};

/// `n` frames of seeded noise at 24 kHz, as the arena holds them.
fn noise(n: usize) -> Vec<u8> {
    let mut rng = XorShift(0x57_21_90);
    let x: Vec<f32> = (0..n)
        .map(|_| rng.next_u32() as i32 as f32 / i32::MAX as f32)
        .collect();
    held_plain(&x, 24_000)
}

/// Column `c` of `[lo, hi)`: its frames, at least one.
fn span(lo: u32, hi: u32, c: usize) -> core::ops::Range<u32> {
    let n = u64::from(hi - lo);
    let a = lo + (c as u64 * n / COLS as u64) as u32;
    let b = lo + ((c as u64 + 1) * n / COLS as u64) as u32;
    a..b.max(a + 1)
}

fn expect(v: &SourceView<'_>, lo: u32, hi: u32) -> Vec<[i8; 2]> {
    (0..COLS)
        .map(|c| {
            let s = span(lo, hi, c).map(|i| (v.sample(i) >> 4) as i8);
            [s.clone().min().unwrap(), s.max().unwrap()]
        })
        .collect()
}

#[test]
fn overview_is_min_max() {
    for n in [5_000, 100, 64] {
        let held = noise(n);
        let v = SourceView::new(&held).unwrap();
        let frames = v.frames();
        let o = Overview::of(&v, 0.0, 1.0);
        assert_eq!(o.frames(), frames);
        assert_eq!(o.rate(), 24_000);
        let got: Vec<_> = (0..COLS).map(|c| o.col(c)).collect();
        assert_eq!(got, expect(&v, 0, frames), "{n} frames");
        // A window: only its frames, either way round.
        let (lo, hi) = (frames / 4, frames * 3 / 4);
        let w = Overview::of(&v, 0.75, 0.25);
        assert_eq!(w, Overview::of(&v, 0.25, 0.75));
        let got: Vec<_> = (0..COLS).map(|c| w.col(c)).collect();
        assert_eq!(got, expect(&v, lo, hi), "{n} frames, a window");
    }
    // Full scale reaches the column's ends; silence sits on 0.
    let held = held_plain(&sine(24_000, 50.0, 10_800), 24_000);
    let o = Overview::of(&SourceView::new(&held).unwrap(), 0.0, 1.0);
    assert!((0..COLS).all(|c| o.col(c) == [-128, 127]));
    let held = held_plain(&[0.0; 1_000], 24_000);
    let o = Overview::of(&SourceView::new(&held).unwrap(), 0.0, 1.0);
    assert!((0..COLS).all(|c| o.col(c) == [0, 0]));
    assert_eq!(Overview::EMPTY.frames(), 0);
}

/// A 100 000-frame source in under 1 ms on the host (spec § 3); the UI
/// computes it when the page opens or SOURCE changes.
#[cfg(not(debug_assertions))]
#[test]
fn overview_under_1ms() {
    let held = noise(100_000);
    let v = SourceView::new(&held).unwrap();
    let best = (0..5)
        .map(|_| {
            let t = std::time::Instant::now();
            std::hint::black_box(Overview::of(std::hint::black_box(&v), 0.0, 1.0));
            t.elapsed()
        })
        .min()
        .unwrap();
    assert!(best < std::time::Duration::from_millis(1), "{best:?}");
}

#[test]
fn heads_pack() {
    assert_eq!(Heads::NONE.head(), None);
    assert_eq!(Heads::NONE.second(), None);
    let h = Heads::new(7, None);
    assert_eq!((h.head(), h.second()), (Some(7), None));
    let h = Heads::new(0, Some(400_000));
    assert_eq!((h.head(), h.second()), (Some(0), Some(400_000)));
    assert_eq!(h.0, 400_000 << 32);
}

/// The only test in this binary that plays TAPE through an `Instrument`:
/// `HEADS` is global.
#[test]
fn heads_published_once_a_block() {
    const FRAMES: u32 = 48_000;
    let held = held_plain(&sine(48_000, 100.0, FRAMES), 48_000);
    let mut src = OneSource::new(&held);
    let mut inst = Box::new(Instrument::new(SR, SampleBudget::for_cpu(u32::MAX)));
    let (mut fx, mut dac, mut scope) = (
        Box::new(FxBus::new()),
        Box::new(DacBlocks::new()),
        scope_writer(),
    );
    let mut shared = Box::<AudioShared>::default();
    for (p, model) in [(0, TapeModel::Loop), (1, TapeModel::Tron)] {
        let a = &mut shared.parts[p];
        a.params = Sound::init(EngineType::Tape).params;
        a.params.tape.model = model;
        a.mod_state = ModState::new();
        a.source = Some(src.id);
    }
    let on = |shared: &AudioShared, p: usize, note: u8| NoteEvent {
        channel: shared.parts[p].mix.channel,
        note: MidiNote::new(note).unwrap(),
        kind: NoteKind::On(Velocity::DEFAULT),
    };
    let mut block = |inst: &mut Instrument, evs: &[NoteEvent], shared: &AudioShared| {
        let v = src.view();
        for &e in evs {
            inst.handle_with(e, shared, &v);
        }
        inst.render_with(&mut fx, &mut dac, shared, &mut scope, &v);
    };
    let b = BLOCK_SIZE as u32;

    block(&mut inst, &[], &shared);
    assert_eq!(HEADS.read(0), Heads::NONE, "no note, no heads");

    // C4 at ROOT on a 48 kHz source: a frame a sample, so the head
    // stands a block further each block, from START.
    block(&mut inst, &[on(&shared, 0, 60)], &shared);
    let first = HEADS.read(0).head().expect("a head");
    assert!(first <= b, "{first}");
    for k in 1..4 {
        block(&mut inst, &[], &shared);
        assert_eq!(HEADS.read(0).head(), Some(first + k * b), "block {k}");
    }
    assert_eq!(HEADS.read(1), Heads::NONE, "Part 2 plays nothing yet");

    // The newest voice: C5 plays two frames a sample.
    block(&mut inst, &[on(&shared, 0, 72)], &shared);
    let h = HEADS.read(0).head().unwrap();
    block(&mut inst, &[], &shared);
    assert_eq!(HEADS.read(0).head(), Some(h + 2 * b), "the newest note's head");

    // TRON's second is where its 8 s of tape runs out: at C4, past this
    // 1 s source, so END.
    block(&mut inst, &[on(&shared, 1, 60)], &shared);
    let t = HEADS.read(1);
    assert!(t.head().is_some());
    assert_eq!(t.second(), Some(FRAMES), "the tape outlasts the source");
    // LENGTH 0 (0.25 s): 12 000 frames from START at C4, 24 000 at C5.
    shared.parts[1].params.tape.size = 0.0;
    for (note, frames) in [(60, 12_000), (72, 24_000)] {
        block(&mut inst, &[on(&shared, 1, note)], &shared);
        let out = HEADS.read(1).second().unwrap();
        assert!(out.abs_diff(frames) <= 2 * b, "note {note}: {out} ≈ {frames}");
    }

    // A kill fades for a block, its heads still playing.
    inst.kill_all();
    block(&mut inst, &[], &shared);
    block(&mut inst, &[], &shared);
    assert_eq!(HEADS.read(0), Heads::NONE, "all quiet, no heads");
    assert_eq!(HEADS.read(1), Heads::NONE);
}
