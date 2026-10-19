//! Mapped CCs move the Sound in the UI (spec §§ 5-7): through the
//! encoder's edit path, ramped over the gap between CCs, and never more
//! than `MOVES − 1` knob moves in flight a Part, so the audio never lands
//! the knobs. A `UiState` is published to a real `Instrument` as the
//! shells do: one publish a pass, the audio's blocks between, or none.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Blocks, Op, ParamAddr};
use chimera_core::block::{ParamKind, ParamSpec};
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::knobs::{KNOBS, Knob, KnobPublisher, MOVES, coeff};
use chimera_core::dsp::modal::ModalParams;
use chimera_core::dsp::va::VaParams;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::midi_in::{CC_RING_LEN, Cc, CcEvent, CcRing};
use chimera_core::midi_map::{MapEntry, Mappable};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::project::{
    Confirmed, Line, LoadLink, PartFrom, PartId, PartSource, PartStatus, ProjectSource,
    ProjectStatus, ReplaceGuard, part_status,
};
use chimera_core::scope::ScopeWriter;
use chimera_core::storage::{Card, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::animation::{FRAME_MS, UiTick};
use chimera_core::ui::block_registry::{ALGO_OP_PAGES, FILTER, VA_HOME};
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::nav::PartChain;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId, Ms, SAMPLE_RATE};
use screen::{Input, feed};

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const TYPE: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::KIND);
const WAVE_A: ParamAddr = ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::WAVE);
const RESO: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
const WAVE1: ParamAddr = ParamAddr::new(BlockRef::Va, VaParams::WAVE1);
const VA_MODE: ParamAddr = ParamAddr::new(BlockRef::Va, VaParams::MODE);
const STRUCTURE: ParamAddr = ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE);

/// A UI pass: 4 ms, three 1.33 ms audio blocks.
const PASS_MS: u32 = 4;
const PASS_BLOCKS: usize = 3;

/// The fast turn's largest stored jump in one block, a tenth of the
/// range (`param_motion_test::a_fast_turn_of_every_knob_glides`).
const FAST_JUMP: f32 = 0.1;

fn spec(a: ParamAddr) -> &'static ParamSpec {
    a.spec().unwrap()
}

/// A UI, published each pass to an `Instrument`, with the most knob moves
/// of Part 1 the audio took in one block, and whether it has taken the
/// last publish.
struct Bench {
    ui: Box<UiState>,
    shared: Box<AudioShared>,
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    knobs: Box<KnobPublisher>,
    now: u32,
    seen: u32,
    most: u32,
    taken: bool,
}

impl Bench {
    /// Part 1 on `s`, sustained.
    fn new(mut s: Sound) -> Self {
        s.params.envelopes[0].sustain = 1.0;
        let mut ui = Box::new(crate::screen::stack_ui());
        screen::init_part(&mut ui, P1);
        let was = PartChain::of(&ui.project().part(P1).sound);
        **ui.project_mut().edit_part(P1).sound = s;
        ui.replaced_for_test(was);
        let mut b = Self {
            ui,
            shared: Box::new(AudioShared::default()),
            inst: Box::new(Instrument::new(
                SAMPLE_RATE,
                SampleBudget::for_cpu(CPU_HZ_REV_V),
            )),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: common::scope_writer(),
            knobs: Box::new(KnobPublisher::new()),
            now: 1_000,
            seen: 0,
            most: 0,
            taken: true,
        };
        b.publish();
        b.reset_most();
        b
    }

    fn algo() -> Self {
        Self::new(Sound::plain_init(EngineType::Algo))
    }

    fn ch(&self, p: PartId) -> MidiChannel {
        self.ui.project().part(p).mix.channel
    }

    fn map(&mut self, p: PartId, cc: u8, a: ParamAddr) {
        let s = self.ui.project_mut().edit_part(p).sound;
        s.map
            .learn(Cc::new(cc).unwrap(), Mappable::new(a).unwrap())
            .unwrap();
    }

    fn ev(&self, p: PartId, cc: u8, value: u8) -> CcEvent {
        CcEvent {
            channel: self.ch(p),
            cc: Cc::new(cc).unwrap(),
            value,
        }
    }

    /// One pass `ms` on: the CCs, a frame of input, a publish, the blocks.
    fn pass_with(&mut self, ccs: Vec<CcEvent>, input: Option<Input>, ms: u32, blocks: usize) {
        self.now += ms;
        // As the shells: the hand first, then the CCs in the room it left.
        if let Some(i) = input {
            feed(&mut self.ui, i);
        }
        self.ui.hear_ccs(ccs.into_iter(), Ms(self.now), self.taken);
        self.publish();
        for _ in 0..blocks {
            self.render();
        }
    }

    /// One audio block: it takes the publish, every move since it last did.
    fn render(&mut self) {
        let c = self.shared.parts[0].knobs.count();
        self.most = self.most.max(c.wrapping_sub(self.seen));
        (self.seen, self.taken) = (c, true);
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
    }

    /// As if the audio had taken every move so far: the count starts now.
    fn reset_most(&mut self) {
        self.seen = self.shared.parts[0].knobs.count();
        self.most = 0;
    }

