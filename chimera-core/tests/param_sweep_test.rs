//! Every user-facing parameter, from the registry, swept on the desktop's
//! real mix path (`Instrument`, `FxBus`, `DacBlocks`) and measured on each
//! DAC pair apart: finite, under the ceiling, no DC, no clicks the
//! parameter doesn't cause, every voice freed, not silent unless the
//! parameter silences it, and never billed over the CPU budget unshed.
//!
//! `sweep_fast` runs on every `cargo test`: each parameter once (the first
//! operator, ENV and LFO), at its ends and one jump each way. `sweep_thorough`
//! (`cargo test -- --ignored`) sweeps every instance at five points and every
//! choice, on every Sound it applies to, and writes the report to
//! `$PARAM_SWEEP_REPORT` when it is set. What a parameter may legitimately
//! do (fall silent, click, ring on, carry DC) is listed below with the
//! reason; a known defect is named, kept out of the sweeps' verdict, and
//! pinned by an ignored test of its own.

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use chimera_core::addr::{BlockRef, Op};
use chimera_core::block::ParamId;
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::dsp::chorus::ChorusParams;
use chimera_core::dsp::delay::DelayParams;
use chimera_core::dsp::fx_bus::{FxBus, FxParams};
use chimera_core::dsp::modal::{ModalParams, ResonatorMode};
use chimera_core::dsp::reverb::ReverbParams;
use chimera_core::hw::DAC_PAIRS;
use chimera_core::params::{DriveParams, FilterParams, FolderParams, OutParams, PitchParams};
use chimera_core::part::{DacPair, PartParams};
use common::sweep::*;

// ── Cases ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
struct Case {
    param: Param,
    patch: Patch,
    mv: Move,
    /// FX on and every send at 0.5 (the FX and mix parameters' baseline).
    wet: bool,
}

impl Case {
    fn label(&self) -> String {
        let v = match self.mv {
            Move::Static(v) => format!("{v}"),
            Move::Jump { from, to } => format!("{from}→{to}"),
        };
        let wet = if self.wet { " (wet)" } else { "" };
        format!("{} = {v} on {}{wet}", self.param.name(), self.patch.name())
    }

    fn bench(&self) -> Bench {
        let mut mix = PartParams::default();
        let fx = if self.wet {
            mix.sends = [0.5; 3];
            fx_on()
        } else {
            FxParams::default()
        };
        Bench::new(self.patch, mix, fx)
    }

    fn values(&self) -> Vec<f32> {
        match self.mv {
            Move::Static(v) => vec![v],
            Move::Jump { from, to } => vec![from, to],
        }
    }

    fn any(&self, f: impl Fn(f32) -> bool) -> bool {
        self.values().into_iter().any(f)
    }

    fn is(&self, block: BlockRef, id: ParamId) -> bool {
        self.param.is(block, id)
    }

    fn is_op(&self, id: ParamId) -> bool {
        matches!(self.param.block, BlockRef::AlgoOp(_)) && self.param.spec.id == id
    }

    fn modal(&self, id: ParamId) -> bool {
        self.is(BlockRef::Modal, id)
    }

    /// A Modal model that rings on after note-off (all but STRING), the
    /// patch's own or one MODEL moves it to.
    fn rings_on(&self) -> bool {
        self.patch.mode().is_some_and(|m| {
            m != ResonatorMode::String || (self.modal(ModalParams::MODE) && self.any(|v| v != 0.0))
        })
    }
}

const SOUNDS: [Patch; 7] = [
    Patch::AlgoInit,
    Patch::ModalInit(ResonatorMode::String),
    Patch::ModalInit(ResonatorMode::Modal),
    Patch::ModalInit(ResonatorMode::Bowed),
    Patch::ModalInit(ResonatorMode::Sympathetic),
    Patch::BusyAlgo,
    Patch::BusyModal,
];

/// The Sounds `p` is swept on. Quick: the busy patches (every modulator
/// routed, so ENV and LFO are heard), each Modal model its parameter
/// belongs to, and ALGO INIT for the FX and mix, wet.
fn patches(p: &Param, quick: bool) -> Vec<(Patch, bool)> {
    if p.performance() {
        let v = if quick {
            vec![Patch::AlgoInit]
        } else {
            vec![
                Patch::AlgoInit,
                Patch::ModalInit(ResonatorMode::String),
                Patch::BusyAlgo,
            ]
        };
        return v.into_iter().map(|s| (s, true)).collect();
    }
    let v: Vec<Patch> = match (p.block, quick) {
        (BlockRef::Modal, true) => SOUNDS
            .into_iter()
            .filter(|s| matches!(s, Patch::ModalInit(_)) || *s == Patch::BusyModal)
            .collect(),
        (BlockRef::Algo | BlockRef::AlgoOp(_), true) => vec![Patch::BusyAlgo],
        (_, true) => vec![Patch::BusyAlgo, Patch::BusyModal],
        (_, false) => SOUNDS.to_vec(),
    };
    v.into_iter()
        .filter(|s| p.applies(*s))
        .map(|s| (s, false))
        .collect()
}

