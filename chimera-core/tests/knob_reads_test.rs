//! The audio takes only the moves a publish's delta carries (ADR 0115,
//! 0116), and reads no knob: a knob turn is one move of that Part; an FX,
//! mix or ORBIT edit, none; a Part that starts playing, none; a new Sound,
//! none (it lands). Six Parts, published from a Project each block as the
//! shells do, counted in moves taken.

mod common;

use chimera_core::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::knobs::{KNOBS, KnobPublisher};
use chimera_core::hw::{AUDIO_BUDGET_PERCENT, MAX_PARTS, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::project::{PartFrom, PartId, PartSource, Project, ReplaceGuard, TemplateCrc};
use chimera_core::scope::ScopeWriter;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::SAMPLE_RATE;

/// The most moves a Part takes in a block: past it, it lands.
const MOVES: u32 = chimera_core::dsp::knobs::MOVES as u32;

struct Six {
    p: Box<Project>,
    _t: TemplateCrc,
    shared: Box<AudioShared>,
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    knobs: Box<KnobPublisher>,
}

impl Six {
    /// Six Parts on ALGO INIT, sustained; room for every voice.
    fn new() -> Self {
        let (mut p, t) = Project::boxed();
        for part in PartId::ALL {
            let mut s = Sound::init(EngineType::Algo);
            s.params.envelopes[0].sustain = 1.0;
            **p.edit_part(part).sound = s;
        }
        let mut r = Self {
            p,
            _t: t,
            shared: Box::new(AudioShared::default()),
            inst: Box::new(Instrument::new(
                SAMPLE_RATE,
                SampleBudget::for_cpu(u32::MAX),
            )),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: common::scope_writer(),
            knobs: Box::new(KnobPublisher::new()),
        };
        r.shared.update_from_project(&r.p, 0, &mut r.knobs);
        r
    }

    fn key(&mut self, part: usize, on: bool) {
        let ev = NoteEvent {
            channel: self.shared.parts[part].mix.channel,
            note: MidiNote::new(57).unwrap(),
            kind: if on {
                NoteKind::On(Velocity::DEFAULT)
            } else {
                NoteKind::Off
            },
        };
        self.inst.handle(ev, &self.shared);
    }

    /// One block, published first: each Part's knobs read in it.
    fn block(&mut self) -> [u32; MAX_PARTS] {
        self.shared.update_from_project(&self.p, 0, &mut self.knobs);
        let before: [u32; MAX_PARTS] = core::array::from_fn(|p| self.inst.knob_reads(p));
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
        core::array::from_fn(|p| self.inst.knob_reads(p).wrapping_sub(before[p]))
    }

    /// A publish with no render: the audio skips it.
    fn publish(&mut self) {
        self.shared.update_from_project(&self.p, 0, &mut self.knobs);
    }

    /// Part `part` loads a new Sound (INIT), as the browser does.
    fn load(&mut self, part: usize) {
        let src = PartSource {
            part: PartId::ALL[part],
            from: PartFrom::Init(EngineType::Algo),
        };
        let c = ReplaceGuard::check(&self.p, self._t, src)
            .unwrap_or_else(|n| n.into_pending().anyway(&self.p));
        self.p.replace_part(c).unwrap();
    }

    /// Whether every knob of Part `part` has landed: none moves, so its
    /// voices play the stored values.
    fn landed(&self, part: usize) -> bool {
        KNOBS
            .iter()
            .all(|k| self.inst.knob(part, k.addr).map(|k| k.1) == Some(false))
    }

    /// `n` blocks: the most knobs read in any one of them.
    fn blocks(&mut self, n: usize) -> u32 {
        (0..n).map(|_| self.block().iter().sum()).max().unwrap_or(0)
    }

    fn playing(&self) -> usize {
        self.inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| !s.is_free())
            .count()
    }
}

/// With six Parts playing, a knob turn reads that one knob of that Part;
/// an FX, mix or ORBIT edit, none.
#[test]
fn an_edit_re_reads_only_the_edited_part() {
    let mut r = Six::new();
    for p in 0..MAX_PARTS {
        r.key(p, true);
    }
    r.blocks(50);
    assert_eq!(r.playing(), MAX_PARTS);
    assert_eq!(r.blocks(20), 0, "settled, nothing reads");

    r.p.edit_part(PartId::ALL[2]).sound.params.filter.cutoff *= 2.0;
    assert_eq!(r.block(), [0, 0, 1, 0, 0, 0]);
    assert_eq!(r.blocks(400), 0, "the glide reads nothing more");
    {
        let s = r.p.edit_part(PartId::ALL[4]).sound;
        s.params.filter.cutoff *= 0.5;
        s.params.filter.resonance = 0.3;
    }
    assert_eq!(r.block(), [0, 0, 0, 0, 2, 0], "two knobs, two reads");

    r.p.edit_fx().reverb.mix = 0.5;
    assert_eq!(r.block(), [0; MAX_PARTS], "an FX edit");
    r.p.edit_part(PartId::ALL[3]).mix.level = 0.5;
    assert_eq!(r.block(), [0; MAX_PARTS], "a mix edit");
    let _ = r.p.orbit_mut();
    assert_eq!(r.block(), [0; MAX_PARTS], "an ORBIT edit");
}