    /// One 4 ms pass, Part 1's CCs `(cc, value)`.
    fn pass(&mut self, ccs: &[(u8, u8)]) {
        let ev = ccs.iter().map(|&(c, v)| self.ev(P1, c, v)).collect();
        self.pass_with(ev, None, PASS_MS, PASS_BLOCKS);
    }

    fn passes(&mut self, n: usize) {
        for _ in 0..n {
            self.pass(&[]);
        }
    }

    fn publish(&mut self) {
        self.shared
            .update_from_project(self.ui.project(), 0, &mut self.knobs);
        self.taken = false;
    }

    fn note(&mut self) {
        let ev = NoteEvent {
            channel: self.ch(P1),
            note: MidiNote::new(57).unwrap(),
            kind: NoteKind::On(Velocity::DEFAULT),
        };
        self.inst.handle(ev, &self.shared);
        for _ in 0..200 {
            self.render();
        }
    }

    fn stored(&self, p: PartId, a: ParamAddr) -> f32 {
        let part = self.ui.project().read_part(p);
        part.block(a.block).unwrap().get(a.param)
    }

    /// `a`'s stored place, 0..1.
    fn place(&self, p: PartId, a: ParamAddr) -> f32 {
        spec(a).normalize(self.stored(p, a))
    }

    /// Part 1's sounding voice's `a` last block, as a place.
    fn played(&self, a: ParamAddr) -> f32 {
        let busy = self.inst.allocator().slots().map(|s| !s.is_free());
        let v = busy.iter().position(|&b| b).expect("a voice sounds");
        spec(a).normalize(self.inst.played(a)[v])
    }

    /// The active page to `id`, PLUS by PLUS.
    fn go_to_page(&mut self, id: u16) {
        for _ in 0..12 {
            if self.ui.page_def().id == id {
                return;
            }
            feed(&mut self.ui, Input::press(ButtonId::Plus));
        }
        panic!("page {id} not reached");
    }
}

/// The place CC value `v` maps to on a full range.
fn place_of(v: u8) -> f32 {
    v as f32 / 127.0
}

#[test]
fn cc_moves_cutoff() {
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    let stamp = b.ui.project().part(P1).sound.stamp();
    assert_ne!(b.stored(P1, CUTOFF), spec(CUTOFF).min);
    b.pass(&[(74, 0)]);
    b.passes(12);
    assert_eq!(b.stored(P1, CUTOFF), spec(CUTOFF).min);
    let p = b.ui.project();
    assert_ne!(p.part(P1).sound.stamp(), stamp, "the Sound was written");
    assert_eq!(part_status(p.part(P1), p.pool()), PartStatus::Edited);
}

/// Hand-speed sweeps to each end of a learned range, then rest: the
/// stored value lands on the spec's own min and max, not a hair short,
/// for a continuous target and a stepped one, full and inverted.
#[test]
fn swept_ends_reach_min_and_max() {
    for a in [WAVE1, VA_MODE] {
        for invert in [false, true] {
            let mut b = Bench::new(Sound::plain_init(EngineType::Va));
            b.map(P1, 20, a);
            if invert {
                let s = b.ui.project_mut().edit_part(P1).sound;
                let e = s.map.entries()[0];
                s.map.set(
                    0,
                    MapEntry {
                        min: 255,
                        max: 0,
                        ..e
                    },
                );
            }
            let (lo, hi) = if invert {
                (spec(a).max, spec(a).min)
            } else {
                (spec(a).min, spec(a).max)
            };
            for _ in 0..2 {
                for v in (0..=100u8).rev() {
                    b.pass(&[(20, v)]);
                    b.passes(2);
                }
                b.passes(125);
                assert_eq!(b.stored(P1, a), lo, "{a:?} CC 0, inverted {invert}");
                for v in 27..=127u8 {
                    b.pass(&[(20, v)]);
                    b.passes(2);
                }
                b.passes(125);
                assert_eq!(b.stored(P1, a), hi, "{a:?} CC 127, inverted {invert}");
            }
        }
    }
}

/// 8 continuous knobs Part 1's engine reads, the filter's aside (the
/// hand's, on FLT): every ramp step is a move.
fn eight_knobs() -> Vec<&'static Knob> {
    let k: Vec<_> = KNOBS
        .iter()
        .filter(|k| {
            let m = Mappable::new(k.addr);
            k.addr.block != BlockRef::Filter
                && k.spec.kind == ParamKind::Continuous
                && k.addr.block.voice_reads()
                && m.is_some_and(|m| m.live(EngineType::Algo))
        })
        .take(8)
        .collect();
    assert_eq!(k.len(), 8);
    k
}

