//! Slice 1's QA gate for ORBIT (Task 11): the init scene played from a NEW
//! project on the desktop's real mix path, measured per DAC pair and
//! summed. Clicks are judged where sound can break (a ring's off, a steal,
//! STOP, MUTATE's boundary, a BPM jump), never at a note-on; stuck notes
//! are checked every block while running, and after STOP.
//!
//! With `ORBIT_QA_WAV=dir` set, `ab_renders` writes `a-new.wav` (NEW's
//! INIT Parts) and `b-factory.wav` (factory Sounds on Parts 1–5) for UAT.

mod common;

use std::fmt::Write as _;
use std::sync::OnceLock;

use chimera_core::addr::BlockRef;
use chimera_core::block::{ParamId, ParamSpec};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::hw::DAC_PAIRS;
use chimera_core::orbit::RingIdx;
use chimera_core::orbit::clock::Bpm;
use chimera_core::orbit::euclid::{Fill, Steps};
use chimera_core::orbit::rng::Pct;
use chimera_core::orbit::scene::{Scene, SceneRing};
use chimera_core::part::DacPair;
use chimera_core::part::PartParams;
use chimera_core::project::PartId;
use common::orbit::{Mark, OrbitRig, Report, STEADY, orbit_block, report, secs, write_wav};
use common::sweep::{BPS, Bench, Patch, SILENT_RMS, Tape, level, steady_windows};

/// Loops of the init scene (one bar each at 120 BPM: 1,500 blocks).
const LOOP: usize = 1500;

/// The level reference: INIT Algo's C4 triad, held 2 s.
const C4_CHORD: [u8; 3] = [60, 64, 67];

fn assert_clean(name: &str, r: &Report) {
    r.print(name);
    let f = r.faults();
    assert!(f.is_empty(), "{name}:\n{}", f.join("\n"));
}

/// How long after STOP every voice must be free: INIT Algo's longest
/// release, measured once, plus 1 s.
fn stop_bound() -> usize {
    static BOUND: OnceLock<usize> = OnceLock::new();
    *BOUND.get_or_init(|| longest_release() + secs(1.0))
}

/// Blocks from a released chord at velocity 127 until its voices are free.
fn longest_release() -> usize {
    let mut b = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::default());
    let mut tape = Tape::default();
    for &n in &C4_CHORD {
        b.note(n, Some(127));
    }
    for _ in 0..secs(1.0) {
        b.render(&mut tape);
    }
    for &n in &C4_CHORD {
        b.note(n, None);
    }
    let off = tape.blocks();
    for _ in 0..secs(20.0) {
        b.render(&mut tape);
        if b.all_free() {
            break;
        }
    }
    tape.freed(off).expect("INIT's release ends")
}

fn stop_and_drain(rig: &mut OrbitRig, bound: usize) -> Option<usize> {
    rig.play(false);
    for _ in 0..bound + 2 {
        rig.block();
        if rig.freed_after_stop().is_some() {
            break;
        }
    }
    rig.freed_after_stop()
}

#[test]
fn init_scene_is_clean() {
    let bound = stop_bound();
    let mut rig = OrbitRig::new();
    rig.play(true);
    rig.run_loops(10);
    // A live chord on ring 1's Part, held two loops and released.
    for n in [57, 60, 64] {
        rig.key(0, n, true);
    }
    rig.run_loops(2);
    for n in [57, 60, 64] {
        rig.key(0, n, false);
    }
    rig.run_loops(52);
    assert_eq!(rig.view().loops, 65, "64 whole loops");
    // STOP between steps, so its break is judged apart from an attack.
    rig.run(40);
    let freed = stop_and_drain(&mut rig, bound);
    let r = report(&rig, 0);
    assert_clean("init scene, 64 loops", &r);
    assert!(
        freed.is_some_and(|f| f <= bound),
        "STOP freed every voice in {freed:?} blocks, bound {bound}"
    );
    // Ring offs are judged in numbers, and STOP. A steal is taken at the
    // note-on that needs the voice, and MUTATE's boundary is the downbeat,
    // where rings 1 and 2 always play: both share their block with an
    // attack, so `a_steal_fades_clean_on_its_pair` and the sweep judge them.
    assert!(r.judged[0] > 500 && r.judged[2] == 1, "{:?}", r.judged);
}

/// Ring `solo` alone (unmuted), the others muted; `None`: the scene as is.
fn render_levels(solo: Option<usize>, loops: u32) -> Report {
    let mut rig = OrbitRig::new();
    if let Some(s) = solo {
        for (i, sr) in rig.song().live.rings.iter_mut().enumerate() {
            sr.ring.mute = i != s;
        }
    }
    rig.play(true);
    rig.run_loops(loops);
    // From the second loop: past the first attacks' DC settling.
    report(&rig, LOOP)
}