fn cases(quick: bool) -> Vec<Case> {
    let mut out = Vec::new();
    for param in registry(quick) {
        for (patch, wet) in patches(&param, quick) {
            let pts = param.points(quick);
            let (lo, hi) = (pts[0], *pts.last().unwrap());
            for &v in &pts {
                out.push(Case {
                    param,
                    patch,
                    mv: Move::Static(v),
                    wet,
                });
            }
            if lo != hi {
                out.push(Case {
                    param,
                    patch,
                    mv: Move::Jump { from: lo, to: hi },
                    wet,
                });
                if !quick {
                    out.push(Case {
                        param,
                        patch,
                        mv: Move::Jump { from: hi, to: lo },
                        wet,
                    });
                }
            }
        }
        // Clicks stand out on the triangle: every jump again on it.
        let pts = param.points(true);
        let (lo, hi) = (pts[0], *pts.last().unwrap());
        if lo != hi && param.applies(Patch::Probe) {
            let wet = param.performance();
            for (from, to) in [(lo, hi), (hi, lo)] {
                out.push(Case {
                    param,
                    patch: Patch::Probe,
                    mv: Move::Jump { from, to },
                    wet,
                });
            }
        }
    }
    out
}

/// Runs every case on every core; results in case order.
fn run_all(cases: &[Case], t: Timing) -> Vec<Run> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new(vec![None; cases.len()]);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..workers {
            std::thread::Builder::new()
                .stack_size(64 << 20)
                .spawn_scoped(s, || {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(c) = cases.get(i) else { break };
                        let r = play(&mut c.bench(), Some(&c.param), c.mv, t);
                        results.lock().unwrap()[i] = Some(r);
                    }
                })
                .unwrap();
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(Option::unwrap)
        .collect()
}

// ── What a parameter may legitimately do ───────────────────────────────

/// Why `c` may leave Part 1 silent. Each is the parameter doing its job.
fn silences(c: &Case) -> Option<&'static str> {
    if c.is(BlockRef::Part, PartParams::LEVEL) && c.any(|v| v == 0.0) {
        return Some("Part LEVEL 0 mutes the Part");
    }
    if c.is(BlockRef::Out, OutParams::VOLUME) && c.any(|v| v == 0.0) {
        return Some("OUT LEVEL 0 is the voice at volume 0");
    }
    if (c.is(BlockRef::Part, PartParams::CHANNEL)
        || (c.param.block == BlockRef::Channels && c.param.spec.id.0 == 0))
        && c.any(|v| v != 0.0)
    {
        return Some("Part 1 listens on another MIDI channel than the notes'");
    }
    if c.modal(ModalParams::EXCITE) && c.any(|v| v == 0.0) {
        return Some("EXCITE 0: nothing strikes the resonator");
    }
    if (c.modal(ModalParams::FORCE) || c.modal(ModalParams::SPEED)) && c.any(|v| v == 0.0) {
        return Some("FORCE or SPEED 0: the bow doesn't press, or doesn't move");
    }
    if c.is_op(AlgoOpParams::LEVEL) && c.any(|v| v == 0.0) {
        return Some(
            "a carrier at LEVEL 0 is silent, and a voice whose carriers heard at its MORPH are \
             all silent ends (engine.rs `active`): raising LEVEL later doesn't revive the held \
             note",
        );
    }
    if c.is_op(AlgoOpParams::AR) && c.any(|v| v == 0.0) {
        return Some("AR 0 never attacks (the TX81Z's rate 0)");
    }
    if c.is_op(AlgoOpParams::D2R) && c.any(|v| v == 31.0) {
        return Some("D2R 31 decays the held note to silence within its first 10 ms");
    }
    None
}

/// The pairs `c` may put sound on: its OUT values' pairs, and pair 1 for
/// the FX return when it's wet.
fn expected_pairs(c: &Case) -> [bool; DAC_PAIRS] {
    let mut e = [false; DAC_PAIRS];
    if c.is(BlockRef::Part, PartParams::OUTPUT) {
        for v in c.values() {
            e[v as usize] = true;
        }
    } else {
        e[DacPair::P1.index()] = true;
    }
    if c.wet {
        e[0] = true;
    }
    e
}

/// Why a transient at a jump or a release is the parameter's own sound.
fn clicks_ok(c: &Case, label: &str) -> Option<&'static str> {
    if c.is(BlockRef::Part, PartParams::OUTPUT) {
        return Some("OUT is a discrete choice: the Part leaves one pair for another at once");
    }
    if c.param.block == BlockRef::Pitch {
        return Some("a pitch jump retunes a string's loop at once");
    }
    if c.is(BlockRef::Filter, FilterParams::CUTOFF) && c.any(|v| v == 20.0) {
        return Some(
            "CUTOFF moves ten octaves within one block's ramp (#53): at 20 Hz the SVF holds its \
             state and lets it go, the filter's own thump",
        );
    }
    if c.is_op(AlgoOpParams::RR) && label == "chord-off" && c.any(|v| v == 15.0) {
        return Some("RR 15 is the fastest release: a note-off at full level ends within a few ms");
    }
    None
}

/// Why `c` may carry DC to the DAC.
fn dc_ok(c: &Case) -> Option<&'static str> {
    // What sets the spectrum: the algorithm, and each operator's wave,
    // ratio, detune and feedback.
    let spectral = [
        AlgoOpParams::WAVE,
        AlgoOpParams::COARSE,
        AlgoOpParams::FINE,
        AlgoOpParams::DETUNE,
        AlgoOpParams::FEEDBACK,
    ];
    if c.param.block == BlockRef::Algo || spectral.iter().any(|&id| c.is_op(id)) {
        return Some(
            "FM: a ratio, detune or feedback puts a sideband near 0 Hz (c − k·m ≈ 0), below \
             the voice's 5 Hz blocker (ADR 0060)",
        );
    }
    if c.patch == Patch::ModalInit(ResonatorMode::Bowed) && c.param.block == BlockRef::Folder {
        return Some(
            "the bow's attack: the string's static deflection settles, and its Helmholtz motion \
             grows, at the loop's rate, 12 periods (92 ms at C3); the fold rectifies the growing \
             asymmetric wave, which the voice's 5 Hz blocker has not settled 95 ms in (a C3 \
             note 0.003, the four-note chord 0.0065, gone 0.3 s in). Settled, the bow's output \
             under 10 Hz is at least 69 dB under its RMS, G1 to C7 \
             (`a_settled_bow_does_not_drift`, #248)",
        );
    }
    if c.patch == Patch::ModalInit(ResonatorMode::Sympathetic) && c.param.block == BlockRef::Drive {
        return Some(
            "SYMP's halo is a chord (Rings' table, pairs a cent apart): the drive's difference \
             tones between its strings fall below the voice's 5 Hz blocker (0.0048 on P1 before \
             Task 18 made up the halo's low-pass loss, 0.0051 after; 0.0016 with HALO 0)",
        );
    }
    None
}

