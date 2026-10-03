//! Slice 1's QA gate for ORBIT (Task 11): the init scene played from a NEW
//! project, on NEW's factory Sounds, on the desktop's real mix path,
//! measured per DAC pair and summed. Clicks are judged where sound can
//! break (a ring's off, a steal, STOP, MUTATE's boundary, a BPM jump),
//! never at a note-on; stuck notes are checked every block while running,
//! and after STOP.
//!
//! With `ORBIT_QA_WAV=dir` set, `uat_render` writes `new.wav`, NEW as it
//! plays, for UAT.

mod common;

use std::fmt::Write as _;
use std::sync::OnceLock;

use chimera_core::addr::BlockRef;
use chimera_core::block::{ParamId, ParamSpec};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::hw::DAC_PAIRS;
use chimera_core::orbit::RingIdx;
use chimera_core::orbit::chords::Bars;
use chimera_core::orbit::clock::Bpm;
use chimera_core::orbit::euclid::{Fill, Steps};
use chimera_core::orbit::ring::NoteMode;
use chimera_core::orbit::rng::Pct;
use chimera_core::orbit::scene::{OrbitSong, Scene, SceneRing};
use chimera_core::part::DacPair;
use chimera_core::part::PartParams;
use chimera_core::project::{NEW_PARTS, PartId};
use chimera_hal::BLOCK_SIZE;
use chimera_theory::Note;
use common::orbit::{Mark, OrbitRig, Report, judgeable, orbit_block, report, secs, write_wav};
use common::sweep::{
    BPS, Bench, CLICK_RATIO, Patch, SILENT_RMS, Tape, level, steady_windows, transient_window,
};

/// A bar at 120 BPM, in blocks; the init scene's loop is four.
const BAR: usize = 1500;

/// The level reference: a C4 triad held 2 s on TX EPIANO, NEW's Part 2,
/// the scene's loudest voice. The scene sits on factory Sounds since the
/// owner's pick at UAT, so INIT Algo's chord, 6.7 dB over it, no longer
/// says how loud a factory Sound plays.
const C4_CHORD: [u8; 3] = [60, 64, 67];
const REFERENCE: Patch = Patch::Factory(NEW_PARTS[1].index() as u8);

fn assert_clean(name: &str, r: &Report) {
    r.print(name);
    let f = r.faults();
    assert!(f.is_empty(), "{name}:\n{}", f.join("\n"));
}

/// How long after STOP every voice must be free: the longest release of
/// NEW's Sounds (the five factory ones and Part 6's INIT), measured once,
/// plus 1 s.
fn stop_bound() -> usize {
    static BOUND: OnceLock<usize> = OnceLock::new();
    *BOUND.get_or_init(|| {
        let factory = NEW_PARTS.map(|s| Patch::Factory(s.index() as u8));
        let longest = factory
            .into_iter()
            .chain([Patch::AlgoInit])
            .map(longest_release)
            .max();
        longest.expect("NEW has Sounds") + secs(1.0)
    })
}

/// Blocks from a released chord at velocity 127 until its voices are free.
fn longest_release(patch: Patch) -> usize {
    let mut b = Bench::new(patch, PartParams::default(), FxParams::default());
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
    tape.freed(off).expect("the release ends")
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
    rig.run_loops(2);
    // A live chord on ring 1's Part, held a loop and released.
    for n in [57, 60, 64] {
        rig.key(0, n, true);
    }
    rig.run_loops(1);
    for n in [57, 60, 64] {
        rig.key(0, n, false);
    }
    rig.run_loops(13);
    assert_eq!(rig.view().loops, 17, "16 whole loops, 64 bars");
    // STOP between steps, so its break is judged apart from an attack.
    rig.run(40);
    let freed = stop_and_drain(&mut rig, bound);
    let r = report(&rig, 0);
    assert_clean("init scene, 64 bars", &r);
    assert!(
        freed.is_some_and(|f| f <= bound),
        "STOP freed every voice in {freed:?} blocks, bound {bound}"
    );
    // Ring offs are judged in numbers, and STOP. A steal is taken at the
    // note-on that needs the voice, and MUTATE's boundary is the downbeat,
    // where rings 1 and 2 always play: both share their block with an
    // attack, so `a_steal_fades_clean_on_its_pair` and the sweep judge them.
    assert!(r.judged[0] > 240 && r.judged[2] == 1, "{:?}", r.judged);
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
    // From the second bar: past the first attacks' DC settling.
    report(&rig, BAR)
}