#[test]
fn init_scene_levels() {
    let reference = level(Patch::AlgoInit, &C4_CHORD, 100, 2 * BPS).lufs;
    let all = render_levels(None, 16);
    all.print("init scene");
    println!("reference: INIT Algo C4 chord held, {reference:.1} LUFS");
    let solos: Vec<f32> = (0..5)
        .map(|i| {
            let r = render_levels(Some(i), 16);
            println!(
                "ring {} solo: P1 {:.1} LUFS, sum {:.1}, peak {:.3}",
                i + 1,
                r.lufs[0],
                r.sum_lufs,
                r.peak[0]
            );
            r.sum_lufs
        })
        .collect();
    let mut bad = Vec::new();
    for k in (0..DAC_PAIRS).filter(|&k| all.carrying()[k]) {
        if (all.lufs[k] - reference).abs() > 3.0 {
            bad.push(format!("P{} {:.1} LUFS", k + 1, all.lufs[k]));
        }
    }
    if (all.sum_lufs - reference).abs() > 3.0 {
        bad.push(format!("sum {:.1} LUFS", all.sum_lufs));
    }
    assert!(
        bad.is_empty(),
        "outside {reference:.1} ± 3 dB: {}",
        bad.join(", ")
    );
    // The rings the init scene plays sit within 6 dB of each other.
    let init = Scene::init();
    let playing: Vec<f32> = (0..5)
        .filter(|&i| !init.rings[i].ring.mute)
        .map(|i| solos[i])
        .collect();
    let (lo, hi) = playing
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    assert!(hi - lo <= 6.0, "ring solos span {:.1} dB", hi - lo);
    // Ring 1 carries the bass: the lowest register of the rings that play.
    let rings = &init.rings;
    let low = rings[0].ring.octave.get();
    assert!(
        rings
            .iter()
            .skip(1)
            .filter(|r| !r.ring.mute)
            .all(|r| r.ring.octave.get() > low)
    );
}

/// A seeded LCG: the offsets are the same every run.
fn lcg(x: &mut u32) -> u32 {
    *x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *x >> 8
}

#[test]
fn stop_leaves_nothing_sounding() {
    let bound = stop_bound();
    let mut seed = 0x0B17_5709;
    for _ in 0..10 {
        let at = BPS + lcg(&mut seed) as usize % (8 * LOOP);
        let mut rig = OrbitRig::new();
        rig.play(true);
        rig.run(at);
        let freed = stop_and_drain(&mut rig, bound);
        let r = report(&rig, 0);
        println!(
            "STOP at block {at}: freed in {freed:?} blocks (bound {bound}), refused {}",
            r.refused
        );
        assert!(freed.is_some_and(|f| f <= bound), "STOP at {at}: {freed:?}");
        assert_clean(&format!("STOP at {at}"), &r);
    }
}

#[test]
fn stress_is_clean() {
    let bound = stop_bound();
    let mut rig = OrbitRig::new();
    {
        let song = rig.song();
        song.bpm = Bpm::new(300).unwrap();
        for sr in &mut song.live.rings {
            let r = &mut sr.ring;
            *r = r.with_fill(Fill::new(Steps::MAX, 32, 0).unwrap());
            r.mute = false;
            r.mutate = Pct::FULL;
            r.gate = Pct::FULL;
            r.chance = Pct::FULL;
        }
    }
    rig.play(true);
    rig.run_loops(16);
    // Every ring fires on every step, so every off and steal shares a block
    // with an attack: STOP, between steps, is the one break judged.
    rig.run(10);
    let freed = stop_and_drain(&mut rig, bound);
    let r = report(&rig, 0);
    assert_eq!(r.judged[2], 1, "STOP judged");
    assert_clean("stress: 5 × 32 steps, MUTATE and GATE 100 %, 300 BPM", &r);
    assert!(freed.is_some_and(|f| f <= bound), "STOP freed in {freed:?}");
}

/// NEW's Parts spread over the pairs: ring 1's on P1, ring 2's on P2,
/// rings 3 and 5's on P3. Each pair is bounded on its own, and none sits
/// more than 6 dB from the others.
#[test]
fn init_scene_across_pairs() {
    let mut rig = OrbitRig::new();
    for (p, pair) in [(1, DacPair::P2), (2, DacPair::P3), (4, DacPair::P3)] {
        rig.project.edit_part(PartId::ALL[p]).mix.output = pair;
    }
    rig.play(true);
    rig.run_loops(16);
    let r = report(&rig, LOOP);
    assert_eq!(r.carrying(), [true; DAC_PAIRS], "every pair plays");
    assert_clean("init scene across P1–P3", &r);
}