/// 8 mapped knobs swept 0→127 over 2 s, FLT's `hands` each turned every
/// pass: no publish overflows, the budget is full, every knob glides and
/// arrives.
fn sweep(hands: &[EncoderId]) {
    let mut b = Bench::algo();
    let knobs = eight_knobs();
    for (i, k) in knobs.iter().enumerate() {
        b.map(P1, 20 + i as u8, k.addr);
        // From the bottom, so the first CC is no jump.
        let s = b.ui.project_mut().edit_part(P1).sound;
        let mut blk = s.params.block_mut(k.addr.block).unwrap();
        blk.set(k.addr.param, k.spec.denormalize(0.0));
    }
    // The filter mid-range: each hand tick moves its knob.
    let f = &mut b.ui.project_mut().edit_part(P1).sound.params.filter;
    f.cutoff = spec(CUTOFF).denormalize(0.5);
    f.resonance = spec(RESO).denormalize(0.5);
    b.publish();
    b.note();
    b.reset_most();
    b.go_to_page(FILTER.id);
    let mut steps = vec![0.0f32; knobs.len()];
    let mut last: Vec<f32> = knobs.iter().map(|k| b.played(k.addr)).collect();
    let (n, mut was) = (500, u8::MAX);
    for t in 0..=n {
        let v = (127 * t / n) as u8;
        let ccs: Vec<_> = if v == was {
            Vec::new()
        } else {
            (0..knobs.len())
                .map(|i| b.ev(P1, 20 + i as u8, v))
                .collect()
        };
        was = v;
        let d = if t % 2 == 0 { 1 } else { -1 };
        let turn = hands
            .iter()
            .fold(Input::default(), |i, &e| i.and_turn(e, d));
        b.pass_with(ccs, Some(turn), PASS_MS, 0);
        for _ in 0..PASS_BLOCKS {
            b.render();
            for (i, k) in knobs.iter().enumerate() {
                let now = b.played(k.addr);
                steps[i] = steps[i].max((now - last[i]).abs());
                last[i] = now;
            }
        }
    }
    // Full, never over: the CC knobs, the hand's, and one kept for a key.
    assert_eq!(b.most as usize, MOVES - 1, "a publish of {} moves", b.most);
    b.passes(200);
    for (i, k) in knobs.iter().enumerate() {
        assert_eq!(b.place(P1, k.addr), 1.0, "{:?} swept to the top", k.addr);
        let bound = 2.0 * (1.0 - coeff(k.tau, SAMPLE_RATE)) * FAST_JUMP;
        assert!(
            steps[i] <= bound,
            "{:?} {}: {} a block, bound {bound}",
            k.addr,
            k.spec.label,
            steps[i]
        );
    }
}

#[test]
fn cc_sweep_never_snaps() {
    sweep(&[EncoderId::B]);
}

/// Two knobs turned each pass: the CCs give up a write for each.
#[test]
fn cc_sweep_under_two_hands_never_overflows() {
    sweep(&[EncoderId::B, EncoderId::C]);
}

#[test]
fn slow_cc74_step_bound() {
    let mut s = Sound::plain_init(EngineType::Va);
    s.params.filter.cutoff = spec(CUTOFF).min;
    s.params.filter.resonance =
        0.8 * spec(ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE)).max;
    let mut b = Bench::new(s);
    b.map(P1, 74, CUTOFF);
    b.note();
    let mut last = b.inst.played(CUTOFF);
    let mut most = 0.0f32;
    for v in 0..=127u8 {
        for k in 0..20 {
            let ccs = if k == 0 { vec![(74, v)] } else { vec![] };
            b.pass(&ccs);
            let now = b.inst.played(CUTOFF);
            for (a, z) in last.iter().zip(&now) {
                if *a > 0.0 && *z > 0.0 {
                    most = most.max((z / a).log2().abs());
                }
            }
            last = now;
        }
    }
    b.passes(50);
    assert_eq!(b.stored(P1, CUTOFF), spec(CUTOFF).max);
    // Per pass of three blocks: no block of the three moves more.
    assert!(most <= 0.03, "{most} octave in a pass");
}

/// A block each 1 ms pass: the audio takes each publish.
#[test]
fn p_lock_crosses_in_floor() {
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    b.pass_with(vec![b.ev(P1, 74, 0)], None, 1, 1);
    for _ in 0..1_000 {
        b.pass_with(vec![], None, 1, 1);
    }
    assert_eq!(b.place(P1, CUTOFF), 0.0);
    b.pass_with(vec![b.ev(P1, 74, 100)], None, 1, 1);
    let to = spec(CUTOFF).denormalize(place_of(100));
    let mut ms = 0;
    while b.stored(P1, CUTOFF) != to {
        b.pass_with(vec![], None, 1, 1);
        ms += 1;
        assert!(ms <= 15, "not there in 15 ms");
    }
    assert!(ms >= 10, "crossed in {ms} ms: a jump, not a ramp");
}

#[test]
fn stepped_cc_hysteresis() {
    let mut b = Bench::algo();
    b.map(P1, 30, TYPE);
    let mut changes = 0;
    let mut was = b.stored(P1, TYPE);
    assert_eq!(was, 0.0);
    for v in [63, 64, 63, 64] {
        b.pass(&[(30, v)]);
        let now = b.stored(P1, TYPE);
        changes += usize::from(now != was);
        was = now;
    }
    assert_eq!(changes, 1, "63/64/63/64 at a boundary moves once");
    assert_eq!(was, 1.0);
    b.pass(&[(30, 66)]);
    assert_eq!(b.stored(P1, TYPE), 2.0, "2 counts past it, it moves");
}

