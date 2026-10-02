//! The parameter sweep's rig (`param_sweep_test.rs`): the real `Instrument`,
//! `FxBus` and `DacBlocks` as the desktop drives them, every parameter the
//! registry lists set through `Block::set` as the UI sets it, and each DAC
//! pair measured apart, not summed as the desktop's speakers hear them.

use chimera_core::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use chimera_core::block::{ParamId, ParamKind, ParamSpec};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::chorus::ChorusParams;
use chimera_core::dsp::comp::CompParams;
use chimera_core::dsp::delay::DelayParams;
use chimera_core::dsp::fx_bus::{FxBus, FxParams};
use chimera_core::dsp::limiter::{CEILING, OUTPUT_TRIM};
use chimera_core::dsp::modal::{ModalParams, ResonatorMode, reads};
use chimera_core::dsp::reverb::ReverbParams;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, DAC_PAIRS, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::modulation::{ModSource, VCA};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{DriveParams, EngineType, FilterParams, FolderParams, OutParams};
use chimera_core::part::PartParams;
use chimera_core::preset::{Performance, Sound};
use chimera_core::project::PartId;
use chimera_core::scope::ScopeWriter;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// Blocks per second at 48 kHz.
pub const BPS: usize = 750;
/// Held RMS under this (−100 dBFS) on a pair is silence: the Part is not there.
pub const SILENT_RMS: f32 = 1e-5;
/// A pair no Part plays and no FX return reaches carries nothing at all.
pub const LEAK_PEAK: f32 = 1e-7;
/// DC over a whole case, per side, post limiter: −46 dBFS.
pub const DC_MAX: f32 = 0.005;
/// Blocks after a note-on before DC is judged, 0.3 s: past the attack. A
/// note's start-up transient, the voice's 5 Hz blocker settling (3τ, 95 ms)
/// and a bowed string's static deflection settling (a C3 bow's 12 periods),
/// is not a held note's DC.
pub const DC_FROM: usize = 225;
/// A click: a jump's second difference over `CLICK_RATIO` × the larger of
/// the steady states either side of it, and over `CLICK_FLOOR`.
pub const CLICK_RATIO: f32 = 4.0;
pub const CLICK_FLOOR: f32 = 0.02;

// ── Sounds ─────────────────────────────────────────────────────────────

/// The Sounds each parameter is swept on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Patch {
    AlgoInit,
    ModalInit(ResonatorMode),
    /// MORPH PAD (all six operators) with every modulator routed.
    BusyAlgo,
    /// Modal STRING with BODY and the ensemble on and every modulator routed.
    BusyModal,
    Factory(u8),
    /// Operator 1 alone on the triangle: smooth, so any step in it stands out.
    Probe,
}

impl Patch {
    pub fn name(self) -> String {
        match self {
            Patch::AlgoInit => "ALGO INIT".into(),
            Patch::ModalInit(m) => format!("MODAL {} INIT", model_name(m)),
            Patch::BusyAlgo => "BUSY ALGO".into(),
            Patch::BusyModal => "BUSY MODAL".into(),
            Patch::Probe => "TRI PROBE".into(),
            Patch::Factory(i) => format!(
                "F{i} {}",
                chimera_core::factory::factory_sound(i as usize)
                    .map(|s| s.name.as_str().to_string())
                    .unwrap_or_default()
            ),
        }
    }

    pub fn engine(self) -> EngineType {
        self.sound().engine()
    }

    pub fn mode(self) -> Option<ResonatorMode> {
        let s = self.sound();
        (s.engine() == EngineType::Modal).then_some(s.params.modal.mode)
    }

    pub fn sound(self) -> Sound {
        match self {
            Patch::AlgoInit => Sound::init(EngineType::Algo),
            Patch::ModalInit(m) => {
                let mut s = Sound::init(EngineType::Modal);
                s.params.modal.mode = m;
                s
            }
            Patch::BusyAlgo => {
                let mut s = chimera_core::factory::factory_sound(6).expect("MORPH PAD");
                s.params.filter.resonance = 0.4;
                s.params.drive.drive = 0.3;
                s.params.folder.fold = 0.2;
                route_all(&mut s, ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH));
                s
            }
            Patch::BusyModal => {
                let mut s = Sound::init(EngineType::Modal);
                let m = &mut s.params.modal;
                (m.body, m.ens_mix, m.ens_depth) = (0.6, 0.5, 0.5);
                s.params.filter.resonance = 0.3;
                s.params.drive.drive = 0.2;
                route_all(&mut s, ParamAddr::new(BlockRef::Modal, ModalParams::BRIGHT));
                s
            }
            Patch::Factory(i) => chimera_core::factory::factory_sound(i as usize).expect("factory"),
            Patch::Probe => {
                let mut s = Sound::init(EngineType::Algo);
                s.params.algo = AlgoParams::single(chimera_core::dsp::algo::waves::WaveId::TRI);
                s
            }
        }
    }
}