/// A steal's fade, apart from the attack that causes it. Part 1 (on P1)
/// holds eight notes, the whole pool; Part 2, routed to P2, plays one. The
/// voice it takes fades on P1, where nothing starts, so P1 is judged at
/// the steal; the new note sounds on P2. Once with the victim held, once
/// with it in its release (a tail, which the allocator takes first).
#[test]
fn a_steal_fades_clean_on_its_pair() {
    let mut judged = 0;
    for released in [false, true] {
        let mut rig = OrbitRig::new();
        rig.project.edit_part(PartId::ALL[1]).mix.output = DacPair::P2;
        for n in [48, 52, 55, 59, 60, 64, 67, 71] {
            rig.key(0, n, true);
        }
        rig.run(secs(1.0));
        let mut prev = 0;
        if released {
            rig.key(0, 48, false);
            prev = rig.blocks();
            rig.run(100);
        }
        let e = rig.blocks();
        rig.key(1, 72, true);
        rig.run(secs(1.0));
        let case = if released { "a tail" } else { "a held note" };
        assert!(
            rig.marks.contains(&(e, Mark::Steal)),
            "{case}: Part 2's note stole a voice"
        );
        assert!(rig.tape.peak(1) > 1e-3, "{case}: the new note on P2");
        let (pre, post) = steady_windows(e, (prev, usize::MAX), true);
        if pre.len().max(post.len()) >= STEADY {
            judged += 1;
        }
        let click = rig.tape.click(0, e, (prev, usize::MAX), true);
        assert_eq!(click, None, "{case}: the stolen voice's fade on P1");
    }
    assert_eq!(judged, 2, "both steals judged");
}

// ── The parameter sweep, through a running ORBIT ───────────────────────

/// Floors on the sweep's judged MUTATE boundaries and BPM jumps, about
/// three quarters of what it judges (277 and 15, 2026-10-03): a change that
/// moved them all next to a note-on would leave them unjudged.
const MUTATE_JUDGED: usize = 200;
const BPM_JUDGED: usize = 10;

/// What a parameter may legitimately do, with the reason.
fn silences(b: BlockRef, id: ParamId, v: f32) -> Option<&'static str> {
    match b {
        BlockRef::OrbitRing(_) if id == SceneRing::MUTE && v != 0.0 => {
            Some("MUTE silences its ring")
        }
        BlockRef::OrbitRing(_) if id == SceneRing::CHANCE && v == 0.0 => {
            Some("CHANCE 0 never fires a hit")
        }
        BlockRef::OrbitRing(_) if id == SceneRing::HITS && v == 0.0 => Some("HITS 0 lays no hit"),
        _ => None,
    }
}

/// The values a parameter is swept through: min, an interior value, max.
fn points(s: &ParamSpec) -> [f32; 3] {
    [s.min, s.quantize(s.min + (s.max - s.min) / 2.0), s.max]
}

/// Every ORBIT parameter's case, with its ring soloed (the chords' and the
/// tempo's on the whole scene).
fn orbit_params() -> Vec<(BlockRef, &'static ParamSpec)> {
    let mut blocks: Vec<BlockRef> = RingIdx::ALL.map(BlockRef::OrbitRing).to_vec();
    blocks.extend([BlockRef::OrbitChords, BlockRef::Tempo]);
    blocks
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| (b, s)))
        .collect()
}