/// Operator A at FRMT and every other at NOISE: the budget is full, so
/// A's WAVE up from FRMT is refused, and stays (a turn shows it dimmed).
fn budget_full() -> Sound {
    let mut s = Sound::plain_init(EngineType::Algo);
    s.params.algo.try_set_kind(0, OpKind::Formant).unwrap();
    for i in 1..OPS {
        s.params.algo.try_set_kind(i, OpKind::Noise).unwrap();
    }
    s
}

#[test]
fn stepped_cc_past_the_formant_budget_is_refused_as_a_turn() {
    let full = ToastStep::Show(Line::new("FORMANT BUDGET FULL"));
    let kind = |b: &Bench| b.ui.project().part(P1).sound.params.algo.kinds().get(0);
    let mut b = Bench::new(budget_full());
    b.map(P1, 31, WAVE_A);
    b.ui.step_toast(60_000);
    b.pass(&[(31, 127)]);
    assert_eq!(b.ui.step_toast(0), full, "the CC toasts");
    assert_eq!(kind(&b), OpKind::Formant, "the value kept");
    // The encoder's turn, on the same Sound: the same refusal.
    screen::to_osc(&mut b.ui);
    screen::to_osc_sub(&mut b.ui, &ALGO_OP_PAGES[0]);
    b.ui.step_toast(60_000);
    feed(&mut b.ui, Input::turn(EncoderId::A, 1));
    assert_eq!(
        b.ui.step_toast(0),
        ToastStep::Idle,
        "the turn doesn't toast"
    );
    let shown = b.ui.held_choice().expect("NOIS is shown dimmed");
    assert_eq!(shown.held.code, 17);
    assert_eq!(kind(&b), OpKind::Formant);
}

#[test]
fn cc_on_other_channel_ignored() {
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    assert_ne!(b.ch(P1), b.ch(P2));
    let (c1, c2) = (b.stored(P1, CUTOFF), b.stored(P2, CUTOFF));
    let rev = b.ui.project().rev();
    let ev = b.ev(P2, 74, 127);
    b.pass_with(vec![ev], None, PASS_MS, 0);
    for _ in 0..20 {
        b.pass_with(vec![], None, PASS_MS, 0);
    }
    assert_eq!(b.stored(P1, CUTOFF), c1);
    assert_eq!(b.stored(P2, CUTOFF), c2);
    assert_eq!(b.ui.project().rev(), rev, "nothing written");
}

#[test]
fn two_parts_one_channel_both_apply() {
    let mut b = Bench::algo();
    let ch = b.ch(P1);
    b.ui.project_mut().edit_part(P2).mix.channel = ch;
    b.map(P1, 74, CUTOFF);
    b.map(P2, 74, CUTOFF);
    b.pass(&[(74, 0)]);
    b.passes(10);
    assert_eq!(b.stored(P1, CUTOFF), spec(CUTOFF).min);
    assert_eq!(b.stored(P2, CUTOFF), spec(CUTOFF).min);
}

#[test]
fn ramps_reset_on_load() {
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    b.pass(&[(74, 0)]);
    b.pass(&[]);
    assert!(b.stored(P1, CUTOFF) > spec(CUTOFF).min, "mid-ramp");
    let src = PartSource {
        part: P1,
        from: PartFrom::Init(EngineType::Algo),
    };
    let t = b.ui.template();
    common::project::load_anyway(b.ui.project_mut(), t, src).unwrap();
    // The new Sound maps CC 74 too: only a reset ramp stays put.
    b.map(P1, 74, CUTOFF);
    let c = b.stored(P1, CUTOFF);
    let rev = b.ui.project().rev();
    b.passes(20);
    assert_eq!(b.stored(P1, CUTOFF), c, "the old ramp is gone");
    assert_eq!(b.ui.project().rev(), rev);
    // The last CC again is heard: the load forgot it.
    b.pass(&[(74, 0)]);
    b.passes(10);
    assert_eq!(b.stored(P1, CUTOFF), spec(CUTOFF).min);
}

#[test]
fn dead_entries_after_engine_switch() {
    let mut b = Bench::new(Sound::plain_init(EngineType::Modal));
    b.map(P1, 40, STRUCTURE);
    let map = b.ui.project().part(P1).sound.map;
    let swap = |b: &mut Bench, e| {
        let s = b.ui.project_mut().edit_part(P1).sound;
        **s = Sound::plain_init(e);
        s.map = map;
    };
    swap(&mut b, EngineType::Algo);
    assert_eq!(b.ui.project().part(P1).sound.map, map, "the entry kept");
    let before = b.ui.project().part(P1).sound.clone();
    let rev = b.ui.project().rev();
    b.pass(&[(40, 127)]);
    b.passes(10);
    assert!(b.ui.project().part(P1).sound.bits_eq(&before));
    assert_eq!(b.ui.project().rev(), rev, "no block touched");
    swap(&mut b, EngineType::Modal);
    b.pass(&[(40, 0)]);
    b.passes(10);
    assert_eq!(b.place(P1, STRUCTURE), 0.0, "back on MODAL, it applies");
}