/// ORBIT starting three silent Parts in one block reads nothing: each
/// read its changes while silent, a Sound's worth a block at most.
#[test]
fn parts_starting_together_read_nothing() {
    let mut r = Six::new();
    assert_eq!(r.blocks(20), 0, "silent, settled: nothing");
    for p in [1, 3, 4] {
        r.p.edit_part(PartId::ALL[p]).sound.params.filter.cutoff *= 0.5;
    }
    assert_eq!(r.block(), [0, 1, 0, 1, 1, 0], "an edit, a knob");
    for p in [1, 3, 4] {
        r.key(p, true);
    }
    assert_eq!(r.block(), [0; MAX_PARTS], "the start block reads nothing");
    assert_eq!(r.playing(), 3);
}

/// A Part that falls silent mid-glide and plays again on an unchanged
/// Sound lands on the stored values, bit for bit, reading nothing.
#[test]
fn a_part_back_from_silence_plays_the_stored_values() {
    let mut r = Six::new();
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    // A short release: silent before the glide lands.
    {
        let s = r.p.edit_part(PartId::ALL[0]).sound;
        s.params.envelopes[0].release = 0.0;
        for o in &mut s.params.algo.ops {
            o.rr = 15;
        }
    }
    r.blocks(5);
    r.key(0, true);
    r.blocks(50);
    // Two octaves down, in range.
    r.p.edit_part(PartId::ALL[0]).sound.params.filter.cutoff *= 0.25;
    r.block();
    r.block();
    assert_eq!(r.inst.knob(0, cutoff).map(|k| k.1), Some(true), "mid-glide");
    r.key(0, false);
    let mut n = 0;
    while r.playing() > 0 {
        r.block();
        n += 1;
        assert!(n < 5000, "the note never ends");
    }
    assert_eq!(
        r.inst.knob(0, cutoff).map(|k| k.1),
        Some(true),
        "silent mid-glide"
    );
    r.blocks(5);
    r.key(0, true);
    // Its first block may wait on the voice's clear: two.
    assert_eq!(r.blocks(2), 0, "nothing read");
    let v = r
        .inst
        .allocator()
        .slots()
        .iter()
        .position(|s| !s.is_free())
        .unwrap();
    let stored = &r.p.part(PartId::ALL[0]).sound.params;
    for k in &KNOBS {
        let s = stored.block(k.addr.block).unwrap().get(k.addr.param);
        let played = r.inst.played(k.addr)[v];
        assert_eq!(
            played.to_bits(),
            s.to_bits(),
            "{:?} {}",
            k.addr.block,
            k.spec.label
        );
    }
}

/// A Sound loaded into a playing Part lands at once, taking no move; five
/// at once too, and a knob turn on another playing Part in the same block
/// is its one move (ADR 0116).
#[test]
fn loads_into_playing_parts_land_at_once() {
    let mut r = Six::new();
    for p in 0..MAX_PARTS {
        r.key(p, true);
    }
    r.blocks(50);
    r.p.edit_part(PartId::ALL[1]).sound.params.filter.cutoff *= 0.5;
    r.block();
    assert!(!r.landed(1), "mid-glide");
    r.load(1);
    assert_eq!(r.block(), [0; MAX_PARTS]);
    assert!(r.landed(1));
    for p in 1..MAX_PARTS {
        r.load(p);
    }
    r.p.edit_part(PartId::ALL[0]).sound.params.filter.cutoff *= 0.5;
    assert_eq!(r.block(), [1, 0, 0, 0, 0, 0]);
    assert!((1..MAX_PARTS).all(|p| r.landed(p)));
}

