//! The sympathetic slot pool through the `Instrument` (exclusive-state
//! spec § 4): at most four Sympathetic notes ring a set; one with no slot
//! free plays bare and nothing is stolen (the owner's rule, 2026-09-29),
//! every lease comes home, and a note on a pool slot sounds as it would
//! alone.
mod common;
use common::{SR, peak, scope_writer};

use chimera_core::dsp::engines::SlotKind;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::modal::{ResonatorMode, take_cleared_bytes};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument, SYM_CLEAR_BUDGET};
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{MAX_MOD_SOURCES, ModSource, ModState, VCA};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::part::PartMode;
use chimera_core::scope::ScopeWriter;
use chimera_core::sym_alloc::{SYM_SLOTS, SymSlot};
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

type Bits = [u32; BLOCK_SIZE];

const FADE_BLOCKS: usize = Voice::FADE as usize / BLOCK_SIZE;
const SYM: SlotKind = SlotKind::Modal(ResonatorMode::Sympathetic);

fn modal(mode: ResonatorMode) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Modal);
    p.modal.mode = mode;
    p
}

fn sym() -> ParamSnapshot {
    modal(ResonatorMode::Sympathetic)
}

fn bits(b: &[f32; BLOCK_SIZE]) -> Bits {
    b.map(f32::to_bits)
}

/// The largest |x[n] − x[n−1]|.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

/// An `Instrument` whose Part `i` listens on channel `i`, with no CPU
/// ceiling: only the pool limits the notes.
struct Stage {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    shared: AudioShared,
}

impl Stage {
    fn new(parts: &[(ParamSnapshot, PartMode)]) -> Self {
        Self::with_budget(parts, SampleBudget::for_cpu(u32::MAX))
    }

    fn with_budget(parts: &[(ParamSnapshot, PartMode)], budget: SampleBudget) -> Self {
        let mut shared = AudioShared::default();
        for (part, (p, mode)) in shared.parts.iter_mut().zip(parts) {
            part.params = p.clone();
            part.mix.mode = *mode;
        }
        Self {
            inst: Box::new(Instrument::new(SR, budget)),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: scope_writer(),
            shared,
        }
    }

    fn event(&mut self, part: usize, n: u8, kind: NoteKind) {
        let ev = NoteEvent {
            channel: MidiChannel::new(part as u8).unwrap(),
            note: MidiNote::new(n).unwrap(),
            kind,
        };
        self.inst.handle(ev, &self.shared);
    }

    fn on(&mut self, part: usize, n: u8) {
        self.event(part, n, NoteKind::On(Velocity::new(100).unwrap()));
    }

    fn off(&mut self, part: usize, n: u8) {
        self.event(part, n, NoteKind::Off);
    }

    /// One block; then the pool's invariants: the voices ringing a set
    /// number exactly the leases out, at most four, and a voice the
    /// `Allocator` holds free holds nothing, lease or promise.
    fn block(&mut self) {
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
        let rings = self.inst.rings();
        let ringing = rings.iter().filter(|&&r| r).count();
        let pool = self.inst.sym();
        assert!(ringing <= SYM_SLOTS, "{ringing} voices ring");
        assert_eq!(ringing, pool.lent(), "ringing voices vs leases");
        for (v, s) in self.inst.allocator().slots().iter().enumerate() {
            if s.is_free() {
                assert!(!rings[v], "free voice {v} holds a lease");
                assert!(
                    !pool.promised(VoiceIdx::ALL[v]),
                    "a slot promised to free voice {v}"
                );
            }
        }
    }

    /// The booked, non-dying voices of `part`.
    fn voices_of(&self, part: usize) -> usize {
        let slots = self.inst.allocator().slots();
        slots
            .iter()
            .filter(|s| s.part() == Some(part as u8) && !s.dying())
            .count()
    }

    fn bus(&self, part: usize) -> Bits {
        bits(self.inst.part_bus(part))
    }

    /// The voice holding `n` for `part`.
    fn voice_of(&self, part: usize, n: u8) -> usize {
        let slots = self.inst.allocator().slots();
        (0..MAX_VOICES)
            .find(|&v| {
                let s = &slots[v];
                s.held() && s.part() == Some(part as u8) && s.note() == MidiNote::new(n)
            })
            .unwrap_or_else(|| panic!("no voice plays {n} for part {part}"))
    }