#[test]
fn hand_wins() {
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    b.go_to_page(FILTER.id);
    b.pass(&[(74, 0)]);
    b.pass(&[]);
    let mid = b.stored(P1, CUTOFF);
    assert!(mid > spec(CUTOFF).min, "mid-ramp");
    b.pass_with(vec![], Some(Input::turn(EncoderId::B, 3)), 0, 0);
    let turned = b.stored(P1, CUTOFF);
    assert!(turned > mid, "B turns CUTOFF");
    b.passes(20);
    assert_eq!(b.stored(P1, CUTOFF), turned, "the hand's value stays");
}

#[test]
fn ring_keeps_the_last_value() {
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    let ring = CcRing::new();
    for i in 0..CC_RING_LEN + 10 {
        ring.push(b.ev(P1, 74, (i % 100) as u8 + 20));
    }
    assert_eq!(ring.dropped(), 10);
    b.now += PASS_MS;
    b.ui.hear_ccs(ring.drain(), Ms(b.now), true);
    ring.push(b.ev(P1, 74, 5));
    b.now += PASS_MS;
    b.ui.hear_ccs(ring.drain(), Ms(b.now), true);
    for _ in 0..40 {
        b.pass(&[]);
    }
    let want = place_of(5);
    assert!(
        (b.place(P1, CUTOFF) - want).abs() < 1e-5,
        "{} != {want}",
        b.place(P1, CUTOFF)
    );
}

/// `go` loaded as LOAD does, with no audio to settle.
fn load_project(b: &mut Bench, s: &mut MemStore, go: Confirmed<ProjectSource>) {
    let mut card = Card::new();
    let (mut sync, mut set, _) = SystemSync::boot(&mut card, s);
    let link = LoadLink::new();
    b.ui.load_project(&mut card, s, &mut sync, &mut set, go, &link, |swap, _| {
        let _ = swap.settle(&link, || false);
    });
}

/// Mid-ramp on CUTOFF, a project whose Part 1 maps CC 74 to RESO
/// loads: the old ramp writes nothing into it, so it stays Saved.
#[test]
fn a_project_load_ends_every_ramp() {
    let mut s = MemStore::new(1);
    let f = screen::put_with(&mut s, "MAPPED", |p| {
        let m = Mappable::new(RESO).unwrap();
        p.edit_part(P1)
            .sound
            .map
            .learn(Cc::new(74).unwrap(), m)
            .unwrap();
    });
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    b.pass(&[(74, 0)]);
    b.pass(&[]);
    assert!(b.stored(P1, CUTOFF) > spec(CUTOFF).min, "mid-ramp");
    let go = common::project::confirm_load(b.ui.project(), b.ui.template(), f);
    load_project(&mut b, &mut s, go);
    assert_eq!(b.ui.project().meta().file(), Some(f));
    let (reso, rev) = (b.stored(P1, RESO), b.ui.project().rev());
    b.passes(20);
    assert_eq!(b.stored(P1, RESO), reso);
    assert_eq!(b.ui.project().rev(), rev, "nothing written");
    assert_eq!(b.ui.project_status(), ProjectStatus::Saved);
}

/// Mid-ramp, NEW: the next map heard writes nothing until a CC comes.
#[test]
fn new_ends_every_ramp() {
    let mut s = MemStore::new(1);
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    b.pass(&[(74, 0)]);
    b.pass(&[]);
    assert!(b.stored(P1, CUTOFF) > spec(CUTOFF).min, "mid-ramp");
    let (p, t) = (b.ui.project(), b.ui.template());
    let go = ReplaceGuard::check(p, t, ProjectSource::New)
        .unwrap_or_else(|n| n.into_pending().anyway(p));
    load_project(&mut b, &mut s, go);
    let status = b.ui.project_status();
    let (c, rev) = (b.stored(P1, CUTOFF), b.ui.project().rev());
    b.passes(20);
    assert_eq!(b.stored(P1, CUTOFF), c);
    assert_eq!(b.ui.project().rev(), rev, "nothing written");
    assert_eq!(b.ui.project_status(), status);
    // A map on the new Sound, entry 0 again: still no stale ramp.
    b.map(P1, 74, CUTOFF);
    let rev = b.ui.project().rev();
    b.passes(20);
    assert_eq!(b.stored(P1, CUTOFF), c);
    assert_eq!(b.ui.project().rev(), rev, "nothing written");
}