pub fn model_name(m: ResonatorMode) -> &'static str {
    match m {
        ResonatorMode::String => "STRING",
        ResonatorMode::Modal => "BANK",
        ResonatorMode::Bowed => "BOWED",
        ResonatorMode::Sympathetic => "SYMP",
    }
}

/// Every modulator routed: ENV 1 and LFO 1 → CUTOFF, ENV 2 → VCA (the amp
/// follows ENV 2), ENV 3 → PITCH, LFO 2 → `engine_dest`, LFO 3 → DRIVE
/// and FOLD, VEL → LEVEL.
fn route_all(s: &mut Sound, engine_dest: ParamAddr) {
    let ms = &mut s.mod_state;
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let mut at = |addr: ParamAddr| {
        (0..ms.num_dests())
            .find(|&d| ms.dest(d) == addr)
            .or_else(|| ms.push(addr))
            .expect("room for the route")
    };
    let routes = [
        (ModSource::Env1, cutoff, 40),
        (ModSource::Lfo1, cutoff, 20),
        (ModSource::Env2, VCA, 127),
        (
            ModSource::Env3,
            ParamAddr::new(BlockRef::Pitch, chimera_core::params::PitchParams::PITCH),
            6,
        ),
        (ModSource::Lfo2, engine_dest, 30),
        (
            ModSource::Lfo3,
            ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE),
            30,
        ),
        (
            ModSource::Lfo3,
            ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
            20,
        ),
        (
            ModSource::Vel,
            ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
            20,
        ),
    ];
    let dests: Vec<usize> = routes.iter().map(|r| at(r.1)).collect();
    for ((src, _, amt), d) in routes.into_iter().zip(dests) {
        ms.set_amount(src.index(), d, amt);
    }
    s.params.lfos[1].rate = 3.0;
    s.params.lfos[2].rate = 7.0;
}

/// Every effect on at a typical setting, the compressor working.
pub fn fx_on() -> FxParams {
    FxParams {
        chorus: ChorusParams {
            mode: 1,
            mix: 0.5,
            ..Default::default()
        },
        delay: DelayParams {
            mix: 0.5,
            ..Default::default()
        },
        reverb: ReverbParams {
            mix: 0.5,
            ..Default::default()
        },
        comp: CompParams {
            ratio: 3,
            makeup: 0.1,
            ..Default::default()
        },
        ..Default::default()
    }
}

// ── The registry ───────────────────────────────────────────────────────

/// One user-facing parameter: a block instance and its spec.
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub block: BlockRef,
    pub spec: &'static ParamSpec,
}

impl Param {
    pub fn is(&self, block: BlockRef, id: ParamId) -> bool {
        self.block == block && self.spec.id == id
    }

    pub fn name(&self) -> String {
        format!("{:?}.{}", self.block, self.spec.label)
    }

    /// Whether the parameter is shared, not the Sound's: FX, mix, MIDI.
    pub fn performance(&self) -> bool {
        matches!(
            self.block,
            BlockRef::Chorus
                | BlockRef::Delay
                | BlockRef::Reverb
                | BlockRef::Tape
                | BlockRef::Comp
                | BlockRef::Part
        )
    }

    /// Whether `patch` has this parameter at all (Algo's on Algo, Modal's
    /// on the models that read it).
    pub fn applies(&self, patch: Patch) -> bool {
        match self.block {
            BlockRef::Algo | BlockRef::AlgoOp(_) => patch.engine() == EngineType::Algo,
            BlockRef::Modal => patch.mode().is_some_and(|m| reads(m, self.spec.id)),
            _ => true,
        }
    }