/// Publishes the audio never sees (1 to 9 between two renders, past the
/// ring) lose nothing: settled, every knob of a playing Part is the
/// stored value, bit for bit.
#[test]
fn skipped_publishes_lose_nothing() {
    let mut r = Six::new();
    r.key(0, true);
    r.blocks(50);
    for skip in 1..=9usize {
        for round in 0..6 {
            for e in 0..skip {
                let k = &KNOBS[(skip * 7 + round * 3 + e * 5) % KNOBS.len()];
                {
                    let s = r.p.edit_part(PartId::ALL[0]).sound;
                    let b = s.params.block_mut(k.addr.block).unwrap();
                    let v = k.spec.denormalize(0.2 + 0.1 * ((e + round) % 6) as f32);
                    b.set(k.addr.param, k.spec.quantize(v));
                }
                r.publish();
            }
            r.block();
        }
        r.blocks(600);
        assert!(r.landed(0), "skipping {skip}");
    }
}

/// The knobs are reserved once, beside the bus: a budget of the bus, the
/// knobs' reserve and one voice with its Part's knobs admits it; a cycle a
/// sample less refuses it.
#[test]
fn the_knobs_are_reserved_once() {
    use chimera_core::dsp::knobs::Knobs;
    use chimera_core::dsp::voice::Voice;
    let mut r = Six::new();
    r.publish();
    let part = &r.shared.parts[0];
    let voice = Voice::cost(&part.params, &part.mod_state).0;
    let need = FxBus::COST.0 + Knobs::RESERVE.0 + voice + Knobs::COST.0;
    for (cost, admitted) in [(need, 1), (need - 1, 0)] {
        let hz = (cost as u64 * 4_800_000).div_ceil(AUDIO_BUDGET_PERCENT as u64) as u32;
        *r.inst = Instrument::new(SAMPLE_RATE, SampleBudget::for_cpu(hz));
        r.key(0, true);
        assert_eq!(r.playing(), admitted, "budget {cost}");
    }
}

/// The bench's one-Part publish (`publish_sound`) is the publisher's path:
/// a Sound loaded into a playing Part again and again takes no move and
/// lands; a knob turned through it, one.
#[test]
fn a_one_part_publish_loads_and_turns() {
    use chimera_core::project::Loads;
    use chimera_core::tracked::Tracked;
    let mut r = Six::new();
    r.key(5, true);
    r.blocks(50);
    let mut s = Tracked::fresh(Sound::init(EngineType::Algo));
    let mut loads = Loads::NONE;
    for b in 0..120 {
        if b % 12 == 0 {
            s.params.filter.cutoff = if b % 24 == 0 { 500.0 } else { 2000.0 };
            loads = loads.next();
        }
        r.shared.publish_sound(5, &s, loads, &mut r.knobs);
        let before = r.inst.knob_reads(5);
        r.inst
            .render(&mut r.fx, &mut r.dac, &r.shared, &mut r.scope);
        assert_eq!(r.inst.knob_reads(5), before, "a load: no move");
    }
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    assert_eq!(r.inst.knob(5, cutoff).map(|k| k.1), Some(false));
    s.params.filter.cutoff = 600.0;
    r.shared.publish_sound(5, &s, loads, &mut r.knobs);
    let before = r.inst.knob_reads(5);
    r.inst
        .render(&mut r.fx, &mut r.dac, &r.shared, &mut r.scope);
    assert_eq!(
        r.inst.knob_reads(5).wrapping_sub(before),
        1,
        "a turn: one move"
    );
    assert_eq!(r.inst.knob(5, cutoff).map(|k| k.1), Some(true));
}

/// The knobs say when nothing moves: what the bench waits on before
/// timing. A turn moves until its glide lands.
#[test]
fn the_knobs_say_when_they_have_settled() {
    let mut r = Six::new();
    r.key(3, true);
    r.blocks(5);
    assert!(r.inst.knobs_settled());
    r.p.edit_part(PartId::ALL[3]).sound.params.filter.cutoff *= 0.5;
    r.block();
    assert!(!r.inst.knobs_settled(), "a glide");
    let mut n = 1;
    while !r.inst.knobs_settled() {
        r.block();
        n += 1;
        assert!(n < 1000, "never lands");
    }
}

/// A Part takes at most `MOVES` moves a block; past them, it lands.
#[test]
fn a_block_takes_at_most_a_window_of_moves() {
    let mut r = Six::new();
    r.key(0, true);
    r.blocks(20);
    let mods = KNOBS
        .iter()
        .filter(|k| matches!(k.addr.block, BlockRef::Modal | BlockRef::Filter));
    for (i, k) in mods.enumerate() {
        let s = r.p.edit_part(PartId::ALL[0]).sound;
        let b = s.params.block_mut(k.addr.block).unwrap();
        let v = k.spec.quantize(k.spec.denormalize(0.13 + 0.07 * i as f32));
        b.set(k.addr.param, v);
        r.publish();
    }
    let b = r.block();
    assert!(b[0] <= MOVES, "{b:?}");
    assert!(r.landed(0), "past the window: landed");
}