/// One parameter's sweep: each point held four loops (a lone hit at
/// CHANCE 70 % may miss one), then min ↔ max every 3 blocks for 600
/// blocks, then every 60 blocks for 1,200. Returns the report's row, its faults and its judged marks.
fn sweep_param(b: BlockRef, s: &'static ParamSpec) -> (String, Vec<String>, [usize; 5]) {
    let mut rig = OrbitRig::new();
    if let BlockRef::OrbitRing(r) = b {
        for (i, sr) in rig.song().live.rings.iter_mut().enumerate() {
            sr.ring.mute = i != r.get() as usize;
        }
    }
    let set = |rig: &mut OrbitRig, v: f32| orbit_block(rig.song(), b).set(s.id, v);
    let name = format!("{b:?}.{}", s.label);
    let mut line = String::new();
    let mut faults = Vec::new();
    rig.play(true);
    for v in points(s) {
        set(&mut rig, v);
        let from = rig.blocks();
        rig.run_loops(4);
        let heard = rig.tape.rms(0, from + 1, rig.blocks());
        let _ = write!(line, " {v}: {:.1} dBFS rms;", common::sweep::db(heard));
        if heard < SILENT_RMS {
            match silences(b, s.id, v) {
                Some(why) => {
                    let _ = write!(line, " silent ({why});");
                }
                None => faults.push(format!("{name} = {v}: silent ({heard:.2e})")),
            }
        }
    }
    // Fast jumps, then slow ones: a jump every 3 blocks leaves no steady
    // window between them, so a jump's break is judged only in the slow.
    for i in 0..200 {
        set(&mut rig, if i % 2 == 0 { s.min } else { s.max });
        rig.run(3);
    }
    for i in 0..20 {
        set(&mut rig, if i % 2 == 0 { s.max } else { s.min });
        rig.run(60);
    }
    set(&mut rig, s.min);
    let bound = stop_bound();
    let freed = stop_and_drain(&mut rig, bound);
    if !freed.is_some_and(|f| f <= bound) {
        faults.push(format!("{name}: STOP freed in {freed:?}"));
    }
    let r = report(&rig, 0);
    faults.extend(r.faults().into_iter().map(|f| format!("{name}: {f}")));
    let row = format!(
        "| {name} |{line} peak {:.3}, judged/seen {}, refused {} | {} |",
        r.peak[0],
        r.marks(),
        r.refused,
        if faults.is_empty() { "ok" } else { "FAIL" }
    );
    (row, faults, r.judged)
}

#[test]
fn orbit_param_sweep() {
    let cases = orbit_params();
    let n = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunks: Vec<Vec<(BlockRef, &'static ParamSpec)>> = cases
        .chunks(cases.len().div_ceil(n))
        .map(<[_]>::to_vec)
        .collect();
    let mut rows: Vec<(String, Vec<String>, [usize; 5])> = std::thread::scope(|sc| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|c| {
                sc.spawn(move || {
                    c.iter()
                        .map(|&(b, s)| sweep_param(b, s))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a sweep thread"))
            .collect()
    });
    let mut report = String::from(
        "\n## ORBIT parameters, through a running ORBIT\n\n\
         Known legitimate: MUTE silences its ring; CHANCE 0 never fires; HITS 0 lays no hit.\n\n\
         | parameter | points | verdict |\n|---|---|---|\n",
    );
    let mut faults = Vec::new();
    let mut judged = [0; 5];
    for (row, f, j) in rows.drain(..) {
        report.push_str(&row);
        report.push('\n');
        faults.extend(f);
        for (a, b) in judged.iter_mut().zip(j) {
            *a += b;
        }
    }
    println!("{report}");
    if let Ok(path) = std::env::var("PARAM_SWEEP_REPORT") {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("the report");
        f.write_all(report.as_bytes()).expect("the report");
    }
    assert_eq!(cases.len(), 5 * 15 + 4 + 1);
    println!("judged across the sweep, by {:?}: {judged:?}", Mark::KINDS);
    // The full scene never judges MUTATE (the downbeat plays) and has no
    // BPM jump: the sweep is where they are judged, so it must.
    let (mutate, bpm) = (judged[Mark::Mutate(0).kind()], judged[Mark::Bpm.kind()]);
    assert!(mutate >= MUTATE_JUDGED, "MUTATE judged {mutate}");
    assert!(bpm >= BPM_JUDGED, "BPM jumps judged {bpm}");
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

// ── The UAT renders ────────────────────────────────────────────────────

/// 20 s of the init scene: the pairs summed, as the desktop plays them.
fn render_20s(mut rig: OrbitRig) -> Vec<f32> {
    rig.play(true);
    rig.run(secs(20.0));
    rig.sum
}

#[test]
fn ab_renders() {
    let Some(dir) = std::env::var_os("ORBIT_QA_WAV") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the WAV dir");
    write_wav(&dir.join("a-new.wav"), &render_20s(OrbitRig::new()));
    // TX BASS, TX EPIANO, MORPH PAD, TX BELL, SAW LEAD on Parts 1–5.
    let sounds: Vec<(usize, chimera_core::preset::Sound)> = [0, 1, 6, 3, 4]
        .into_iter()
        .enumerate()
        .map(|(p, i)| (p, chimera_core::factory::factory_sound(i).expect("factory")))
        .collect();
    write_wav(
        &dir.join("b-factory.wav"),
        &render_20s(OrbitRig::with_sounds(&sounds)),
    );
    println!("wrote {}/{{a-new,b-factory}}.wav", dir.display());
}