    /// Renders until every voice is free, at most `limit` blocks.
    fn until_idle(&mut self, limit: usize) {
        for _ in 0..limit {
            if self.inst.allocator().slots().iter().all(|s| s.is_free()) {
                return;
            }
            self.block();
        }
        panic!(
            "voices still booked after {limit} blocks: {:?} {:?}",
            self.inst.active(),
            self.inst.allocator().slots()
        );
    }
}

/// `part`'s bus from its first non-zero block, `blocks` long.
fn from_first_sound(stage: &mut Stage, part: usize, blocks: usize) -> Vec<Bits> {
    for _ in 0..200 {
        if stage.inst.part_bus(part).iter().any(|&s| s != 0.0) {
            let mut out = vec![stage.bus(part)];
            for _ in 1..blocks {
                stage.block();
                out.push(stage.bus(part));
            }
            return out;
        }
        stage.block();
    }
    panic!("part {part} never sounds");
}

/// Note `n` alone on `part` of a fresh `Instrument` playing `p`.
fn alone(part: usize, p: &ParamSnapshot, n: u8, blocks: usize) -> Vec<Bits> {
    let mut parts = vec![(sym(), PartMode::Poly); part + 1];
    parts[part] = (p.clone(), PartMode::Poly);
    let mut s = Stage::new(&parts);
    s.on(part, n);
    s.block();
    from_first_sound(&mut s, part, blocks)
}

/// Part 1 holds four Sympathetic notes, one a block from block 0, each
/// ringing a slot: the pool is full. Rendered to block 9.
fn four_held() -> Stage {
    let mut s = Stage::new(&vec![(sym(), PartMode::Poly); 3]);
    for (b, n) in [48, 50, 52, 53].into_iter().enumerate() {
        s.on(0, n);
        s.block();
        assert!(s.inst.rings()[s.voice_of(0, n)], "block {b}: {n} rings");
    }
    for _ in 4..10 {
        s.block();
    }
    s
}

/// With every slot held, a fifth Sympathetic note plays at once on a
/// voice of its own, bare: the main string alone.
#[test]
fn fifth_note_plays_bare_while_four_ring() {
    let mut s = four_held();
    s.on(1, 55);
    let v = s.voice_of(1, 55);
    s.block();
    assert_eq!(s.inst.slot_kinds()[v], SYM);
    assert!(s.inst.active()[v], "55 sounds at once");
    assert!(!s.inst.rings()[v], "55 is bare");
    assert_eq!(s.inst.sym().lent(), SYM_SLOTS);
    assert!(peak(s.inst.part_bus(1)) > 0.0);
    let bare = from_first_sound(&mut s, 1, 8);
    assert_ne!(bare, alone(1, &sym(), 55, 8), "no halo: not 55 on a slot");
}

/// A fifth note fades, steals and ends nothing: the four ringing notes'
/// bus is bit-identical to the same four with no fifth note.
#[test]
fn nothing_is_faded_or_stolen_for_the_pool() {
    let run = |fifth: bool| {
        let mut s = four_held();
        if fifth {
            s.on(1, 55);
        }
        let r = s.inst.rebuilds();
        let bus: Vec<Bits> = (0..20)
            .map(|_| {
                s.block();
                s.bus(0)
            })
            .collect();
        for n in [48, 50, 52, 53] {
            let v = s.voice_of(0, n);
            assert!(s.inst.active()[v] && s.inst.rings()[v], "{n} rings on");
            assert_eq!(s.inst.rebuilds()[v], r[v], "{n} is never rebuilt");
        }
        assert_eq!(s.inst.allocator().refused(), 0, "nothing refused or lost");
        bus
    };
    assert_eq!(run(true), run(false));
}

