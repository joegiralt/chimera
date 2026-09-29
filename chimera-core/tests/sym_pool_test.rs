//! The sympathetic slot pool through the `Instrument` (exclusive-state
//! spec § 4): at most four Sympathetic notes, the oldest yields to a new
//! one (the Rings rule), every lease comes home, and a note on a pool slot
//! sounds as it would alone.
mod common;
use common::{SR, peak, scope_writer};

use chimera_core::dsp::engines::SlotKind;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{MAX_VOICES, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
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
    dac: DacOut,
    scope: ScopeWriter,
    shared: AudioShared,
}

impl Stage {
    fn new(parts: &[(ParamSnapshot, PartMode)]) -> Self {
        let mut shared = AudioShared::default();
        for (part, (p, mode)) in shared.parts.iter_mut().zip(parts) {
            part.params = p.clone();
            part.mix.mode = *mode;
        }
        Self {
            inst: Box::new(Instrument::new(SR, SampleBudget::for_cpu(u32::MAX))),
            fx: Box::new(FxBus::new()),
            dac: [[0.0; BLOCK_SIZE * 2]; _],
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

    /// One block; then the pool's invariant: the voices holding a
    /// Sympathetic slot number exactly the leases out, at most four.
    fn block(&mut self) {
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
        let sym = self.sym_voices();
        assert!(sym <= SYM_SLOTS, "{sym} Sympathetic voices");
        assert_eq!(sym, self.inst.sym().lent(), "Sympathetic voices vs leases");
    }

    fn sym_voices(&self) -> usize {
        self.inst.slot_kinds().iter().filter(|&&k| k == SYM).count()
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

/// The four held notes, one a block from block 0, Part 1 holding 48 (the
/// oldest) and Part 3 the rest; rendered to block 9.
fn four_held() -> Stage {
    let mut s = Stage::new(&vec![(sym(), PartMode::Poly); 3]);
    let held = [(0, 48), (2, 50), (2, 52), (2, 53)];
    for b in 0..10 {
        if let Some(&(part, n)) = held.get(b) {
            s.on(part, n);
        }
        s.block();
    }
    s
}

/// Part 1's bus from the last sample of block 9 through the fade.
fn through_the_fade(s: &mut Stage) -> Vec<f32> {
    let mut x = vec![s.inst.part_bus(0)[BLOCK_SIZE - 1]];
    for _ in 0..FADE_BLOCKS {
        s.block();
        x.extend_from_slice(s.inst.part_bus(0));
    }
    x
}

/// Part 1's bus is the stolen voice alone, so the fade's step has
/// `switch_never_clicks`' bound: |Δ(x·g)| ≤ |Δx|·g + |x|·|Δg|, with x the
/// same blocks unstolen. (A plucked Sympathetic note still grows at block
/// 9, so block 9's step and peak don't bound the next two.)
#[test]
fn a_fifth_sympathetic_note_steals_the_oldest() {
    let x = through_the_fade(&mut four_held());
    let bound = max_step(&x) + peak(&x) / Voice::FADE as f32;

    let mut s = four_held();
    assert!(
        peak(s.inst.part_bus(0)) > 0.0,
        "Part 1 sounds before the steal"
    );
    let u = s.voice_of(0, 48);
    let r = s.inst.rebuilds()[u];
    s.on(1, 55);
    assert_eq!(s.voice_of(1, 55), u, "55 is booked on the oldest's voice");
    let faded = through_the_fade(&mut s);
    let worst = max_step(&faded);
    assert!(worst <= bound, "Part 1's step {worst} > {bound}");
    assert_eq!(*faded.last().unwrap(), 0.0, "the fade ends silent");
    assert_eq!(
        s.inst.rebuilds()[u],
        r.wrapping_add(2),
        "the fade end rests, then 55 builds"
    );
    assert_eq!(s.inst.slot_kinds()[u], SYM, "55 sounds on the stolen voice");

    let got = from_first_sound(&mut s, 1, 8);
    assert_eq!(got, alone(1, &sym(), 55, 8), "55 as on a fresh Instrument");
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
    let mut s = Stage::new(&vec![(sym(), PartMode::Poly); 3]);
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
    assert_eq!(most, SYM_SLOTS, "the storm fills the pool");
    for (part, n) in held {
        s.off(part, n);
    }
    s.until_idle(5_000);
    assert_eq!(s.inst.sym().free(), SYM_SLOTS);
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

#[test]
fn the_last_four_played_restart_through_the_instrument() {
    let notes: Vec<u8> = (60..68).collect();
    let mut s = held_strings(&[], &notes);
    s.block();
    s.block();
    let voices: Vec<usize> = notes.iter().map(|&n| s.voice_of(0, n)).collect();
    let before = s.inst.rebuilds();

    s.shared.parts[0].params = sym();
    for _ in 0..FADE_BLOCKS + 1 {
        s.block();
    }
    let (kinds, active, slots) = (
        s.inst.slot_kinds(),
        s.inst.active(),
        s.inst.allocator().slots(),
    );
    for (i, &v) in voices.iter().enumerate() {
        if i >= 4 {
            assert_eq!(kinds[v], SYM, "{} restarts", notes[i]);
            assert!(active[v], "{} sounds", notes[i]);
            let d = s.inst.rebuilds()[v].wrapping_sub(before[v]);
            assert_eq!(d, 2, "{}: the rest, then the note", notes[i]);
        } else {
            assert!(!active[v], "{} is silent", notes[i]);
            assert!(slots[v].held(), "{} is still held", notes[i]);
        }
    }

    // 60 again: it sounds, on the voice of 64, now the oldest slot.
    s.off(0, 60);
    s.on(0, 60);
    let u = voices[4];
    assert_eq!(s.voice_of(0, 60), u, "60 plays on 64's voice");
    for _ in 0..FADE_BLOCKS {
        assert!(s.inst.active()[u], "64 fades over FADE");
        s.block();
    }
    s.block();
    assert_eq!(s.inst.slot_kinds()[u], SYM);
    assert!(s.inst.active()[u], "60 sounds");

    for &n in &notes {
        s.off(0, n);
    }
    s.until_idle(5_000);
    assert_eq!(s.inst.sym().free(), SYM_SLOTS);
}

/// The second half of the one above: Part 2's two older notes hold two
/// slots, the restarts evict them, and each claimer starts at most one
/// block after its eviction's fade ends, never on a slot still lent.
#[test]
fn the_last_four_played_evict_older_holders() {
    let notes: Vec<u8> = (60..66).collect();
    let mut s = held_strings(&[40, 43], &notes);
    let evicted = [s.voice_of(1, 40), s.voice_of(1, 43)];
    let claimers = [s.voice_of(0, 62), s.voice_of(0, 63)];
    let silent = [s.voice_of(0, 60), s.voice_of(0, 61)];
    assert_eq!(s.inst.sym().lent(), 2);

    s.shared.parts[0].params = sym();
    let mut ended = [None; 2];
    let mut started = [None; 2];
    for b in 0..FADE_BLOCKS + 4 {
        s.block();
        let (kinds, active) = (s.inst.slot_kinds(), s.inst.active());
        for i in 0..2 {
            if ended[i].is_none() && !active[evicted[i]] {
                ended[i] = Some(b);
            }
            if started[i].is_none() && kinds[claimers[i]] == SYM {
                assert_ne!(kinds[evicted[i]], SYM, "claimer {i} on a lent slot");
                started[i] = Some(b);
            }
        }
    }
    for i in 0..2 {
        let (e, st) = (ended[i].expect("evicted"), started[i].expect("restarted"));
        assert!(
            st >= e && st <= e + 1,
            "claimer {i}: ended {e}, started {st}"
        );
    }
    let active = s.inst.active();
    assert!(silent.iter().all(|&v| !active[v]), "60 and 61 fall silent");

    for &n in &notes {
        s.off(0, n);
    }
    s.off(1, 40);
    s.off(1, 43);
    s.until_idle(5_000);
    assert_eq!(s.inst.sym().free(), SYM_SLOTS);
}

#[test]
fn a_resting_voice_gives_its_slot_back() {
    let mut p = sym();
    p.modal.decay = 0.3;
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

#[test]
fn a_mono_holder_is_stolen_when_newest_arrives() {
    let mut s = Stage::new(&[
        (sym(), PartMode::Mono),
        (sym(), PartMode::Poly),
        (sym(), PartMode::Poly),
        (sym(), PartMode::Poly),
    ]);
    for (part, n) in [(0, 48), (1, 55), (2, 60), (3, 67)] {
        s.on(part, n);
        s.block();
    }
    let mono = s.voice_of(0, 48);
    s.on(1, 57);
    assert_eq!(s.voice_of(1, 57), mono, "the Mono holder is the oldest");
    for _ in 0..FADE_BLOCKS {
        assert!(s.inst.active()[mono], "the Mono note fades over FADE");
        s.block();
    }
    s.block();
    assert_eq!(s.inst.slot_kinds()[mono], SYM, "57 sounds on it");
    assert!(s.inst.active()[mono]);

    // Part 1 again: a fresh Mono note, stealing the now-oldest slot (55's).
    let oldest = s.voice_of(1, 55);
    s.on(0, 50);
    assert_eq!(s.voice_of(0, 50), oldest);
    for _ in 0..FADE_BLOCKS + 1 {
        s.block();
    }
    assert_eq!(s.inst.slot_kinds()[oldest], SYM);
    assert!(s.inst.active()[oldest], "Part 1's new note sounds");
    assert!(peak(s.inst.part_bus(0)) > 0.0);
}

/// The pool ranks slots by the `Allocator`'s own note ages: a Mono
/// retrigger refreshes its slot to the age `book` gives the note, so the
/// next steal takes the voice with the lowest `VoiceSlot::age`, not the
/// retriggered one.
#[test]
fn pool_ages_are_the_allocators() {
    let mut s = Stage::new(&[
        (sym(), PartMode::Mono),
        (sym(), PartMode::Poly),
        (sym(), PartMode::Poly),
        (sym(), PartMode::Poly),
    ]);
    for (part, n) in [(0, 48), (1, 55), (2, 60), (3, 67)] {
        s.on(part, n);
        s.block();
    }
    let mono = s.voice_of(0, 48);
    s.on(0, 50);
    assert_eq!(s.voice_of(0, 50), mono, "Mono retriggers its voice");
    s.block();
    let slots = s.inst.allocator().slots();
    let oldest = (0..MAX_VOICES)
        .filter(|&v| s.inst.slot_kinds()[v] == SYM)
        .min_by_key(|&v| slots[v].age())
        .unwrap();
    assert_eq!(oldest, s.voice_of(1, 55), "55 is now the oldest by age");
    s.on(2, 62);
    assert_eq!(s.voice_of(2, 62), oldest, "the steal follows the ages");
}