    /// The sweep's points: min, three interior points, max; every choice
    /// of an Enum. `quick`: min and max, and every choice up to four.
    pub fn points(&self, quick: bool) -> Vec<f32> {
        let s = self.spec;
        let n = (s.max - s.min).round() as usize;
        if s.kind == ParamKind::Enum && (!quick || n < 4) {
            return (0..=n).map(|v| v as f32).collect();
        }
        let fr: &[f32] = if quick {
            &[0.0, 1.0]
        } else {
            &[0.0, 0.25, 0.5, 0.75, 1.0]
        };
        let mut v: Vec<f32> = fr
            .iter()
            .map(|f| s.quantize(s.min + f * (s.max - s.min)))
            .collect();
        v.dedup();
        v
    }
}

/// Theme is the UI's (`ThemeSettings`): it never reaches the audio.
pub const NOT_AUDIO: [BlockRef; 1] = [BlockRef::Theme];

/// Every parameter of every block instance in `BlockRef::ALL`, less the
/// UI's own; `one_instance`: the first operator, ENV and LFO only.
pub fn registry(one_instance: bool) -> Vec<Param> {
    BlockRef::ALL
        .into_iter()
        .filter(|b| !NOT_AUDIO.contains(b))
        .filter(|b| {
            !one_instance
                || !matches!(
                    b,
                    BlockRef::AlgoOp(o) if o.index() > 0
                ) && !matches!(b, BlockRef::Env(s) if s.index() > 0)
                    && !matches!(b, BlockRef::Lfo(s) if s.index() > 0)
        })
        .flat_map(|block| block.specs().iter().map(move |spec| Param { block, spec }))
        .collect()
}

// ── The rig ────────────────────────────────────────────────────────────

pub struct Bench {
    pub perf: Box<Performance>,
    shared: Box<AudioShared>,
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
}

pub const CH: MidiChannel = MidiChannel::clamped(0);

impl Bench {
    /// Part 1 plays `patch` on channel 1 with `mix`; the other Parts listen
    /// on channels 2–6 and stay silent.
    pub fn new(patch: Patch, mix: PartParams, fx: FxParams) -> Self {
        let mut perf = Box::new(Performance::new());
        *perf.edit(PartId::ALL[0]).sound = patch.sound();
        *perf.edit(PartId::ALL[0]).mix = mix;
        perf.fx = fx;
        let shared = Box::new(AudioShared::from_performance(&perf));
        Self {
            perf,
            shared,
            inst: Box::new(Instrument::new(
                chimera_hal::SAMPLE_RATE,
                SampleBudget::for_cpu(CPU_HZ_REV_V),
            )),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: super::scope_writer(),
        }
    }

    /// The UI's edit: `Block::set` (clamped, rounded), then the publish.
    pub fn set(&mut self, p: &Param, v: f32) {
        self.perf
            .edit(PartId::ALL[0])
            .block_mut(p.block)
            .expect("an audio block")
            .set(p.spec.id, v);
        self.shared.update_from(&self.perf, 0);
    }

    pub fn get(&mut self, p: &Param) -> f32 {
        self.perf
            .edit(PartId::ALL[0])
            .block(p.block)
            .expect("an audio block")
            .get(p.spec.id)
    }

    pub fn note(&mut self, note: u8, vel: Option<u8>) {
        let kind = match vel {
            Some(v) => NoteKind::On(Velocity::new(v).unwrap()),
            None => NoteKind::Off,
        };
        let ev = NoteEvent {
            channel: CH,
            note: MidiNote::new(note).unwrap(),
            kind,
        };
        self.inst.handle(ev, &self.shared);
    }

    pub fn render(&mut self, tape: &mut Tape) {
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
        let (out, pre) = (self.dac.out(), self.dac.input());
        for k in 0..DAC_PAIRS {
            tape.out[k].extend_from_slice(&out[k]);
            tape.pre_peak[k] = pre[k].iter().fold(tape.pre_peak[k], |m, x| m.max(x.abs()));
        }
        tape.pre.extend_from_slice(&pre[0]);
        tape.bus.extend_from_slice(self.inst.part_bus(0));
        let a = self.inst.allocator();
        let live: u32 = a
            .slots()
            .iter()
            .filter(|s| !s.dying())
            .map(|s| s.cost().0)
            .sum();
        let busy = a.slots().iter().filter(|s| !s.is_free()).count();
        tape.bill.push(FxBus::COST.0 + live);
        tape.busy.push(busy as u8);
        tape.budget = a.budget().as_cost().0;
        tape.refused = a.refused();
        let p = &self.shared.parts[0];
        tape.voice_cost = tape.voice_cost.max(Voice::cost(&p.params, &p.mod_state).0);
    }