/// A slot freed while a bare note sounds goes to the next new note; the
/// bare note keeps playing bare.
#[test]
fn a_freed_slot_goes_to_the_next_new_note_not_a_ringing_bare_one() {
    let mut s = Stage::new(&vec![(sym(), PartMode::Poly); 2]);
    short_sym(&mut s, 2);
    for n in [48, 50, 52, 53] {
        s.on(0, n);
    }
    s.on(1, 55);
    s.block();
    let bare = s.voice_of(1, 55);
    assert!(!s.inst.rings()[bare]);
    s.off(0, 48);
    for _ in 0..4 {
        s.block();
    }
    assert_eq!(s.inst.sym().free(), 1, "48's slot is back");
    assert!(
        !s.inst.rings()[bare] && s.inst.active()[bare],
        "55 plays on, bare"
    );
    s.on(1, 57);
    s.block();
    let v = s.voice_of(1, 57);
    assert!(s.inst.rings()[v], "57 takes the freed slot");
    assert!(!s.inst.rings()[bare], "55 stays bare");
}

/// However long it rings, a bare note is never rebuilt to take a slot
/// that comes free: a halo is fixed when a note starts.
#[test]
fn a_bare_note_never_gains_a_halo() {
    let mut s = Stage::new(&vec![(sym(), PartMode::Poly); 2]);
    short_sym(&mut s, 1);
    for n in [48, 50, 52, 53] {
        s.on(0, n);
    }
    s.on(1, 55);
    s.block();
    let v = s.voice_of(1, 55);
    let r = s.inst.rebuilds()[v];
    for n in [48, 50, 52, 53] {
        s.off(0, n);
    }
    for _ in 0..100 {
        s.block();
        assert!(!s.inst.rings()[v], "55 gained a halo");
    }
    assert_eq!(s.inst.sym().free(), SYM_SLOTS);
    assert!(s.inst.active()[v], "55 still sounds");
    assert_eq!(s.inst.rebuilds()[v], r, "never rebuilt");
}

#[test]
fn other_models_keep_eight_voices() {
    for mode in [
        ResonatorMode::String,
        ResonatorMode::Bowed,
        ResonatorMode::Modal,
    ] {
        let mut s = Stage::new(&[(modal(mode), PartMode::Poly)]);
        for n in 0..MAX_VOICES as u8 {
            s.on(0, 60 + n);
        }
        for _ in 0..4 {
            s.block();
        }
        let slots = s.inst.allocator().slots();
        assert!(
            slots.iter().all(|v| v.held() && v.part() == Some(0)),
            "{mode:?}: eight notes booked"
        );
        assert_eq!(s.inst.active(), [true; MAX_VOICES], "{mode:?}: eight sound");
        assert!(
            s.inst
                .slot_kinds()
                .iter()
                .all(|&k| k == SlotKind::Modal(mode)),
            "{mode:?}"
        );
        assert!(peak(s.inst.part_bus(0)) > 0.0, "{mode:?}: the bus sounds");
        assert_eq!(s.inst.sym().free(), SYM_SLOTS, "{mode:?}: no slot taken");
    }
}

#[test]
fn four_sympathetic_notes_sound_as_alone() {
    let notes = [48, 55, 60, 67];
    let mut s = Stage::new(&vec![(sym(), PartMode::Poly); 4]);
    for (part, &n) in notes.iter().enumerate() {
        s.on(part, n);
    }
    let mut buses = vec![Vec::new(); 4];
    for _ in 0..16 {
        s.block();
        for (part, bus) in buses.iter_mut().enumerate() {
            bus.push(s.bus(part));
        }
    }
    assert_eq!(s.inst.sym().lent(), 4);
    for (part, &n) in notes.iter().enumerate() {
        let mut a = Stage::new(&vec![(sym(), PartMode::Poly); part + 1]);
        a.on(part, n);
        let want: Vec<Bits> = (0..16)
            .map(|_| {
                a.block();
                a.bus(part)
            })
            .collect();
        assert!(want.iter().flatten().any(|&x| x != 0), "{n} sounds");
        assert_eq!(buses[part], want, "part {part}, note {n}");
    }
}