/// The hand beats a CC in its pass (spec § 5): a turn and its param's CC
/// in one pass, the turned value stands and nothing ramps or steps; the
/// next pass's CC applies.
#[test]
fn hand_beats_a_cc_in_its_pass() {
    let mut b = Bench::algo();
    b.map(P1, 74, CUTOFF);
    b.map(P1, 75, TYPE);
    b.go_to_page(FILTER.id);
    let (cut0, type0) = (b.stored(P1, CUTOFF), b.stored(P1, TYPE));
    let ccs = vec![b.ev(P1, 74, 127)];
    b.pass_with(
        ccs,
        Some(Input::turn(EncoderId::B, -3)),
        PASS_MS,
        PASS_BLOCKS,
    );
    let cut = b.stored(P1, CUTOFF);
    assert!(cut < cut0, "B turned CUTOFF down");
    let ccs = vec![b.ev(P1, 75, 127)];
    b.pass_with(
        ccs,
        Some(Input::turn(EncoderId::A, 1)),
        PASS_MS,
        PASS_BLOCKS,
    );
    let kind = b.stored(P1, TYPE);
    assert_ne!(kind, type0, "A turned TYPE");
    assert_ne!(kind, spec(TYPE).max, "short of where the CC points");
    b.passes(20);
    assert_eq!(b.stored(P1, CUTOFF), cut, "no ramp from the pass's CC");
    assert_eq!(b.stored(P1, TYPE), kind, "no step from the pass's CC");
    b.pass(&[(74, 0), (75, 0)]);
    b.passes(40);
    assert_eq!(b.stored(P1, CUTOFF), spec(CUTOFF).min, "the next CC ramps");
    assert_eq!(b.stored(P1, TYPE), spec(TYPE).min, "the next CC steps");
}

const MODEL: ParamAddr = ParamAddr::new(
    BlockRef::TapeEngine,
    chimera_core::dsp::tape_engine::TapeEngineParams::MODEL,
);

/// TAPE MODEL changing every pass, by a mapped CC and by the hand, beside
/// ramping CC knobs: a MODEL's defaults count, so no publish carries more
/// than `MOVES - 1` knob moves, and the knobs still arrive.
#[test]
fn model_changes_never_overflow_a_publish() {
    use chimera_core::ui::block_registry::TAPE_DECK;
    let mut b = Bench::new(Sound::plain_init(EngineType::Tape));
    let knobs: Vec<&Knob> = KNOBS
        .iter()
        .filter(|k| {
            k.spec.kind == ParamKind::Continuous
                && k.addr.block != BlockRef::TapeEngine
                && k.addr.block.voice_reads()
                && Mappable::new(k.addr).is_some_and(|m| m.live(EngineType::Tape))
        })
        .take(8)
        .collect();
    assert!(knobs.len() >= 5);
    for (i, k) in knobs.iter().enumerate() {
        b.map(P1, 20 + i as u8, k.addr);
        let s = b.ui.project_mut().edit_part(P1).sound;
        let mut blk = s.params.block_mut(k.addr.block).unwrap();
        blk.set(k.addr.param, k.spec.denormalize(0.0));
    }
    b.map(P1, 30, MODEL);
    b.publish();
    b.reset_most();
    b.go_to_page(TAPE_DECK.id);
    let n = 400;
    for t in 0..=n {
        let v = (127 * t / n) as u8;
        let mut ccs: Vec<_> = (0..knobs.len())
            .map(|i| b.ev(P1, 20 + i as u8, v))
            .collect();
        // STRETCH and PING-PONG: two knob defaults differ.
        ccs.push(b.ev(P1, 30, if t % 2 == 0 { 64 } else { 127 }));
        // The hand turns MODEL every third pass, the CC the others.
        let turn = (t % 3 == 0).then(|| Input::turn(EncoderId::A, if t % 2 == 0 { 1 } else { -1 }));
        b.pass_with(ccs, turn, PASS_MS, PASS_BLOCKS);
        assert!(
            (b.most as usize) < MOVES,
            "pass {t}: a publish of {} moves",
            b.most
        );
    }
    b.passes(300);
    for k in &knobs {
        assert_eq!(b.place(P1, k.addr), 1.0, "{:?} arrives", k.addr);
    }
}

/// The audio takes every move published since it last took one: two UI
/// passes with no block between, the hand turning in the second, then a
/// block. The CCs left room for what was in flight, so the block never
/// takes more than `MOVES`.
#[test]
fn two_passes_in_one_block_never_overflow() {
    let mut b = Bench::algo();
    let knobs = eight_knobs();
    for (i, k) in knobs.iter().enumerate() {
        b.map(P1, 20 + i as u8, k.addr);
    }
    b.go_to_page(FILTER.id);
    b.publish();
    b.note();
    b.reset_most();
    for v in [0, 127, 0] {
        // Heard, so the next passes' ramps write every knob.
        let ccs = (0..knobs.len())
            .map(|i| b.ev(P1, 20 + i as u8, v))
            .collect();
        b.pass_with(ccs, None, PASS_MS, PASS_BLOCKS);
        b.pass_with(vec![], None, PASS_MS, 0);
        let turn = Input::turn(EncoderId::B, 1);
        b.pass_with(vec![], Some(turn), PASS_MS, 0);
        b.render();
        assert!(b.most as usize <= MOVES, "a block took {} moves", b.most);
        assert!(b.most as usize >= MOVES - 1, "the budget is spent");
    }
}