    pub fn all_free(&self) -> bool {
        self.inst.allocator().slots().iter().all(|s| s.is_free())
    }
}

/// What one run left: each pair's output (interleaved L, R, after the
/// limiter), Part 1's bus, and the allocator's bill each block.
#[derive(Default)]
pub struct Tape {
    pub out: [Vec<f32>; DAC_PAIRS],
    pub pre_peak: [f32; DAC_PAIRS],
    /// P1 before the limiter, a block ahead of `out`.
    pub pre: Vec<f32>,
    pub bus: Vec<f32>,
    pub bill: Vec<u32>,
    pub busy: Vec<u8>,
    pub budget: u32,
    pub refused: u32,
    pub voice_cost: u32,
}

impl Tape {
    pub fn blocks(&self) -> usize {
        self.bus.len() / BLOCK_SIZE
    }

    /// Pair `k`'s samples of blocks `a..b`, both sides.
    pub fn pair(&self, k: usize, a: usize, b: usize) -> &[f32] {
        let n = self.out[k].len();
        &self.out[k][(2 * BLOCK_SIZE * a).min(n)..(2 * BLOCK_SIZE * b).min(n)]
    }

    pub fn rms(&self, k: usize, a: usize, b: usize) -> f32 {
        super::rms(self.pair(k, a, b)).max(0.0)
    }

    pub fn peak(&self, k: usize) -> f32 {
        self.out[k].iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    pub fn finite(&self) -> bool {
        self.out.iter().all(|p| p.iter().all(|x| x.is_finite()))
            && self.bus.iter().all(|x| x.is_finite())
    }

    /// The DC over blocks `a..b` of pair `k`, the larger side's: an offset
    /// both halves of the window share (the lesser of their means, if of
    /// one sign). Sub-audio movement, an FM sideband near 0 Hz or a bow's
    /// drift, averages to opposite signs and reads as none.
    pub fn dc(&self, k: usize, a: usize, b: usize) -> f32 {
        let m = (a + b) / 2;
        let mean = |a: usize, b: usize, o: usize| {
            let p = self.pair(k, a, b);
            let n = (p.len() / 2).max(1) as f64;
            (p.iter().skip(o).step_by(2).map(|&x| x as f64).sum::<f64>() / n) as f32
        };
        (0..2)
            .map(|o| {
                let (x, y) = (mean(a, m, o), mean(m, b, o));
                if x * y > 0.0 {
                    x.abs().min(y.abs())
                } else {
                    0.0
                }
            })
            .fold(0.0, f32::max)
    }

    /// The largest second difference over blocks `a..b` of pair `k`, per side.
    pub fn d2(&self, k: usize, a: usize, b: usize) -> f32 {
        let p = self.pair(k, a.saturating_sub(1), b);
        let side = |o: usize| {
            let s: Vec<f32> = p.iter().skip(o).step_by(2).copied().collect();
            s.windows(3)
                .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
                .fold(0.0f32, f32::max)
        };
        side(0).max(side(1))
    }

    /// Transient at event block `e` (heard a block later, the limiter's
    /// lookahead) against the steady states before and after it.
    /// Each steady window stops short of the events either side
    /// (`prev`, `next`), so one event's transient never hides another's.
    pub fn click(
        &self,
        k: usize,
        e: usize,
        (prev, next): (usize, usize),
        after: bool,
    ) -> Option<f32> {
        let span = |a: usize, b: usize| if a < b { self.d2(k, a, b) } else { 0.0 };
        let pre = span(e.saturating_sub(40).max(prev + 6), e.saturating_sub(1));
        let trans = self.d2(k, e + 1, e + 4);
        let post = if after {
            span(e + 20, (e + 60).min(next.saturating_sub(1)))
        } else {
            0.0
        };
        let steady = pre.max(post);
        (trans > CLICK_FLOOR && trans > CLICK_RATIO * steady).then_some(trans / steady.max(1e-9))
    }

    /// Blocks from `from` until no voice is busy; `None` if never.
    pub fn freed(&self, from: usize) -> Option<usize> {
        self.busy[from..].iter().position(|&b| b == 0)
    }
}

// ── The script ─────────────────────────────────────────────────────────

/// One case's timeline, in blocks. A single note (C4) is held and
/// released, then a chord (C3 E3 G3 C4) is held and released and its tail
/// rendered until every voice is free or `bound` passes. A jump case moves
/// the parameter to `to` and back mid-attack, mid-chord and in the tail;
/// a static case's DC is judged from 0.3 s after each note-on, over the
/// held rest (`DC_FROM`).
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub hold: usize,
    pub gap: usize,
    pub bound: usize,
}