#[test]
fn a_handed_over_slot_carries_nothing() {
    let mut s = Stage::new(&[(sym(), PartMode::Poly), (sym(), PartMode::Poly)]);
    s.on(0, 60);
    let a = s.voice_of(0, 60);
    for _ in 0..20 {
        s.block();
    }
    assert_eq!(s.inst.sym().holder(SymSlot::ALL[0]), Some(VoiceIdx::ALL[a]));
    s.off(0, 60);
    s.until_idle(5_000);
    assert_eq!(s.inst.sym().free(), SYM_SLOTS);

    s.off(0, 60);
    s.on(1, 67);
    let b = s.voice_of(1, 67);
    assert_ne!(a, b, "B lands on another voice");
    assert_eq!(
        s.inst.sym().holder(SymSlot::ALL[0]),
        Some(VoiceIdx::ALL[b]),
        "B takes A's slot"
    );
    let got: Vec<Bits> = (0..16)
        .map(|_| {
            s.block();
            s.bus(1)
        })
        .collect();
    let mut f = Stage::new(&[(sym(), PartMode::Poly), (sym(), PartMode::Poly)]);
    f.on(1, 67);
    let want: Vec<Bits> = (0..16)
        .map(|_| {
            f.block();
            f.bus(1)
        })
        .collect();
    assert!(want.iter().flatten().any(|&x| x != 0), "B sounds");
    assert_eq!(got, want);
}

#[test]
fn every_lease_comes_home() {
    let most = storm(SampleBudget::for_cpu(u32::MAX));
    assert_eq!(most, SYM_SLOTS, "the storm fills the pool");
    // The chip's ceiling sheds and refuses too.
    storm(SampleBudget::for_cpu(CPU_HZ_REV_V));
}

/// The storm; the most leases out at once.
fn storm(budget: SampleBudget) -> usize {
    const MODES: [ResonatorMode; 4] = [
        ResonatorMode::Modal,
        ResonatorMode::String,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ];
    let mut rng = 0xC0FF_EE11_u32;
    let mut next = |n: u32| {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng % n
    };
    let mut s = Stage::with_budget(&vec![(sym(), PartMode::Poly); 3], budget);
    let mut held: Vec<(usize, u8)> = Vec::new();
    let mut most = 0;
    for _ in 0..2_000 {
        if next(4) == 0 {
            let (part, n) = (next(3) as usize, 36 + next(49) as u8);
            s.on(part, n);
            held.push((part, n));
        }
        if next(4) == 0 && !held.is_empty() {
            let (part, n) = held.swap_remove(next(held.len() as u32) as usize);
            s.off(part, n);
        }
        if next(4) == 0 {
            let part = next(3) as usize;
            s.shared.parts[part].params.modal.mode = MODES[next(4) as usize];
        }
        if next(4) == 0 {
            let p = &mut s.shared.parts[next(3) as usize].params;
            let engine = match p.engine() {
                EngineType::Algo => EngineType::Modal,
                EngineType::Modal => EngineType::Algo,
            };
            let mode = p.modal.mode;
            *p = ParamSnapshot::for_engine(engine);
            p.modal.mode = mode;
        }
        s.block();
        most = most.max(s.inst.sym().lent());
    }
    for (part, n) in held {
        s.off(part, n);
    }
    s.until_idle(5_000);
    assert_eq!(s.inst.sym().free(), SYM_SLOTS);
    most
}

/// Part 1 holds String notes `notes`, one a block; each is booked on a
/// voice, oldest first. Part 2 (Sympathetic) holds `older` before them.
fn held_strings(older: &[u8], notes: &[u8]) -> Stage {
    let mut s = Stage::new(&[
        (modal(ResonatorMode::String), PartMode::Poly),
        (sym(), PartMode::Poly),
    ]);
    for &n in older {
        s.on(1, n);
        s.block();
    }
    for &n in notes {
        s.on(0, n);
        s.block();
    }
    s
}