/// A CC in the pass that queues a LOAD (spec § 1): dropped, so the load's
/// witness holds and the project loads.
#[test]
fn a_cc_beside_a_queued_load_is_dropped() {
    let mut s = MemStore::new(1);
    let f = screen::put(&mut s, "NEXT");
    let mut ui = Box::new(crate::screen::stack_ui());
    let ch = ui.project().part(P1).mix.channel;
    let ev = |v| CcEvent {
        channel: ch,
        cc: Cc::new(74).unwrap(),
        value: v,
    };
    // INIT's CC 74 heard: CUTOFF ramps from the next pass.
    ui.hear_ccs(core::iter::once(ev(0)), Ms(1_000), true);
    let mut r = screen::Rig::new(&mut ui, s);
    r.to(&["PROJECT", "LOAD PROJECT"]);
    let l = r.ui.listing();
    let i = (0..l.len())
        .find(|&i| l.entry(i).unwrap().file() == f)
        .expect("listed");
    r.bar_to(i);
    // As the shells: the key queues the load, the CCs, then the card.
    screen::tap(r.ui, ButtonId::Edit);
    assert!(r.ui.job_queued_for_test());
    r.ui.hear_ccs(core::iter::once(ev(127)), Ms(1_010), true);
    r.work();
    assert_eq!(r.ui.project().meta().file(), Some(f), "loaded");
}

/// A CC steps op A's WAVE off FRMT on the active Part: the chain loses
/// its FRMT page, and the page is kept by id as the same encoder turn
/// keeps it.
#[test]
fn a_stepped_cc_keeps_the_page_as_a_turn_does() {
    use chimera_core::midi_map::ramp::bin;
    let on_op_a = || {
        let mut s = Sound::plain_init(EngineType::Algo);
        s.params.algo.try_set_kind(0, OpKind::Formant).unwrap();
        let mut b = Bench::new(s);
        screen::to_osc(&mut b.ui);
        screen::to_osc_sub(&mut b.ui, &ALGO_OP_PAGES[0]);
        b
    };
    let kind = |b: &Bench| b.ui.project().part(P1).sound.params.algo.kinds().get(0);
    let chain = |b: &Bench| PartChain::of(&b.ui.project().part(P1).sound);
    let mut turned = on_op_a();
    assert_eq!(chain(&turned), PartChain::AlgoFormant);
    feed(&mut turned.ui, Input::turn(EncoderId::A, -1));
    assert_eq!(chain(&turned), PartChain::Algo, "FRMT's page gone");
    let mut cc = on_op_a();
    cc.map(P1, 31, WAVE_A);
    let s = spec(WAVE_A);
    let steps = (s.max - s.min) as u16 + 1;
    let at = (cc.stored(P1, WAVE_A) - s.min) as u16;
    let v = (0..=127u8)
        .find(|&v| bin(v, steps, 0, 255, at) == at - 1)
        .unwrap();
    cc.pass(&[(31, v)]);
    assert_eq!(kind(&cc), kind(&turned));
    assert_eq!(cc.ui.page_def().id, ALGO_OP_PAGES[0].id, "kept by id");
    assert_eq!(cc.ui.location(), turned.ui.location());
    assert_eq!(cc.ui.page(), turned.ui.page());
}

/// `a` on Part 1 from where it rests to CC `cc`'s top: ramped over passes,
/// never a jump, and there.
fn ramps_up(b: &mut Bench, cc: u8, a: ParamAddr) {
    let mut was = b.place(P1, a);
    assert!(was < 1.0, "{a:?} rests below the top");
    b.pass(&[(cc, 127)]);
    let mut steps = 0;
    while b.place(P1, a) < 1.0 {
        b.pass(&[]);
        let now = b.place(P1, a);
        assert!(now >= was, "{a:?} never backs off");
        assert!(now - was < 0.5, "{a:?} jumped {} in a pass", now - was);
        steps += usize::from(now > was);
        was = now;
        assert!(steps < 40, "{a:?} never arrives");
    }
    assert!(steps >= 3, "{a:?} in {steps} steps: a jump, not a ramp");
}

/// INIT's default map (spec § 9): LEVEL, PAN, the sends and the wheel's
/// DEPTH each ramp to their CC.
#[test]
fn init_default_map_moves_its_targets() {
    use chimera_core::dsp::lfo::LfoParams;
    use chimera_core::dsp::modulator::LfoSlot;
    use chimera_core::params::OutParams;
    use chimera_core::part::PartParams;
    let mut b = Bench::new(Sound::init(EngineType::Algo));
    b.note();
    for (cc, a) in [
        (7, ParamAddr::new(BlockRef::Part, PartParams::LEVEL)),
        (10, ParamAddr::new(BlockRef::Part, PartParams::PAN)),
        (91, ParamAddr::new(BlockRef::Out, OutParams::SEND3)),
        (93, ParamAddr::new(BlockRef::Out, OutParams::SEND1)),
        (
            1,
            ParamAddr::new(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::DEPTH),
        ),
    ] {
        ramps_up(&mut b, cc, a);
    }
}