pub const FAST: Timing = Timing {
    hold: 450,
    gap: 120,
    bound: 1500,
};
pub const THOROUGH: Timing = Timing {
    hold: 450,
    gap: 150,
    bound: 3000,
};

pub const CHORD: [u8; 4] = [48, 52, 55, 60];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Move {
    Static(f32),
    Jump { from: f32, to: f32 },
}

impl Timing {
    pub fn a_off(&self) -> usize {
        self.hold
    }
    pub fn b_on(&self) -> usize {
        self.hold + self.gap
    }
    pub fn b_off(&self) -> usize {
        self.b_on() + self.hold
    }
    /// The jumps' blocks and values, `(block, to_max)`: within each note's
    /// first 0.3 s (`DC_FROM`), so the held rest is static.
    pub fn jumps(&self) -> [(usize, bool); 4] {
        [
            (DC_FROM / 3, true),
            (2 * DC_FROM / 3, false),
            (self.b_on() + DC_FROM / 2, true),
            (self.b_off() + 60, false),
        ]
    }
}

/// Everything one case measured.
#[derive(Clone, Debug)]
pub struct Run {
    pub finite: bool,
    pub peak: [f32; DAC_PAIRS],
    pub pre_peak: [f32; DAC_PAIRS],
    /// Held RMS, single note then chord, per pair.
    pub held: [[f32; DAC_PAIRS]; 2],
    pub dc: [f32; DAC_PAIRS],
    /// `(event label, pair, ratio)`.
    pub clicks: Vec<(&'static str, usize, f32)>,
    /// Blocks after the chord's release until every voice is free.
    pub freed: Option<usize>,
    /// Bus RMS, the tail's first and last 0.2 s: a stuck voice holds level.
    pub tail: (f32, f32),
    pub bill_max: u32,
    pub over_blocks: usize,
    pub budget: u32,
    pub refused: u32,
    pub voice_cost: u32,
    pub most_busy: u8,
}

/// Plays the case: `p` at `mv`'s value(s) on `bench`.
pub fn play(bench: &mut Bench, p: Option<&Param>, mv: Move, t: Timing) -> Run {
    let (start, to) = match mv {
        Move::Static(v) => (v, None),
        Move::Jump { from, to } => (from, Some(to)),
    };
    if let Some(p) = p {
        bench.set(p, start);
    }
    let mut tape = Tape::default();
    let mut events: Vec<(usize, &'static str)> = Vec::new();
    let end = t.b_off() + t.bound;
    for b in 0..end {
        if b == 0 {
            bench.note(60, Some(100));
        }
        if b == t.a_off() {
            bench.note(60, None);
            events.push((b, "note-off"));
        }
        if b == t.b_on() {
            for n in CHORD {
                bench.note(n, Some(100));
            }
        }
        if b == t.b_off() {
            for n in CHORD {
                bench.note(n, None);
            }
            events.push((b, "chord-off"));
        }
        if let (Some(p), Some(to)) = (p, to) {
            for (i, &(j, up)) in t.jumps().iter().enumerate() {
                if b == j {
                    bench.set(p, if up { to } else { start });
                    events.push((j, ["jump A↑", "jump A↓", "jump B↑", "jump tail↓"][i]));
                }
            }
        }
        bench.render(&mut tape);
        // The tail: done once every voice is free and a jump has had its say.
        if b > t.b_off() + 120
            && bench.all_free()
            && tape.busy.iter().rev().take(40).all(|&x| x == 0)
        {
            break;
        }
    }
    // Each time the last voice frees: a step there is a click at the end.
    for b in 1..tape.blocks() {
        if tape.busy[b - 1] > 0 && tape.busy[b] == 0 {
            events.push((b, "voice freed"));
        }
    }
    let pairs = |f: &dyn Fn(usize) -> f32| core::array::from_fn(f);
    let mut clicks = Vec::new();
    let mut edges: Vec<usize> = events.iter().map(|e| e.0).chain([0, t.b_on()]).collect();
    edges.sort_unstable();
    for &(e, label) in &events {
        // After a jump or a note-off the sound goes on: its steady state
        // there counts too (a slow release keeps the waveform's own edges).
        let after = label != "voice freed";
        if e + if after { 60 } else { 4 } >= tape.blocks() || e < 42 {
            continue;
        }
        let prev = edges.iter().rev().find(|&&x| x < e).copied().unwrap_or(0);
        let next = edges
            .iter()
            .find(|&&x| x > e)
            .copied()
            .unwrap_or(usize::MAX);
        // A voice freed within another event's transient window is that
        // event's, judged there. One freed as the next note starts would
        // hear that note's onset as its transient.
        if !after && (e <= prev + 4 || next <= e + 4) {
            continue;
        }
        for k in 0..DAC_PAIRS {
            if let Some(r) = tape.click(k, e, (prev, next), after) {
                clicks.push((label, k, r));
            }
        }
    }
    let tail_at = t.b_off();
    let fifth = BPS / 5;
    let bus_rms = |a: usize, b: usize| {
        let n = tape.bus.len();
        super::rms(&tape.bus[(a * BLOCK_SIZE).min(n)..(b * BLOCK_SIZE).min(n)])
    };
    let last = tape.blocks();
    Run {
        finite: tape.finite(),
        peak: pairs(&|k| tape.peak(k)),
        pre_peak: tape.pre_peak,
        held: [
            pairs(&|k| tape.rms(k, 8, t.a_off())),
            pairs(&|k| tape.rms(k, t.b_on() + 8, t.b_off())),
        ],
        dc: pairs(&|k| {
            tape.dc(k, DC_FROM, t.a_off())
                .max(tape.dc(k, t.b_on() + DC_FROM, t.b_off()))
        }),
        clicks,
        freed: tape.freed(tail_at),
        tail: (
            bus_rms(tail_at, tail_at + fifth),
            bus_rms(last.saturating_sub(fifth), last),
        ),
        bill_max: tape.bill.iter().copied().max().unwrap_or(0),
        over_blocks: tape.bill.iter().filter(|&&b| b > tape.budget).count(),
        budget: tape.budget,
        refused: tape.refused,
        voice_cost: tape.voice_cost,
        most_busy: tape.busy.iter().copied().max().unwrap_or(0),
    }
}

/// Whether the peak is within the output stage's ceiling.
pub fn within_ceiling(peak: f32) -> bool {
    peak <= CEILING * (1.0 + 1e-6)
}

pub fn db(x: f32) -> f32 {
    20.0 * (x.max(1e-12)).log10()
}

/// Ungated BS.1770 loudness of an interleaved stereo run at 48 kHz: the
/// K-weighting's shelf and high-pass, then −0.691 + 10·log10 of the sides'
/// mean squares summed. Over a sustained window it is the integrated
/// loudness less the gate, which a held note never trips.
pub fn lufs(x: &[f32]) -> f32 {
    let biquad = |b: [f64; 3], a: [f64; 2], s: &[f64]| -> Vec<f64> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        s.iter()
            .map(|&x0| {
                let y = b[0] * x0 + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
                (x2, x1, y2, y1) = (x1, x0, y1, y);
                y
            })
            .collect()
    };
    let k = |s: Vec<f64>| {
        let s = biquad(
            [
                1.535_124_859_586_97,
                -2.691_696_189_406_38,
                1.198_392_810_852_85,
            ],
            [-1.690_659_293_182_41, 0.732_480_774_215_85],
            &s,
        );
        biquad(
            [1.0, -2.0, 1.0],
            [-1.990_047_454_833_98, 0.990_072_250_366_21],
            &s,
        )
    };
    let ms = |s: Vec<f64>| s.iter().map(|v| v * v).sum::<f64>() / s.len().max(1) as f64;
    let l = k(x.iter().step_by(2).map(|&v| v as f64).collect());
    let r = k(x.iter().skip(1).step_by(2).map(|&v| v as f64).collect());
    (-0.691 + 10.0 * (ms(l) + ms(r)).max(1e-20).log10()) as f32
}

// ── Levels ─────────────────────────────────────────────────────────────

/// One held sound's level on each pair, after the output stage.
#[derive(Clone, Copy, Debug)]
pub struct Level {
    pub peak: [f32; DAC_PAIRS],
    pub rms: [f32; DAC_PAIRS],
    /// P1's ungated K-weighted loudness (`lufs`).
    pub lufs: f32,
    /// Part 1's bus (its voices summed, before pan, level and the trim).
    pub bus_rms: f32,
    pub bus_peak: f32,
    /// The limiter's deepest gain reduction, dB, over the trim (0 unlimited).
    pub gr_db: f32,
    /// The loudness it took from P1 over the hold, dB.
    pub limited_db: f32,
}

/// `notes` at `vel` on `patch`, held `blocks`, measured over the hold
/// (from the block the limiter's lookahead first lets through).
pub fn level(patch: Patch, notes: &[u8], vel: u8, blocks: usize) -> Level {
    let mut b = Bench::new(patch, PartParams::default(), FxParams::default());
    let mut tape = Tape::default();
    for blk in 0..=blocks {
        if blk == 0 {
            for &n in notes {
                b.note(n, Some(vel));
            }
        }
        b.render(&mut tape);
    }
    let bus = &tape.bus[..blocks * BLOCK_SIZE];
    Level {
        peak: core::array::from_fn(|k| {
            tape.pair(k, 1, blocks + 1)
                .iter()
                .fold(0.0f32, |m, x| m.max(x.abs()))
        }),
        rms: core::array::from_fn(|k| tape.rms(k, 1, blocks + 1)),
        lufs: lufs(tape.pair(0, 1, blocks + 1)),
        bus_rms: super::rms(bus),
        bus_peak: bus.iter().fold(0.0f32, |m, x| m.max(x.abs())),
        gr_db: db(tape.pre_peak.iter().fold(0.0f32, |m, &x| m.max(x)) * OUTPUT_TRIM / CEILING)
            .max(0.0),
        limited_db: lufs(&tape.pre[..2 * BLOCK_SIZE * blocks]) + db(OUTPUT_TRIM)
            - lufs(tape.pair(0, 1, blocks + 1)),
    }
}

/// `x`'s Welch power spectrum: `n`-point Hann frames (`n` a power of two)
/// at 50 % overlap, averaged; bin `k` is `k · SR / n` Hz.
pub fn power(x: &[f32], n: usize) -> Vec<f64> {
    use std::f64::consts::TAU;
    let mut acc = vec![0.0; n / 2];
    let mut frames = 0;
    let mut i = 0;
    while i + n <= x.len() {
        let mut re: Vec<f64> = (0..n)
            .map(|k| x[i + k] as f64 * (0.5 - 0.5 * (TAU * k as f64 / n as f64).cos()))
            .collect();
        let mut im = vec![0.0; n];
        // Radix-2, in place.
        let mut j = 0;
        for a in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j |= bit;
            if a < j {
                re.swap(a, j);
                im.swap(a, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let (wr, wi) = ((-TAU / len as f64).cos(), (-TAU / len as f64).sin());
            for s in (0..n).step_by(len) {
                let (mut cr, mut ci) = (1.0, 0.0);
                for k in 0..len / 2 {
                    let (a, b) = (s + k, s + k + len / 2);
                    let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                    (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                    (re[a], im[a]) = (re[a] + tr, im[a] + ti);
                    (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
                }
            }
            len <<= 1;
        }
        for (k, a) in acc.iter_mut().enumerate() {
            *a += re[k] * re[k] + im[k] * im[k];
        }
        frames += 1;
        i += n / 2;
    }
    acc.iter().map(|a| a / frames.max(1) as f64).collect()
}

/// `notes` at `vel` on `patch`, held `blocks`: P1's left side, limited.
pub fn p1_left(patch: Patch, notes: &[u8], vel: u8, blocks: usize) -> Vec<f32> {
    let mut b = Bench::new(patch, PartParams::default(), FxParams::default());
    let mut tape = Tape::default();
    for &n in notes {
        b.note(n, Some(vel));
    }
    for _ in 0..blocks {
        b.render(&mut tape);
    }
    tape.out[0].iter().step_by(2).copied().collect()
}