/// A switch onto Sympathetic restarts every held note after its fade;
/// newest first they take the free slots, so the last four played ring
/// and the rest play bare. With Part 2 holding two slots, only the newest
/// two ring, and Part 2's notes ring on untouched: nothing is evicted.
#[test]
fn switch_gives_the_last_four_played_a_halo() {
    for (older, halos) in [(&[][..], 4), (&[72, 76][..], 2)] {
        let notes: Vec<u8> = (60..66).collect();
        let mut s = held_strings(older, &notes);
        s.block();
        let voices: Vec<usize> = notes.iter().map(|&n| s.voice_of(0, n)).collect();
        let holders: Vec<usize> = older.iter().map(|&n| s.voice_of(1, n)).collect();
        let before = s.inst.rebuilds();

        s.shared.parts[0].params = sym();
        for _ in 0..FADE_BLOCKS + 1 {
            s.block();
        }
        let (kinds, active, rings) = (s.inst.slot_kinds(), s.inst.active(), s.inst.rings());
        for (i, &v) in voices.iter().enumerate() {
            let n = notes[i];
            assert_eq!(kinds[v], SYM, "{n} restarts on Sympathetic");
            assert!(active[v], "{n} sounds");
            assert_eq!(rings[v], i >= notes.len() - halos, "{n}'s halo");
            let d = s.inst.rebuilds()[v].wrapping_sub(before[v]);
            assert_eq!(d, 2, "{n}: the fade end's rest, then the note");
        }
        for &h in &holders {
            assert!(active[h] && rings[h], "Part 2 rings on");
            assert_eq!(s.inst.rebuilds()[h], before[h], "Part 2 untouched");
        }
        assert_eq!(s.inst.allocator().refused(), 0, "nothing refused or lost");

        for &n in &notes {
            s.off(0, n);
        }
        for &n in older {
            s.off(1, n);
        }
        s.until_idle(5_000);
        assert_eq!(s.inst.sym().free(), SYM_SLOTS);
    }
}

#[test]
fn a_resting_voice_gives_its_slot_back() {
    let mut p = sym();
    p.modal.damp = 1.0 - 0.3;
    let mut s = Stage::new(&[(p, PartMode::Poly)]);
    s.on(0, 60);
    let v = s.voice_of(0, 60);
    s.block();
    assert_eq!(s.inst.sym().lent(), 1);
    // Released, it rings down on its own: no fade, no reset.
    s.off(0, 60);
    for _ in 0..5_000 {
        let r = s.inst.rebuilds()[v];
        s.block();
        if !s.inst.active()[v] {
            assert_eq!(s.inst.rebuilds()[v], r.wrapping_add(1), "one rest rebuild");
            assert_eq!(
                s.inst.slot_kinds()[v],
                SlotKind::Modal(ResonatorMode::String)
            );
            assert_eq!(s.inst.sym().free(), SYM_SLOTS);
            return;
        }
    }
    panic!("the note never ends");
}

/// A Mono Sympathetic Part with the pool full keeps its one voice: its
/// notes play bare there, retriggered in place, and take nothing from the
/// ringing Part.
#[test]
fn a_mono_part_keeps_one_voice() {
    let mut s = Stage::new(&[(sym(), PartMode::Mono), (sym(), PartMode::Poly)]);
    for n in [48, 50, 52, 53] {
        s.on(1, n);
        s.block();
    }
    s.on(0, 60);
    s.block();
    let m = s.voice_of(0, 60);
    let others = [48, 50, 52, 53].map(|n| s.voice_of(1, n));
    for n in [62, 64] {
        s.on(0, n);
        for _ in 0..FADE_BLOCKS + 2 {
            s.block();
            assert_eq!(s.voices_of(0), 1, "after {n}: one voice for Part 1");
        }
        assert_eq!(s.voice_of(0, n), m, "{n}: its own voice");
        assert!(s.inst.active()[m] && !s.inst.rings()[m], "{n} sounds bare");
        for &o in &others {
            assert!(s.inst.active()[o] && s.inst.rings()[o]);
        }
    }
}

/// Sympathetic ended by ENV 1 on the VCA at RELEASE 0: a note released
/// goes idle within a block, however low (a low loop filters too seldom
/// to fall silent soon on its own).
fn short_sym(s: &mut Stage, parts: usize) {
    let mut reg = ModDestRegistry::new();
    let _ = reg.add(VCA, *b"TEST\0\0\0\0");
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    let d = ms.find(VCA).unwrap();
    ms.set_route(ModSource::Env1.index(), d, 127);
    for part in &mut s.shared.parts[..parts] {
        part.params.envelopes[0].release = 0.0;
        part.mod_state = ms.clone();
    }
}