/// Why a voice may stay busy on a silent bus past the bound.
fn zombie_ok(c: &Case) -> Option<&'static str> {
    if c.rings_on() {
        return Some(
            "BANK, BOWED and SYMP ring on after note-off; the engine judges its own ring, before \
             the filter and the VCA, so a setting that silences it after the engine (OUT LEVEL \
             0, a filter MODE) leaves the voice busy until the ring decays",
        );
    }
    None
}

/// Why the tail may hold its level to the bound, yet not be stuck: it
/// still falls (`last < first`), however slowly.
fn stuck_ok(c: &Case, (first, last): (f32, f32)) -> Option<&'static str> {
    if last < first
        && c.rings_on()
        && c.is(BlockRef::Folder, FolderParams::FOLD)
        && c.any(|v| v >= 0.75)
    {
        return Some(
            "FOLD's gain (×4.4 at 0.75, ×7 at 1) folds a ringing model's tail back up to full \
             level: the ring decays at DAMP's T60 beneath it",
        );
    }
    None
}

/// Why the tail may outlast the bound (it still decays).
fn long_tail(c: &Case) -> Option<&'static str> {
    if c.rings_on() {
        return Some("BANK, BOWED and SYMP ring on at DAMP's T60 after note-off (ADR 0054, 0056)");
    }
    if c.modal(ModalParams::DAMP) {
        return Some("DAMP sets the ring's T60, up to 20 s");
    }
    if c.is_op(AlgoOpParams::RR) || c.is_op(AlgoOpParams::D2R) || c.is_op(AlgoOpParams::D1R) {
        return Some("a slow operator release rings on");
    }
    if matches!(c.param.block, BlockRef::Env(_) | BlockRef::Lfo(_)) {
        return Some("ENV 2 drives the busy patches' VCA: its release and times hold the note");
    }
    None
}

// ── Known defects ──────────────────────────────────────────────────────

/// A defect the sweep found, left for its fix: its ignored `defect_*`
/// test fails until then. None open: Task 17 fixed the last (the voice's
/// and the Modal engines' DC, snapped settings, a silent operator holding
/// its voice).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Defect {}

impl Defect {
    fn what(self) -> &'static str {
        match self {}
    }
}

fn known(_c: &Case, _kind: &Kind, _label: &str) -> Option<Defect> {
    None
}

// ── Verdicts ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    NotFinite,
    OverCeiling,
    Dc,
    Click,
    Stuck,
    Zombie,
    LongTail,
    Silent,
    Leak,
    OverBudget,
}

#[derive(Clone, Debug)]
struct Finding {
    kind: Kind,
    case: usize,
    what: String,
}