/// MIX + SEQ, then a turn moves the focus to cell `i`: armed on it.
fn arm_on(b: &mut Bench, i: usize) {
    use chimera_core::ui::learn::Learn;
    feed(
        &mut b.ui,
        Input::chord(ButtonId::Mix, ButtonId::Seq).at(10_000),
    );
    feed(
        &mut b.ui,
        Input::release(ButtonId::Seq)
            .and_held(ButtonId::Mix)
            .at(10_100),
    );
    feed(&mut b.ui, Input::release(ButtonId::Mix).at(10_133));
    let e = [
        EncoderId::A,
        EncoderId::B,
        EncoderId::C,
        EncoderId::D,
        EncoderId::E,
        EncoderId::F,
    ][i];
    feed(&mut b.ui, Input::turn(e, 1));
    assert!(matches!(b.ui.learn(), Learn::Armed { .. }));
    assert_eq!(b.ui.focused_slot(), i);
}

/// Learn end to end on a MODAL and an ALGO cell: armed on the cell, its
/// CC maps it, and the CC then ramps it.
#[test]
fn learned_cells_ramp() {
    use chimera_core::ui::block_registry::{ALGO_LEVEL, MODAL_1};
    let bright = ParamAddr::new(BlockRef::Modal, ModalParams::BRIGHT);
    let mut b = Bench::new(Sound::init(EngineType::Modal));
    b.go_to_page(MODAL_1.id);
    arm_on(&mut b, 2);
    b.pass(&[(40, 0)]);
    let map = b.ui.project().part(P1).sound.map;
    assert!(
        map.targets_of(Cc::new(40).unwrap())
            .any(|(_, e)| e.target.addr() == bright)
    );
    b.passes(20);
    ramps_up(&mut b, 40, bright);

    let level = ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::LEVEL);
    let mut b = Bench::new(Sound::init(EngineType::Algo));
    screen::to_osc(&mut b.ui);
    screen::to_osc_sub(&mut b.ui, &ALGO_LEVEL);
    arm_on(&mut b, 0);
    b.pass(&[(41, 0)]);
    let map = b.ui.project().part(P1).sound.map;
    assert!(
        map.targets_of(Cc::new(41).unwrap())
            .any(|(_, e)| e.target.addr() == level)
    );
    b.pass(&[(41, 0)]);
    b.passes(20);
    ramps_up(&mut b, 41, level);
}

/// One UI frame each `FRAME_MS` of 4 ms passes, VA's home showing WAVE1
/// at slot 0; WAVE1's played place and its cell's, after each frame.
fn frames(b: &mut Bench, n: u32, mut each: impl FnMut(u32, f32, f32)) {
    for f in 1..=n {
        b.passes((FRAME_MS / PASS_MS) as usize);
        b.ui.update(UiTick::for_test());
        each(
            f * FRAME_MS,
            b.played(WAVE1),
            b.ui.renderer.anim[0].current(),
        );
    }
}

/// VA, WAVE1 on CC 20 at 0, a note sounding, its cell shown and settled.
fn wave1_at_rest() -> Bench {
    let mut b = Bench::new(Sound::plain_init(EngineType::Va));
    b.map(P1, 20, WAVE1);
    b.pass(&[(20, 0)]);
    b.note();
    assert_eq!(b.ui.page_def().id, VA_HOME.id);
    frames(&mut b, 40, |_, _, _| {});
    assert_eq!(b.ui.renderer.anim[0].current(), 0.0);
    b
}

/// The cell's number: the place at the 127 steps a CC has.
fn shown(place: f32) -> u8 {
    (place * 127.0).round() as u8
}

/// A single CC after silence (spec § 5): the sound crosses in the 15 ms
/// floor plus the 30 ms knob glide, and the cell shows it as quickly,
/// lerped, never a snap: on the unit it crawled for a second.
#[test]
fn a_cc_jump_shows_as_fast_as_it_sounds() {
    let mut b = wave1_at_rest();
    b.pass(&[(20, 127)]);
    // Played: a 15 ms ramp, then the one-pole's τ.
    b.passes(((15 + 30) / PASS_MS) as usize);
    assert!(b.played(WAVE1) > 0.6, "{} at 45 ms", b.played(WAVE1));
    let mut first = None;
    frames(&mut b, 6, |ms, played, cell| {
        first.get_or_insert(cell);
        if ms >= 200 {
            assert_eq!(shown(played), 127, "played at {ms} ms");
            assert_eq!(shown(cell), 127, "shown {cell} at {ms} ms");
        }
    });
    assert!(first.unwrap() < 1.0, "the cell lerps, never snaps");
}

/// A sweep to each end at 5 ms a step: the cell shows the end within
/// 200 ms of the last CC, as the sound has it.
#[test]
fn a_swept_end_shows_promptly() {
    let mut b = wave1_at_rest();
    for (from, to) in [(0u8, 127u8), (127, 0)] {
        let up = to > from;
        for k in 0..=127u8 {
            let v = if up { k } else { 127 - k };
            b.pass_with(vec![b.ev(P1, 20, v)], None, 5, 4);
            if k % 10 == 0 {
                b.ui.update(UiTick::for_test());
            }
        }
        frames(&mut b, 4, |_, _, _| {});
        assert_eq!(shown(b.played(WAVE1)), to, "played");
        assert_eq!(shown(b.ui.renderer.anim[0].current()), to, "shown");
    }
}