/// Four Parts on the short Sympathetic Sound, each having played `last`
/// once and gone idle, so each slot's lines are dirty to `last`'s loops.
fn after(last: u8) -> Stage {
    after_each([last; 4])
}

/// As `after`, Part `i` (and so slot `i`) having played `last[i]`.
fn after_each(last: [u8; 4]) -> Stage {
    let mut s = Stage::new(&vec![(sym(), PartMode::Poly); 4]);
    short_sym(&mut s, 4);
    for (part, &n) in last.iter().enumerate() {
        s.on(part, n);
    }
    s.block();
    for (part, &n) in last.iter().enumerate() {
        s.off(part, n);
    }
    s.until_idle(50);
    let _ = take_cleared_bytes();
    s
}

const CHORD: [u8; 4] = [40, 47, 52, 59];

/// The chord played on Parts 1 to 4 at once, `blocks` blocks: each
/// Part's bus, the bytes cleared each block and each voice's rebuilds.
fn chord(mut s: Stage, blocks: usize) -> (Vec<Vec<Bits>>, Vec<usize>, Vec<[u16; MAX_VOICES]>) {
    let (mut buses, mut cleared, mut rebuilds) = (vec![Vec::new(); 4], Vec::new(), Vec::new());
    for (part, &n) in CHORD.iter().enumerate() {
        s.on(part, n);
    }
    for _ in 0..blocks {
        let before = s.inst.rebuilds();
        s.block();
        cleared.push(take_cleared_bytes());
        let after = s.inst.rebuilds();
        rebuilds.push(core::array::from_fn(|v| after[v].wrapping_sub(before[v])));
        for (part, bus) in buses.iter_mut().enumerate() {
            bus.push(s.bus(part));
        }
    }
    assert_eq!(s.inst.allocator().refused(), 0, "nothing refused or lost");
    (buses, cleared, rebuilds)
}

/// The block each Part first sounds in.
fn starts(buses: &[Vec<Bits>]) -> Vec<usize> {
    buses
        .iter()
        .map(|b| {
            b.iter()
                .position(|x| x.iter().any(|&s| s != 0))
                .expect("sounds")
        })
        .collect()
}

/// Every slot last played at MIDI 0: each note-on clears eight whole
/// rings, so the budget takes one a block and the chord starts a note a
/// block, never clearing more than the budget in any block.
#[test]
fn a_worst_case_chord_starts_a_note_a_block() {
    let (buses, cleared, rebuilds) = chord(after(0), 8);
    let mut at = starts(&buses);
    at.sort_unstable();
    assert_eq!(at, [0, 1, 2, 3]);
    for (b, &c) in cleared.iter().enumerate() {
        assert!(c <= SYM_CLEAR_BUDGET, "block {b}: {c} > {SYM_CLEAR_BUDGET}");
    }
    assert!(
        cleared[..4].iter().all(|&c| c > SYM_CLEAR_BUDGET * 3 / 4),
        "{cleared:?}"
    );
    for r in &rebuilds {
        assert!(r.iter().all(|&n| n <= 3), "{r:?}");
    }
}

/// Every slot last played at A4: four small clears fit one block, so the
/// chord starts at once.
#[test]
fn a_typical_chord_starts_at_once() {
    let (buses, cleared, _) = chord(after(69), 4);
    assert_eq!(starts(&buses), [0; 4]);
    assert!(cleared[0] <= SYM_CLEAR_BUDGET / 2, "{cleared:?}");
}

/// A note that waited plays as it would alone, from its first sound, and
/// starts from silence without a click: its step from the silence before
/// it is its own first step, as on a fresh `Instrument`.
#[test]
fn a_spread_chord_plays_each_note_as_alone() {
    let (buses, ..) = chord(after(0), 24);
    for (part, bus) in buses.iter().enumerate() {
        let from = starts(&buses)[part];
        let mut a = Stage::new(&vec![(sym(), PartMode::Poly); 4]);
        short_sym(&mut a, 4);
        a.on(part, CHORD[part]);
        a.block();
        let want = from_first_sound(&mut a, part, bus.len() - from);
        assert_eq!(bus[from..], want[..], "part {part}");

        let samples =
            |b: &[Bits]| -> Vec<f32> { b.iter().flatten().map(|&x| f32::from_bits(x)).collect() };
        let mut alone = vec![0.0];
        alone.extend(samples(&want));
        assert!(
            max_step(&samples(bus)) <= max_step(&alone),
            "part {part}: a click at its delayed start"
        );
    }
}