#[test]
fn init_scene_levels() {
    let reference = level(REFERENCE, &C4_CHORD, 100, 2 * BPS).lufs;
    let all = render_levels(None, 4);
    all.print("init scene");
    println!("reference: TX EPIANO C4 chord held, {reference:.1} LUFS");
    let solos: Vec<f32> = (0..5)
        .map(|i| {
            let r = render_levels(Some(i), 4);
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
        let at = BPS + lcg(&mut seed) as usize % (8 * BAR);
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
    rig.run_loops(4);
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
/// rings 3 and 4's (Part 3) on P3, and Part 5's, muted ring 5's, there
/// too. Each pair is bounded on its own, and none sits more than 6 dB from
/// the others.
#[test]
fn init_scene_across_pairs() {
    let mut rig = OrbitRig::new();
    for (p, pair) in [(1, DacPair::P2), (2, DacPair::P3), (4, DacPair::P3)] {
        rig.project.edit_part(PartId::ALL[p]).mix.output = pair;
    }
    rig.play(true);
    rig.run_loops(4);
    let r = report(&rig, BAR);
    assert_eq!(r.carrying(), [true; DAC_PAIRS], "every pair plays");
    assert_clean("init scene across P1–P3", &r);
}

/// Part 1 (on P1) holds eight notes, the whole pool, for a second; with
/// `released`, its 48 is let go 20 blocks before the end. With `steal`,
/// Part 2 (routed to P2) then plays one note, which takes a voice of Part
/// 1's. Both Parts on INIT Algo, whose sustain and release the thresholds
/// below were set by. Returns the rig a second on, and the steal's block.
fn steal_scene(released: bool, steal: bool) -> (OrbitRig, usize) {
    let mut rig = OrbitRig::on_init();
    rig.project.edit_part(PartId::ALL[1]).mix.output = DacPair::P2;
    for n in [48, 52, 55, 59, 60, 64, 67, 71] {
        rig.key(0, n, true);
    }
    rig.run(secs(1.0));
    if released {
        rig.key(0, 48, false);
        rig.run(20);
    }
    let e = rig.blocks();
    if steal {
        rig.key(1, 72, true);
    }
    rig.run(secs(1.0));
    (rig, e)
}

/// A steal's fade, apart from the attack that causes it and from the rest
/// of the chord. The stealer plays on P2, so P1 holds only the victim's
/// fade; P1 less the same render without the steal is the victim alone,
/// fading out where the control's goes on. That difference is judged at
/// the steal against the victim's own steady state: a hard cut there (the
/// fade mutated to gain 0) fails both cases, at 6× and 8×. Once with the
/// victim held, once with it 20 blocks into its release (a tail, which the
/// allocator takes first).
#[test]
fn a_steal_fades_clean_on_its_pair() {
    for released in [false, true] {
        let case = if released { "a tail" } else { "a held note" };
        let (rig, e) = steal_scene(released, true);
        let (control, _) = steal_scene(released, false);
        assert!(
            rig.marks.contains(&(e, Mark::Steal)),
            "{case}: Part 2's note stole a voice"
        );
        assert!(rig.tape.peak(1) > 1e-3, "{case}: the new note on P2");
        let gone = gone(&control.tape, &rig.tape);
        let after = gone.rms(0, e + 20, e + 60);
        assert!(after > 1e-4, "{case}: the victim left P1 ({after:.2e})");
        let quiet_before = gone.pair(0, 0, e).iter().all(|&x| x == 0.0);
        assert!(quiet_before, "{case}: the renders agree before the steal");
        // The difference's only events: the chord's onsets at block 0.
        assert!(
            judgeable(&gone, e, (0, usize::MAX), &[0]),
            "{case}: judgeable"
        );
        // `Tape::click`'s ratio without its floor: the difference is the
        // victim alone, so its own steady state is the scale, however
        // quiet a tail is.
        let r = fade_ratio(&gone, e);
        println!("{case}: the fade's transient {r:.2}× the victim's steady");
        assert!(
            r <= CLICK_RATIO,
            "{case}: the stolen voice's fade clicks on P1, {r:.1}× its steady state"
        );
    }
}

/// A fade's transient at event block `e` over its own steady state, on a
/// tape holding only what the event took away (a control less the render).
fn fade_ratio(gone: &Tape, e: usize) -> f32 {
    let heard = transient_window(e);
    let trans = gone.d2(0, heard.start, heard.end);
    let (_, post) = steady_windows(e, (0, usize::MAX), true);
    trans / gone.d2(0, post.start, post.end)
}

/// P1 less `cut`: what a note's off took away, alone.
fn gone(control: &Tape, cut: &Tape) -> Tape {
    let mut g = Tape::default();
    g.out[0] = control.out[0]
        .iter()
        .zip(&cut.out[0])
        .map(|(c, s)| c - s)
        .collect();
    g.bus = vec![0.0; control.bus.len()];
    g
}

/// A note on `patch` at block `ON`, let go at `off` (`None`: held), then
/// with `hard` muted from the loudest sample of the off's first heard
/// block on.
fn gate0(patch: Patch, note: u8, off: Option<usize>, hard: bool) -> Tape {
    const ON: usize = 60;
    let mut b = Bench::new(patch, PartParams::default(), FxParams::default());
    let mut t = Tape::default();
    for i in 0..ON + 80 {
        if i == ON {
            b.note(note, Some(100));
        }
        if Some(i) == off {
            b.note(note, None);
        }
        b.render(&mut t);
    }
    if let (true, Some(e)) = (hard, off) {
        let from = 2 * BLOCK_SIZE * (e + 1);
        let peak = (0..BLOCK_SIZE).max_by(|&i, &j| {
            let x = |i: usize| t.out[0][from + 2 * i].abs();
            x(i).total_cmp(&x(j))
        });
        t.out[0][from + 2 * peak.expect("a block")..].fill(0.0);
    }
    t
}

/// A ring's off at GATE 0 lands two blocks into its note's attack. On a
/// decaying Sound `Tape::click` hears that as a click (TX EPIANO's E5:
/// 21.7×), but the note held reads the same there: the window holds the
/// attack, the steady window a quieter release. Without its tine (operator
/// 2 at LEVEL 0) still 6.7×: no band limit on the judge would clear it.
/// So NEW's factory Sounds are judged here as a steal is: the release
/// alone, the held control less the cut, against its own steady state
/// (TX EPIANO's E5: 0.96×). A hard cut there reads tens of times over.
#[test]
fn a_gate_0_off_releases_clean_on_factory_sounds() {
    const OFF: usize = 62;
    for slot in NEW_PARTS {
        let patch = Patch::Factory(slot.index() as u8);
        for note in [48, 60, 76, 84] {
            let control = gate0(patch, note, None, false);
            let cut = gate0(patch, note, Some(OFF), false);
            let r = fade_ratio(&gone(&control, &cut), OFF);
            println!("{} note {note}: the release {r:.2}×", patch.name());
            assert!(
                r <= CLICK_RATIO,
                "{} note {note}: the off at GATE 0 clicks, {r:.1}×",
                patch.name()
            );
        }
    }
    // The judge hears a cut: TX EPIANO's E5 muted at the off.
    let epiano = Patch::Factory(NEW_PARTS[1].index() as u8);
    let control = gate0(epiano, 76, None, false);
    let h = fade_ratio(&gone(&control, &gate0(epiano, 76, Some(OFF), true)), OFF);
    assert!(h > CLICK_RATIO, "a hard cut reads {h:.1}×");
}

// ── The parameter sweep, through a running ORBIT ───────────────────────

/// Floors on the sweep's judged MUTATE boundaries and BPM jumps, about
/// half of what it judges (416 and 19, 2026-10-03): a change that
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

/// What the sweep turns: a parameter, or NOTES' own cells on a ring, a
/// pool's or a STEP ring's.
#[derive(Clone, Copy, Debug)]
enum Case {
    Param(BlockRef, &'static ParamSpec),
    /// The first chip's note, C-1 to G9.
    Note(RingIdx, NoteMode),
    /// CLEAR, as MIX + turn: down rolls new notes (below 0), up leaves
    /// one C. The jumps alternate the two.
    Clear(RingIdx, NoteMode),
}

impl Case {
    fn ring(self) -> Option<RingIdx> {
        match self {
            Case::Param(BlockRef::OrbitRing(r), _) | Case::Note(r, _) | Case::Clear(r, _) => {
                Some(r)
            }
            Case::Param(..) => None,
        }
    }

    fn name(self) -> String {
        match self {
            Case::Param(b, s) => format!("{b:?}.{}", s.label),
            Case::Note(r, m) => format!("OrbitRing({}).NOTE {m:?}", r.get()),
            Case::Clear(r, m) => format!("OrbitRing({}).CLEAR {m:?}", r.get()),
        }
    }

    /// Min, an interior value, max.
    fn points(self) -> [f32; 3] {
        match self {
            Case::Param(_, s) => [s.min, s.quantize(s.min + (s.max - s.min) / 2.0), s.max],
            Case::Note(..) => [0.0, 64.0, 127.0],
            Case::Clear(..) => [-1.0, 1.0, 1.0],
        }
    }

    fn set(self, song: &mut OrbitSong, v: f32) {
        match self {
            Case::Param(b, s) => orbit_block(song, b).set(s.id, v),
            Case::Note(r, _) => {
                let note = Note::new(v as u8).expect("a note");
                song.live.rings[r.get() as usize].set_chip(0, note);
            }
            // The scene's own: the UI's MIX gate is `clear_needs_mix`'s.
            Case::Clear(r, _) if v > 0.0 => song.live.rings[r.get() as usize].clear(),
            Case::Clear(r, _) => song.live.scatter(r),
        }
    }

    /// What it may legitimately do, with the reason.
    fn silences(self, v: f32) -> Option<&'static str> {
        match self {
            Case::Param(b, s) => silences(b, s.id, v),
            Case::Note(..) | Case::Clear(..) => None,
        }
    }
}

/// Every ORBIT parameter's case, and NOTE and CLEAR on every ring in
/// either mode; a ring's soloed (the chords' and the tempo's on the whole
/// scene).
fn orbit_params() -> Vec<Case> {
    let mut blocks: Vec<BlockRef> = RingIdx::ALL.map(BlockRef::OrbitRing).to_vec();
    blocks.extend([BlockRef::OrbitChords, BlockRef::Tempo]);
    let mut cases: Vec<Case> = blocks
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| Case::Param(b, s)))
        .collect();
    for r in RingIdx::ALL {
        for m in NoteMode::ALL {
            cases.extend([Case::Note(r, m), Case::Clear(r, m)]);
        }
    }
    cases
}

/// One parameter's sweep: each point held four loops (a lone hit at
/// CHANCE 70 % may miss one), then min ↔ max every 3 blocks for 600
/// blocks, then every 60 blocks for 1,200. Returns the report's row, its
/// faults and its judged marks.
///
/// On the init scene made busier: a loop a bar, and MUTATE 30 % on every
/// ring. The init scene has four bars and no MUTATE, so the sweep would
/// cross a quarter of the loop boundaries and judge no mutation outside
/// MUTATE's own rows.
///
/// And on INIT Algo, as before NEW had factory Sounds: the sweep judges
/// ORBIT's timing, not a Sound. On a decaying Sound an off at GATE 0 reads
/// as a click where none is
/// (`a_gate_0_off_releases_clean_on_factory_sounds` judges those).
fn sweep_param(case: Case) -> (String, Vec<String>, [usize; 5]) {
    let mut rig = OrbitRig::on_init();
    let song = rig.song();
    song.live.chords.bars = Bars::ONE;
    for (i, sr) in song.live.rings.iter_mut().enumerate() {
        sr.ring.mutate = Pct::clamped(30);
        if let Some(r) = case.ring() {
            sr.ring.mute = i != r.get() as usize;
        }
    }
    if let Case::Note(r, m) | Case::Clear(r, m) = case {
        song.live.rings[r.get() as usize].ring.mode = m;
    }
    let set = |rig: &mut OrbitRig, v: f32| case.set(rig.song(), v);
    let name = case.name();
    let [min, _, max] = case.points();
    let mut line = String::new();
    let mut faults = Vec::new();
    rig.play(true);
    for v in case.points() {
        set(&mut rig, v);
        let from = rig.blocks();
        rig.run_loops(4);
        let heard = rig.tape.rms(0, from + 1, rig.blocks());
        let _ = write!(line, " {v}: {:.1} dBFS rms;", common::sweep::db(heard));
        if heard < SILENT_RMS {
            match case.silences(v) {
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
        set(&mut rig, if i % 2 == 0 { min } else { max });
        rig.run(3);
    }
    for i in 0..20 {
        set(&mut rig, if i % 2 == 0 { max } else { min });
        rig.run(60);
    }
    set(&mut rig, min);
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
    let chunks: Vec<Vec<Case>> = cases
        .chunks(cases.len().div_ceil(n))
        .map(<[_]>::to_vec)
        .collect();
    let mut rows: Vec<(String, Vec<String>, [usize; 5])> = std::thread::scope(|sc| {
        let handles: Vec<_> = chunks
            .iter()
            .map(|c| sc.spawn(move || c.iter().map(|&c| sweep_param(c)).collect::<Vec<_>>()))
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
    assert_eq!(cases.len(), 5 * 15 + 4 + 1 + 5 * 4);
    println!("judged across the sweep, by {:?}: {judged:?}", Mark::KINDS);
    // The full scene never judges MUTATE (the downbeat plays) and has no
    // BPM jump: the sweep is where they are judged, so it must.
    let (mutate, bpm) = (judged[Mark::Mutate(0).kind()], judged[Mark::Bpm.kind()]);
    assert!(mutate >= MUTATE_JUDGED, "MUTATE judged {mutate}");
    assert!(bpm >= BPM_JUDGED, "BPM jumps judged {bpm}");
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

// ── The UAT render ─────────────────────────────────────────────────────

/// 20 s of NEW playing: the pairs summed, as the desktop plays them.
#[test]
fn uat_render() {
    let Some(dir) = std::env::var_os("ORBIT_QA_WAV") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the WAV dir");
    let mut rig = OrbitRig::new();
    rig.play(true);
    rig.run(secs(20.0));
    write_wav(&dir.join("new.wav"), &rig.sum);
    println!("wrote {}/new.wav", dir.display());
}