#[derive(Default)]
struct Verdict {
    /// Unexpected: each fails the sweep.
    findings: Vec<Finding>,
    known: BTreeMap<Defect, Vec<Finding>>,
    /// Each allowance used, and on how many cases.
    allowed: BTreeMap<(Kind, &'static str), usize>,
    long_tails: usize,
}

fn judge(cases: &[Case], runs: &[Run], t: Timing) -> Verdict {
    let mut v = Verdict::default();
    for (i, (c, r)) in cases.iter().zip(runs).enumerate() {
        let mut out: Vec<(Kind, String, &str)> = Vec::new();
        let mut allowed: Vec<(Kind, &'static str)> = Vec::new();
        if !r.finite {
            out.push((Kind::NotFinite, "NaN or inf on a pair".into(), ""));
        }
        for k in 0..DAC_PAIRS {
            if !within_ceiling(r.peak[k]) {
                out.push((
                    Kind::OverCeiling,
                    format!("P{} peak {:.4}", k + 1, r.peak[k]),
                    "",
                ));
            }
            if matches!(c.mv, Move::Static(_)) && r.dc[k] > DC_MAX {
                match dc_ok(c) {
                    Some(why) => allowed.push((Kind::Dc, why)),
                    None => out.push((Kind::Dc, format!("P{} DC {:.4}", k + 1, r.dc[k]), "")),
                }
            }
        }
        for &(label, k, ratio) in &r.clicks {
            // Wet, an echo of an onset lands anywhere: the voice's own
            // events are judged dry.
            if c.wet && !label.starts_with("jump") {
                continue;
            }
            match clicks_ok(c, label) {
                Some(why) => allowed.push((Kind::Click, why)),
                None => out.push((
                    Kind::Click,
                    format!("{label} on P{}: {ratio:.1}× the steady state", k + 1),
                    label,
                )),
            }
        }
        let expected = expected_pairs(c);
        for k in 0..DAC_PAIRS {
            if !expected[k] && r.peak[k] > LEAK_PEAK {
                let what = format!(
                    "P{} carries {:.2e} with no Part routed there",
                    k + 1,
                    r.peak[k]
                );
                out.push((Kind::Leak, what, ""));
            }
        }
        let heard = |w: usize| {
            (0..DAC_PAIRS)
                .filter(|&k| expected[k])
                .map(|k| r.held[w][k] * r.held[w][k])
                .sum::<f32>()
                .sqrt()
        };
        if let Some(w) = (0..2).find(|&w| heard(w) < SILENT_RMS) {
            match silences(c) {
                Some(why) => allowed.push((Kind::Silent, why)),
                None => {
                    let what = format!("{} held RMS {:.2e}", ["single note", "chord"][w], heard(w));
                    out.push((Kind::Silent, what, ""));
                }
            }
        }
        if r.freed.is_none() {
            let (first, last) = r.tail;
            if last <= 1e-6 {
                match zombie_ok(c) {
                    Some(why) => allowed.push((Kind::Zombie, why)),
                    None => out.push((
                        Kind::Zombie,
                        format!(
                            "busy {} blocks after release on a silent bus ({last:.1e})",
                            t.bound
                        ),
                        "",
                    )),
                }
            } else if last >= first * 0.89 {
                match stuck_ok(c, (first, last)) {
                    Some(why) => allowed.push((Kind::Stuck, why)),
                    None => out.push((
                        Kind::Stuck,
                        format!(
                            "not freed {} blocks after release; tail {first:.2e} → {last:.2e}",
                            t.bound
                        ),
                        "",
                    )),
                }
            } else {
                v.long_tails += 1;
                match long_tail(c) {
                    Some(why) => allowed.push((Kind::LongTail, why)),
                    None => out.push((
                        Kind::LongTail,
                        format!(
                            "ringing {} blocks after release: {first:.2e} → {last:.2e}",
                            t.bound
                        ),
                        "",
                    )),
                }
            }
        }
        if r.over_blocks > 0 {
            let what = format!(
                "billed {} over the budget {} for {} blocks, unshed",
                r.bill_max, r.budget, r.over_blocks
            );
            out.push((Kind::OverBudget, what, ""));
        }
        for a in allowed {
            *v.allowed.entry(a).or_default() += 1;
        }
        for (kind, what, label) in out {
            let known = known(c, &kind, label);
            let f = Finding {
                kind,
                case: i,
                what,
            };
            match known {
                Some(d) => v.known.entry(d).or_default().push(f),
                None => v.findings.push(f),
            }
        }
    }
    v
}

fn summary(cases: &[Case], f: &[Finding]) -> String {
    let mut s = String::new();
    let mut by: BTreeMap<(Kind, String), Vec<String>> = BTreeMap::new();
    for x in f {
        let c = &cases[x.case];
        by.entry((
            x.kind.clone(),
            format!("{} on {}", c.param.name(), c.patch.name()),
        ))
        .or_default()
        .push(format!("{:?}: {}", c.mv, x.what));
    }
    for ((k, who), v) in &by {
        let _ = writeln!(s, "{k:?} {who}: {} ({} cases)", v[0], v.len());
    }
    s
}

struct Sweep {
    cases: Vec<Case>,
    runs: Vec<Run>,
    verdict: Verdict,
    took: std::time::Duration,
}

fn sweep(quick: bool, t: Timing) -> Sweep {
    let cases = cases(quick);
    let t0 = std::time::Instant::now();
    let runs = run_all(&cases, t);
    let verdict = judge(&cases, &runs, t);
    Sweep {
        cases,
        runs,
        verdict,
        took: t0.elapsed(),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

/// The sweep reads the registry: every block with audio, and every one of
/// its parameters, is in it; Theme alone is left out.
#[test]
fn the_sweep_covers_the_registry() {
    let all = registry(false);
    for b in BlockRef::ALL {
        let n = all.iter().filter(|p| p.block == b).count();
        if NOT_AUDIO.contains(&b) {
            assert_eq!(n, 0, "{b:?}");
        } else {
            assert_eq!(n, b.specs().len(), "{b:?}");
        }
    }
    let quick: Vec<_> = cases(true).iter().map(|c| c.param.name()).collect();
    for p in registry(true) {
        assert!(quick.contains(&p.name()), "{} unswept", p.name());
    }
    // `Bench::set` reaches the value the Sound or Performance holds.
    for p in registry(false) {
        let (patch, wet) = patches(&p, false)[0];
        let c = Case {
            param: p,
            patch,
            mv: Move::Static(p.spec.max),
            wet,
        };
        let mut b = c.bench();
        b.set(&p, p.spec.max);
        assert_eq!(b.get(&p), p.spec.quantize(p.spec.max), "{}", p.name());
    }
}

#[test]
fn sweep_fast() {
    let s = sweep(true, FAST);
    let v = &s.verdict;
    println!(
        "{} parameters, {} cases in {:.1?}; {} known-defect findings",
        registry(true).len(),
        s.cases.len(),
        s.took,
        v.known.values().map(Vec::len).sum::<usize>()
    );
    let msg = summary(&s.cases, &v.findings);
    assert!(
        v.findings.is_empty(),
        "{} findings:\n{msg}",
        v.findings.len()
    );
}

#[test]
#[ignore = "thorough: every instance at five points on every Sound (~2 min debug); cargo test -- --ignored"]
fn sweep_thorough() {
    let s = sweep(false, THOROUGH);
    let v = &s.verdict;
    println!(
        "{} parameters, {} cases in {:.1?}; {} known-defect findings",
        registry(false).len(),
        s.cases.len(),
        s.took,
        v.known.values().map(Vec::len).sum::<usize>()
    );
    if let Ok(path) = std::env::var("PARAM_SWEEP_REPORT") {
        std::fs::write(&path, report(&s)).expect("the report");
        println!("report: {path}");
    }
    let msg = summary(&s.cases, &v.findings);
    assert!(
        v.findings.is_empty(),
        "{} findings:\n{msg}",
        v.findings.len()
    );
}

/// OUT moves Part 1's dry signal to its pair whole, and only there; the
/// FX return stays on pair 1. Summed, the pairs are what OUT = P1 plays.
#[test]
fn out_moves_the_part_to_its_pair() {
    let out = find(BlockRef::Part, PartParams::OUTPUT);
    for wet in [false, true] {
        let tape = |pair: f32| {
            let c = Case {
                param: out,
                patch: Patch::AlgoInit,
                mv: Move::Static(pair),
                wet,
            };
            let mut b = c.bench();
            b.set(&out, pair);
            let mut t = Tape::default();
            for blk in 0..400 {
                if blk == 0 {
                    b.note(60, Some(100));
                }
                if blk == 200 {
                    b.note(60, None);
                }
                b.render(&mut t);
            }
            t
        };
        let home = tape(0.0);
        for k in 1..DAC_PAIRS {
            let moved = tape(k as f32);
            let silent = |p: usize| moved.out[p].iter().all(|&x| x == 0.0);
            assert!(silent(3 - k), "wet {wet}: P{} silent", 4 - k);
            assert!(!silent(k), "wet {wet}: the Part on P{}", k + 1);
            if wet {
                for (i, &x) in home.out[0].iter().enumerate() {
                    let sum = moved.out[0][i] + moved.out[k][i];
                    assert!(
                        (sum - x).abs() < 1e-6,
                        "wet: P1 + P{} is OUT P1's, at {i}",
                        k + 1
                    );
                }
            } else {
                assert!(silent(0), "dry: P1 empty once OUT moves");
                assert_eq!(
                    moved.out[k],
                    home.out[0],
                    "dry: P{} plays what P1 did",
                    k + 1
                );
            }
        }
    }
}

/// Every INIT and patch: finite and under the ceiling on every pair.
#[test]
fn levels_are_safe() {
    for p in level_patches() {
        for notes in [&[60u8][..], &CHORD[..]] {
            let l = level(p, notes, 127, 300);
            for peak in l.peak {
                assert!(peak.is_finite() && within_ceiling(peak), "{}", p.name());
            }
            assert!(l.rms[0] > SILENT_RMS, "{} sounds", p.name());
        }
    }
}

/// ADR 0058's reference: ALGO INIT's C4 at velocity 100 on P1, LUFS.
const REFERENCE_LUFS: f32 = -15.0;

/// ADR 0058: ALGO INIT sits at the reference, ±0.1 dB, and each Modal
/// model's INIT, one C4 at velocity 100, within ±1 dB of it. The limiter
/// is a safety ceiling (ADR 0050): no INIT's chord at velocity 127, the
/// hardest strike, loses more than `MAX_CHORD_LIMITED_DB` to it. The wider
/// voicings (`WIDE_5`, `WIDE_8`) are printed, not gated (`--nocapture`).
#[test]
fn modal_models_match_the_loudness_reference() {
    let algo = level(Patch::AlgoInit, &[60], 100, BPS).lufs;
    assert!(
        (algo - REFERENCE_LUFS).abs() <= 0.1,
        "ALGO INIT at {algo:.2} LUFS, not the reference's {REFERENCE_LUFS}"
    );
    let mut bad = Vec::new();
    for m in MODELS {
        let p = Patch::ModalInit(m);
        let d = level(p, &[60], 100, BPS).lufs - REFERENCE_LUFS;
        let chord = level(p, &CHORD, 127, BPS);
        println!(
            "{}: {d:+.2} dB; chord at 127: gain reduction {:.1} dB at most, {:.2} dB of its \
             loudness",
            p.name(),
            chord.gr_db,
            chord.limited_db
        );
        // Reported, not gated: the owner's loudness reference decides these.
        for wide in [&WIDE_5[..], &WIDE_8[..]] {
            let l = level(p, wide, 127, BPS);
            println!(
                "  {}-note chord at 127: gain reduction {:.1} dB at most, {:.2} dB of its \
                 loudness",
                wide.len(),
                l.gr_db,
                l.limited_db
            );
        }
        if d.abs() > 1.0 {
            bad.push(format!("{}: {d:+.2} dB off the reference", p.name()));
        }
        if chord.limited_db > MAX_CHORD_LIMITED_DB {
            bad.push(format!(
                "{}: the limiter took {:.2} dB of the chord at 127",
                p.name(),
                chord.limited_db
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Wider v127 voicings, reported beside `CHORD` (objective QA, Task 18):
/// a five-note open chord and eight notes over three octaves.
const WIDE_5: [u8; 5] = [36, 48, 55, 60, 64];
const WIDE_8: [u8; 8] = [36, 43, 48, 52, 55, 60, 64, 72];

/// The loudness the limiter may take from an INIT's chord at velocity 127,
/// dB: a safety ceiling may duck a strike, not hold the note down.
const MAX_CHORD_LIMITED_DB: f32 = 3.0;

const MODELS: [ResonatorMode; 4] = [
    ResonatorMode::String,
    ResonatorMode::Modal,
    ResonatorMode::Bowed,
    ResonatorMode::Sympathetic,
];

// ── The defects Task 17 fixed, pinned ───────────────────────────────────

fn find(block: BlockRef, id: ParamId) -> Param {
    registry(false)
        .into_iter()
        .find(|p| p.is(block, id))
        .unwrap()
}

/// The folder's SYM biases its fold, so at SYM 0 a note's fold is
/// asymmetric: no DC reaches the DAC, held or through the silent release,
/// so pair 1 doesn't step as the voice frees (ADR 0060).
#[test]
fn the_folder_sym_puts_no_dc_on_the_dac() {
    let mut b = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::default());
    b.set(&find(BlockRef::Folder, FolderParams::FOLD), 0.5);
    b.set(&find(BlockRef::Folder, FolderParams::SYMMETRY), 0.0);
    let mut t = Tape::default();
    for blk in 0..1000 {
        if blk == 0 {
            b.note(60, Some(100));
        }
        if blk == 150 {
            b.note(60, None);
        }
        b.render(&mut t);
    }
    let end = 150 + t.freed(150).expect("freed");
    let held = t.dc(0, DC_FROM, 150);
    let release = t.dc(0, end - 40, end - 1);
    assert!(
        held < DC_MAX && release < DC_MAX,
        "held DC {held:.4}, silent release DC {release:.4}"
    );
}

/// Each case's static DC on P1 over `DC_MAX`, as the sweep reads it.
fn dc_over(cases: &[(Patch, Param, f32, Timing)]) -> Vec<String> {
    cases
        .iter()
        .filter_map(|&(patch, p, v, t)| {
            let c = Case {
                param: p,
                patch,
                mv: Move::Static(v),
                wet: false,
            };
            let dc = play(&mut c.bench(), Some(&p), c.mv, t).dc[0];
            (dc > DC_MAX).then(|| format!("{}: DC {dc:.4}", c.label()))
        })
        .collect()
}

/// A bright or fast bow drifts below the engine's 10 Hz blocker; the
/// voice's blocker takes it (ADR 0060).
#[test]
fn a_bright_bow_puts_no_dc_on_the_dac() {
    let bowed = Patch::ModalInit(ResonatorMode::Bowed);
    let bad = dc_over(&[
        (bowed, find(BlockRef::Modal, ModalParams::BRIGHT), 1.0, FAST),
        (
            bowed,
            find(BlockRef::Modal, ModalParams::BRIGHT),
            0.75,
            THOROUGH,
        ),
        (bowed, find(BlockRef::Modal, ModalParams::SPEED), 1.0, FAST),
    ]);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// SYMP's pluck is zero-mean, so its halo gains no 0 Hz mode to drift
/// under the output blocker; BANK's output has the blocker too.
#[test]
fn the_modal_engines_put_no_dc_on_the_dac() {
    let symp = Patch::ModalInit(ResonatorMode::Sympathetic);
    let bank = Patch::ModalInit(ResonatorMode::Modal);
    let bad = dc_over(&[
        (symp, find(BlockRef::Pitch, PitchParams::PITCH), 12.0, FAST),
        (symp, find(BlockRef::Pitch, PitchParams::PITCH), 24.0, FAST),
        (
            symp,
            find(BlockRef::Modal, ModalParams::COUPLE),
            1.0,
            THOROUGH,
        ),
        (
            symp,
            find(BlockRef::Modal, ModalParams::HALO),
            1.0,
            THOROUGH,
        ),
        (
            bank,
            find(BlockRef::Modal, ModalParams::STRUCTURE),
            1.0,
            FAST,
        ),
    ]);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Never snap: a jump of these continuous parameters eases, on the
/// triangle probe or on a Sound where the stage is in play. Each clicked
/// before Task 17.
#[test]
fn jumps_never_click() {
    let symp = Patch::ModalInit(ResonatorMode::Sympathetic);
    // Each (parameter, Sound, wet, timing) clicks today on its own.
    let params = [
        (
            find(BlockRef::Out, OutParams::VOLUME),
            Patch::Probe,
            false,
            FAST,
        ),
        (
            find(BlockRef::Part, PartParams::LEVEL),
            Patch::Probe,
            false,
            FAST,
        ),
        (
            find(BlockRef::Part, PartParams::PAN),
            Patch::Probe,
            false,
            FAST,
        ),
        (
            find(BlockRef::Part, PartParams::SEND_CHORUS),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Part, PartParams::SEND_DELAY),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Part, PartParams::SEND_REVERB),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Chorus, ChorusParams::DEPTH),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Chorus, ChorusParams::MODE),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Reverb, ReverbParams::MIX),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Chorus, ChorusParams::MIX),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Delay, DelayParams::MIX),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Delay, DelayParams::TIME_MS),
            Patch::Probe,
            true,
            FAST,
        ),
        (
            find(BlockRef::Folder, FolderParams::FOLD),
            symp,
            false,
            FAST,
        ),
        (
            find(BlockRef::Folder, FolderParams::SYMMETRY),
            Patch::BusyModal,
            false,
            THOROUGH,
        ),
        (find(BlockRef::Drive, DriveParams::DRIVE), symp, false, FAST),
        (
            find(BlockRef::Drive, DriveParams::MIX),
            Patch::BusyModal,
            false,
            THOROUGH,
        ),
        (
            find(BlockRef::Filter, FilterParams::DRIVE),
            symp,
            false,
            FAST,
        ),
    ];
    let mut bad = Vec::new();
    for (p, patch, wet, t) in params {
        for (from, to) in [(p.spec.min, p.spec.max), (p.spec.max, p.spec.min)] {
            let c = Case {
                param: p,
                patch,
                mv: Move::Jump { from, to },
                wet,
            };
            let r = play(&mut c.bench(), Some(&p), c.mv, t);
            let jumps: Vec<_> = r
                .clicks
                .iter()
                .filter(|x| x.0.starts_with("jump"))
                .collect();
            if !jumps.is_empty() {
                bad.push(format!("{}: {jumps:?}", c.label()));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// DRIVE's on/off at TONE's ends, where its wet at 0.001 is 2.5 dB under
/// the dry: the gate's 20 ms fade (ADR 0061) never clicks.
#[test]
fn drive_jumps_never_click_at_any_tone() {
    let symp = Patch::ModalInit(ResonatorMode::Sympathetic);
    let (drive, tone) = (
        find(BlockRef::Drive, DriveParams::DRIVE),
        find(BlockRef::Drive, DriveParams::TONE),
    );
    let mut bad = Vec::new();
    for patch in [symp, Patch::Probe] {
        for t in [0.0, 1.0] {
            for (from, to) in [(0.0, 1.0), (1.0, 0.0)] {
                let c = Case {
                    param: drive,
                    patch,
                    mv: Move::Jump { from, to },
                    wet: false,
                };
                let mut b = c.bench();
                b.set(&tone, t);
                let r = play(&mut b, Some(&drive), c.mv, FAST);
                let jumps: Vec<_> = r
                    .clicks
                    .iter()
                    .filter(|x| x.0.starts_with("jump"))
                    .collect();
                if !jumps.is_empty() {
                    bad.push(format!("TONE {t}, {}: {jumps:?}", c.label()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// ALGO INIT's operators 2–4 are carriers only in ALG B (A1): at MORPH 0
/// they're unheard once operator 1 has released, so a slow release on one
/// holds neither the voice nor its bill (23 s before Task 17).
#[test]
fn a_silent_operator_frees_the_voice() {
    let mut b = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::default());
    b.set(&find(BlockRef::AlgoOp(Op::B), AlgoOpParams::RR), 1.0);
    let mut t = Tape::default();
    for blk in 0..1500 {
        if blk == 0 {
            b.note(60, Some(100));
        }
        if blk == 150 {
            b.note(60, None);
        }
        b.render(&mut t);
    }
    let quiet = (150..t.blocks())
        .find(|&k| t.bus[k * 64..(k + 1) * 64].iter().all(|x| x.abs() < 1e-5))
        .expect("quiet");
    let freed = t.freed(150).map(|f| f + 150);
    assert!(
        freed.is_some_and(|f| f < quiet + BPS),
        "quiet at block {quiet}, freed {freed:?}"
    );
}

// ── The report ─────────────────────────────────────────────────────────

fn level_patches() -> Vec<Patch> {
    let mut v = SOUNDS.to_vec();
    v.extend((0..8).map(Patch::Factory));
    v
}

fn dbs(x: f32) -> String {
    if x <= 0.0 {
        "−∞".into()
    } else {
        format!("{:.1}", db(x))
    }
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// The owner's question: Modal against Algo at INIT.
fn modal_loudness(s: &mut String) {
    let pats = [
        Patch::AlgoInit,
        Patch::ModalInit(ResonatorMode::String),
        Patch::ModalInit(ResonatorMode::Modal),
        Patch::ModalInit(ResonatorMode::Sympathetic),
        Patch::ModalInit(ResonatorMode::Bowed),
    ];
    let conds: [(&str, &[u8]); 3] = [("C3", &[48]), ("C4", &[60]), ("chord", &CHORD)];
    let _ = writeln!(
        s,
        "Held 1 s, measured on P1 after the output stage (trim 1/√8, ceiling −1 dBFS); LUFS is \
         ungated BS.1770 (K-weighted) over the hold. Δ is LUFS against ALGO INIT at the same \
         notes and velocity. Bus RMS is Part 1's voices summed, before pan, Part LEVEL and the \
         trim.\n"
    );
    let _ = writeln!(
        s,
        "| Sound | vel | notes | peak dBFS | RMS dBFS | LUFS | Δ vs ALGO dB | bus RMS dBFS |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|");
    let mut algo = BTreeMap::new();
    let mut offs: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for p in pats {
        for vel in [64u8, 127] {
            for (name, notes) in conds {
                let l = level(p, notes, vel, BPS);
                if p == Patch::AlgoInit {
                    algo.insert((vel, name), l.lufs);
                }
                let d = l.lufs - algo[&(vel, name)];
                if p != Patch::AlgoInit {
                    offs.entry(p.name()).or_default().push(d);
                }
                let _ = writeln!(
                    s,
                    "| {} | {vel} | {name} | {} | {} | {:.1} | {d:+.1} | {} |",
                    p.name(),
                    dbs(l.peak[0]),
                    dbs(l.rms[0]),
                    l.lufs,
                    dbs(l.bus_rms)
                );
            }
        }
    }
    let _ = writeln!(
        s,
        "\n| Model | mean Δ vs ALGO INIT dB | range dB |\n|---|---|---|"
    );
    for (m, v) in offs {
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let (lo, hi) = v
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        let _ = writeln!(s, "| {m} | {mean:+.1} | {lo:+.1} … {hi:+.1} |");
    }
}

/// Every INIT and patch: RMS and peak per pair, LUFS on P1, flagged
/// ±6 dB from the median.
fn levels(s: &mut String) {
    let rows: Vec<(Patch, Level, Level)> = level_patches()
        .into_iter()
        .map(|p| (p, level(p, &[60], 100, BPS), level(p, &CHORD, 100, BPS)))
        .collect();
    let med = median(rows.iter().map(|r| r.1.lufs).collect());
    let _ = writeln!(
        s,
        "C4 and the chord (C3 E3 G3 C4) at velocity 100, held 1 s, OUT P1, FX off. Median \
         single-note loudness {med:.1} LUFS; a flag marks ±6 dB from it. P2 and P3 carry \
         nothing with OUT on P1 (−∞ is exact silence).\n"
    );
    let _ = writeln!(
        s,
        "| Sound | P1 RMS | P1 peak | P2 RMS | P3 RMS | LUFS | vs median | chord P1 RMS | chord peak | chord LUFS | flag |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|---|---|---|");
    for (p, one, ch) in rows {
        let d = one.lufs - med;
        let flag = match d {
            d if d > 6.0 => "HOT",
            d if d < -6.0 => "QUIET",
            _ => "",
        };
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} | {} | {:.1} | {d:+.1} | {} | {} | {:.1} | {flag} |",
            p.name(),
            dbs(one.rms[0]),
            dbs(one.peak[0]),
            dbs(one.rms[1]),
            dbs(one.rms[2]),
            one.lufs,
            dbs(ch.rms[0]),
            dbs(ch.peak[0]),
            ch.lufs,
        );
    }
}

fn report(sw: &Sweep) -> String {
    let (cases, runs, v) = (&sw.cases, &sw.runs, &sw.verdict);
    let mut s = String::new();
    let _ = writeln!(s, "## Modal against Algo at INIT\n");
    modal_loudness(&mut s);
    let _ = writeln!(s, "\n## Levels\n");
    levels(&mut s);

    let _ = writeln!(s, "\n## Sweep\n");
    let statics = cases
        .iter()
        .filter(|c| matches!(c.mv, Move::Static(_)))
        .count();
    let _ = writeln!(
        s,
        "{} parameters (every block instance in `BlockRef::ALL` but Theme), {} cases ({statics} \
         static, {} jumps) in {:.1?}, timing {:?}. Unexpected findings: {}. Known-defect \
         findings: {}. Long tails (still decaying at the bound): {}.\n",
        registry(false).len(),
        cases.len(),
        cases.len() - statics,
        sw.took,
        THOROUGH,
        v.findings.len(),
        v.known.values().map(Vec::len).sum::<usize>(),
        v.long_tails,
    );
    let _ = writeln!(
        s,
        "### Known defects\n\n| Defect | cases | example |\n|---|---|---|"
    );
    for (d, f) in &v.known {
        let c = &cases[f[0].case];
        let _ = writeln!(
            s,
            "| {} | {} | {}: {} |",
            d.what(),
            f.len(),
            c.label(),
            f[0].what
        );
    }
    if !v.findings.is_empty() {
        let _ = writeln!(
            s,
            "\n### Unexpected\n\n```\n{}```",
            summary(cases, &v.findings)
        );
    }
    let _ = writeln!(
        s,
        "\n### Allowed, and why\n\n| check | reason | cases |\n|---|---|---|"
    );
    for ((k, why), n) in &v.allowed {
        let _ = writeln!(s, "| {k:?} | {why} | {n} |");
    }

    let _ = writeln!(s, "\n### Routing moves (Part 1 off P1)\n");
    let _ = writeln!(s, "Chord held RMS per pair, dBFS.\n");
    let _ = writeln!(s, "| case | P1 | P2 | P3 |\n|---|---|---|---|");
    for (c, r) in cases.iter().zip(runs) {
        if c.is(BlockRef::Part, PartParams::OUTPUT) && c.any(|v| v != 0.0) {
            let [a, b, d] = r.held[1];
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} |",
                c.label(),
                dbs(a),
                dbs(b),
                dbs(d)
            );
        }
    }

    let _ = writeln!(s, "\n### CPU\n");
    let budget = runs.iter().map(|r| r.budget).max().unwrap_or(0);
    let over = runs.iter().filter(|r| r.over_blocks > 0).count();
    let short = runs
        .iter()
        .filter(|r| (r.most_busy as usize) < CHORD.len())
        .count();
    let _ = writeln!(
        s,
        "Budget {budget} cycles/sample (rev V, 70 %), {} of it reserved for the FX bus. Cases \
         billed over the budget without shedding: {over}. Cases whose chord got fewer than {} \
         voices: {short}.\n",
        FxBus::COST.0,
        CHORD.len()
    );
    let mut by: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (c, r) in cases.iter().zip(runs) {
        let e = by.entry(c.patch.name()).or_default();
        e.1 += 1;
        if (r.most_busy as usize) < CHORD.len() {
            e.0 += 1;
        }
    }
    let _ = writeln!(
        s,
        "| Sound | cases with a short chord | cases |\n|---|---|---|"
    );
    for (p, (n, of)) in by {
        let _ = writeln!(s, "| {p} | {n} | {of} |");
    }
    let _ = writeln!(s);
    let mut top: Vec<(&Case, &Run)> = cases.iter().zip(runs).collect();
    top.sort_by_key(|(_, r)| std::cmp::Reverse(r.voice_cost));
    let _ = writeln!(
        s,
        "| costliest settings | voice cost | max bill | most voices |\n|---|---|---|---|"
    );
    let mut seen = Vec::new();
    for (c, r) in top {
        let key = (c.param.name(), c.patch.name());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let _ = writeln!(
            s,
            "| {} | {} | {} | {} |",
            c.label(),
            r.voice_cost,
            r.bill_max,
            r.most_busy
        );
        if seen.len() == 12 {
            break;
        }
    }
    s
}