/// The pool full, a note on Part 2 waits for Part 3's fade on its voice;
/// then Part 2 becomes Sympathetic. `restart_switched` passes that voice
/// by (it sounds Part 3), so the note is placed when it starts: no slot
/// is free, so it plays bare. It never sits booked, held and silent.
#[test]
fn a_note_waiting_across_a_switch_to_sympathetic_is_placed() {
    let mut s = Stage::new(&[
        (sym(), PartMode::Poly),
        (modal(ResonatorMode::String), PartMode::Poly),
        (modal(ResonatorMode::String), PartMode::Poly),
    ]);
    // Part 3's four notes, the oldest, then Part 1's four Sympathetic:
    // eight voices, the pool full.
    for n in [36, 40, 43, 47] {
        s.on(2, n);
    }
    s.block();
    for n in [60, 64, 67, 71] {
        s.on(0, n);
    }
    s.block();
    assert_eq!(s.inst.sym().lent(), SYM_SLOTS);
    s.on(1, 50);
    let v = s.voice_of(1, 50);
    assert!(
        s.inst.active()[v],
        "50 waits for Part 3's fade on its voice"
    );
    s.shared.parts[1].params = sym();
    let mut sounded = false;
    for _ in 0..12 {
        s.block();
        sounded |= peak(s.inst.part_bus(1)) > 0.0;
    }
    let held_silent = (0..MAX_VOICES).any(|u| {
        let slot = s.inst.allocator().slots()[u];
        slot.held() && slot.part() == Some(1) && !s.inst.active()[u]
    });
    assert!(
        sounded || s.inst.allocator().refused() > 0,
        "50 sounds or is counted"
    );
    assert!(!held_silent, "no voice booked, held and silent");
}

/// Notes wait on the clear budget in the order they came: a small clear
/// behind two deferred worst-case ones doesn't start before them, in the
/// drain or after it.
#[test]
fn notes_waiting_on_the_clear_budget_keep_their_order() {
    let (buses, cleared, _) = chord(after_each([0, 0, 0, 69]), 8);
    assert_eq!(starts(&buses), [0, 1, 2, 2]);
    for (b, &c) in cleared.iter().enumerate() {
        assert!(c <= SYM_CLEAR_BUDGET, "block {b}: {c}");
    }
}

/// As above, but with every slot free: the waiting note, placed as it
/// starts on its now Sympathetic Part, rings.
#[test]
fn a_note_waiting_across_a_switch_takes_a_free_slot() {
    let mut s = Stage::new(&[
        (modal(ResonatorMode::String), PartMode::Poly),
        (modal(ResonatorMode::String), PartMode::Poly),
        (modal(ResonatorMode::String), PartMode::Poly),
    ]);
    for n in [36, 40, 43, 47] {
        s.on(2, n);
    }
    s.block();
    for n in [60, 64, 67, 71] {
        s.on(0, n);
    }
    s.block();
    assert_eq!(s.inst.sym().lent(), 0);
    s.on(1, 50);
    let v = s.voice_of(1, 50);
    assert!(
        s.inst.active()[v],
        "50 waits for Part 3's fade on its voice"
    );
    s.shared.parts[1].params = sym();
    for _ in 0..FADE_BLOCKS + 3 {
        s.block();
    }
    assert!(s.inst.active()[v], "50 sounds");
    assert!(s.inst.rings()[v], "50 started with slots free: it rings");
}

/// Round-robin wrapped, so the chord lands on voices 6, 7, 0 and 1: the
/// budget's queue still starts them in the order they came, not by voice.
#[test]
fn the_budget_queue_is_served_by_age_not_voice_index() {
    let mut parts = vec![(sym(), PartMode::Poly); 4];
    parts.push((ParamSnapshot::for_engine(EngineType::Algo), PartMode::Poly));
    let mut s = Stage::new(&parts);
    short_sym(&mut s, 4);
    for part in 0..4 {
        s.on(part, 0);
    }
    s.block();
    for part in 0..4 {
        s.off(part, 0);
    }
    s.until_idle(50);
    // Two Algo notes move round-robin on by two.
    s.on(4, 60);
    s.on(4, 62);
    s.block();
    s.off(4, 60);
    s.off(4, 62);
    s.until_idle(20_000);
    let _ = take_cleared_bytes();
    let (buses, cleared, _) = chord(s, 8);
    assert_eq!(starts(&buses), [0, 1, 2, 3]);
    assert!(
        cleared.iter().all(|&c| c <= SYM_CLEAR_BUDGET),
        "{cleared:?}"
    );
}

/// MODE flipped Sympathetic → String → Sympathetic, each within the last
/// flip's fade, over four held low notes: every restart goes through the
/// clear budget, so no block clears more than it, and all four ring again.
#[test]
fn a_mode_toggle_storm_keeps_the_clear_budget() {
    let mut s = Stage::new(&[(sym(), PartMode::Poly)]);
    for n in [0, 1, 2, 3] {
        s.on(0, n);
    }
    for _ in 0..6 {
        s.block();
    }
    let _ = take_cleared_bytes();
    let mut cleared = Vec::new();
    for b in 0..40 {
        s.shared.parts[0].params = match b {
            0 | 3 | 5 => modal(ResonatorMode::String),
            _ => sym(),
        };
        s.block();
        cleared.push(take_cleared_bytes());
    }
    for (b, &c) in cleared.iter().enumerate() {
        assert!(c <= SYM_CLEAR_BUDGET, "block {b}: {c} > {SYM_CLEAR_BUDGET}");
    }
    let voices = [0, 1, 2, 3].map(|n| s.voice_of(0, n));
    for v in voices {
        assert!(
            s.inst.active()[v] && s.inst.rings()[v],
            "voice {v} rings again"
        );
    }
    assert_eq!(s.inst.allocator().refused(), 0, "nothing refused or lost");
}

/// Four held String notes, on Parts 1 to 4 and on voices 6, 7, 0 and 1
/// (round-robin wrapped), switch to Sympathetic together over slots last
/// played low: their fades end in one block, and the budget lets their
/// restarts in oldest first, a block apart, not by voice index.
#[test]
fn restarts_whose_fades_end_together_start_oldest_first() {
    let mut parts = vec![(sym(), PartMode::Poly); 4];
    parts.push((ParamSnapshot::for_engine(EngineType::Algo), PartMode::Poly));
    let mut s = Stage::new(&parts);
    short_sym(&mut s, 4);
    for part in 0..4 {
        s.on(part, 0);
    }
    s.block();
    for part in 0..4 {
        s.off(part, 0);
    }
    s.until_idle(50);
    s.on(4, 60);
    s.on(4, 62);
    s.block();
    s.off(4, 60);
    s.off(4, 62);
    s.until_idle(20_000);
    for part in &mut s.shared.parts[..4] {
        part.params.modal.mode = ResonatorMode::String;
    }
    for (part, &n) in CHORD.iter().enumerate() {
        s.on(part, n);
    }
    s.block();
    let voices: Vec<usize> = CHORD
        .iter()
        .enumerate()
        .map(|(p, &n)| s.voice_of(p, n))
        .collect();
    assert_eq!(voices, [6, 7, 0, 1], "round-robin wrapped");
    let _ = take_cleared_bytes();
    for part in &mut s.shared.parts[..4] {
        part.params.modal.mode = ResonatorMode::Sympathetic;
    }
    let mut rang = [None; 4];
    for b in 0..FADE_BLOCKS + 8 {
        s.block();
        assert!(take_cleared_bytes() <= SYM_CLEAR_BUDGET, "block {b}");
        for (i, &v) in voices.iter().enumerate() {
            if rang[i].is_none() && s.inst.rings()[v] {
                rang[i] = Some(b);
            }
        }
    }
    let rang = rang.map(|b| b.expect("each restart rings"));
    assert!(rang.windows(2).all(|w| w[1] == w[0] + 1), "{rang:?}");
}
