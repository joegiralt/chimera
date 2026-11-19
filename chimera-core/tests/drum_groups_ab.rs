//! The voice-groups A/B (ADR 0280, #993), the drum-steal spike's patterns
//! rendered on this tree, for the ear and the refusal counts. One render a
//! process, so the hit seeds start alike: `AB_PATTERN` 1–6, `AB_MODE`
//! `today` (patterns 4–6 with the open HAT's, CYMBAL's and toms' chokes
//! OFF: 11 groups, a kit from before the bound) or `groups` (INIT's
//! chokes: 8) or `reserve` (`groups` with the DRUM Part on RESERVE
//! `AB_RESERVE`, ADR 0281, the mode's name then `reserveN`), `AB_OUT`
//! the directory:
//! `cargo test --release -p chimera-core --test drum_groups_ab -- --ignored`.

mod common;

use chimera_core::MidiChannel;
use chimera_core::budget::CONTROL_RESERVE;
use chimera_core::dsp::drum::groups::VoiceGroups;
use chimera_core::dsp::drum::types::{ChokeGroup, Kit, PadId};
use chimera_core::dsp::drum::view::KitWriter;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_hal::BLOCK_SIZE;

const SR: f64 = common::SR as f64;

/// (block, channel, note, velocity; 0 = off).
type Ev = (usize, u8, u8, u8);

fn at(sec: f64) -> usize {
    (sec * SR / BLOCK_SIZE as f64).round() as usize
}

fn pattern1() -> Vec<Ev> {
    let mut e: Vec<Ev> = (0..8).map(|b| (b, 0, 0, 127)).collect();
    e.push((8, 0, 0, 0));
    e.push((9, 0, 7, 127));
    e.push((10, 0, 7, 0));
    e
}

fn pattern2() -> Vec<Ev> {
    let step = 60.0 / 140.0 / 4.0;
    let mut e = vec![];
    let hit = |t: f64, note: u8, vel: u8, e: &mut Vec<Ev>| {
        e.push((at(t), 0, note, vel));
        e.push((at(t + step / 2.0), 0, note, 0));
    };
    for bar in 0..4 {
        for s in 0..16 {
            let t = (bar * 16 + s) as f64 * step;
            if s == 0 || s == 8 {
                hit(t, 0, 127, &mut e);
            }
            if s == 4 || s == 12 {
                hit(t, 1, 110, &mut e);
            }
            if s == 15 {
                hit(t, 8, 90, &mut e);
            } else {
                hit(t, 7, 80, &mut e);
            }
        }
    }
    // The fill: pads 1–12 on 32nds from the last bar's step 10.
    for p in 0..12u8 {
        let t = (3 * 16 + 10) as f64 * step + f64::from(p) * step / 2.0;
        hit(t, p, 100, &mut e);
    }
    e
}

fn pattern3() -> Vec<Ev> {
    let mut e = pattern2();
    let end = at(4.0 * 16.0 * 60.0 / 140.0 / 4.0);
    for (ch, chord) in [(1, [48, 55, 60, 63, 67, 70]), (2, [53, 57, 60, 64, 67, 72])] {
        for n in chord {
            e.push((0, ch, n, 90));
            e.push((end, ch, n, 0));
        }
    }
    e
}

/// One hit, gate half a step.
fn hit(e: &mut Vec<Ev>, t: f64, gate: f64, note: u8, vel: u8) {
    e.push((at(t), 0, note, vel));
    e.push((at(t + gate), 0, note, 0));
}

/// Big room at 128: STACK kick four on the floor, CLAP on 2 and 4, open
/// HAT on the offbeats, SHAKER 16ths, CYMBAL on each bar's one, a snare
/// roll into the last bar. 8 bars.
fn pattern4() -> Vec<Ev> {
    let step = 60.0 / 128.0 / 4.0;
    let mut e = vec![];
    for bar in 0..8 {
        for s in 0..16 {
            let t = (bar * 16 + s) as f64 * step;
            if s % 4 == 0 {
                hit(&mut e, t, step / 2.0, 0, 127);
            }
            if s == 4 || s == 12 {
                hit(&mut e, t, step / 2.0, 3, 110);
            }
            if s % 4 == 2 {
                hit(&mut e, t, step / 2.0, 8, 95);
            }
            hit(&mut e, t, step / 2.0, 11, if s % 2 == 0 { 70 } else { 50 });
            if s == 0 {
                hit(&mut e, t, step / 2.0, 9, 115);
            }
        }
    }
    // A 32nd snare roll over bar 7's second half, rising.
    for k in 0..16u8 {
        let t = (6 * 16 + 8) as f64 * step + f64::from(k) * step / 2.0;
        hit(&mut e, t, step / 4.0, 1, 60 + k * 4);
    }
    e
}

/// Breakbeat at 172: kick and snare broken, ghost snares, closed HAT
/// 8ths, STACK toms running 32nds down the kit every second bar's end,
/// CYMBAL crashes after each run. 8 bars.
fn pattern5() -> Vec<Ev> {
    let step = 60.0 / 172.0 / 4.0;
    let mut e = vec![];
    let kick = [0, 6, 10];
    let snare = [4, 12];
    let ghost = [7, 9, 15];
    for bar in 0..8 {
        let fill = bar % 2 == 1;
        for s in 0..16 {
            let t = (bar * 16 + s) as f64 * step;
            if fill && s >= 12 {
                continue;
            }
            if kick.contains(&s) {
                hit(&mut e, t, step / 2.0, 0, 120);
            }
            if snare.contains(&s) {
                hit(&mut e, t, step / 2.0, 1, 115);
            }
            if ghost.contains(&s) {
                hit(&mut e, t, step / 4.0, 1, 45);
            }
            if s % 2 == 0 {
                hit(&mut e, t, step / 2.0, 7, 85);
            }
            if s == 0 && bar > 0 && bar % 2 == 0 {
                hit(&mut e, t, step / 2.0, 9, 120);
            }
        }
        if fill {
            // Toms high to low, two 32nds each, over the bar's last beat.
            for k in 0..8u8 {
                let t = (bar * 16 + 12) as f64 * step + f64::from(k) * step / 2.0;
                hit(&mut e, t, step / 4.0, 6 - k / 3, 100 + k * 3);
            }
        }
    }
    e
}

/// `pattern5` under two held 6-note pad chords on Parts 2 (VA) and 3
/// (ALGO), changing every 2 bars.
fn pattern6() -> Vec<Ev> {
    let mut e = pattern5();
    let bar = 16.0 * 60.0 / 172.0 / 4.0;
    let chords: [[[u8; 6]; 4]; 2] = [
        [
            [45, 52, 57, 60, 64, 67],
            [41, 48, 53, 57, 60, 64],
            [43, 50, 55, 59, 62, 67],
            [40, 47, 52, 55, 59, 64],
        ],
        [
            [57, 60, 64, 67, 71, 72],
            [53, 57, 60, 64, 67, 69],
            [55, 59, 62, 67, 71, 74],
            [52, 55, 59, 64, 67, 71],
        ],
    ];
    for (ch, prog) in chords.iter().enumerate() {
        for (i, c) in prog.iter().enumerate() {
            let (on, off) = (at(i as f64 * 2.0 * bar), at((i + 1) as f64 * 2.0 * bar) - 1);
            for &n in c {
                e.push((on, ch as u8 + 1, n, 85));
                e.push((off, ch as u8 + 1, n, 0));
            }
        }
    }
    e
}

fn write_wav(path: &str, lr: &[f32]) {
    let data = (lr.len() * 3) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&common::SR.to_le_bytes());
    b.extend_from_slice(&(common::SR * 6).to_le_bytes());
    b.extend_from_slice(&6u16.to_le_bytes());
    b.extend_from_slice(&24u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for &x in lr {
        let v = (f64::from(chimera_core::dsp::clampf(x, -1.0, 1.0)) * 8_388_607.0).round() as i32;
        b.extend_from_slice(&v.to_le_bytes()[..3]);
    }
    std::fs::write(path, b).unwrap();
}

#[test]
#[ignore = "A/B render"]
fn render_ab() {
    let pat: u8 = std::env::var("AB_PATTERN").unwrap().parse().unwrap();
    let mode = std::env::var("AB_MODE").unwrap();
    let out = std::env::var("AB_OUT").unwrap();
    let mut kit = Kit::DEFAULT;
    let stack: &[u8] = match pat {
        1 => &[0],
        4 => &[0, 9],
        5 | 6 => &[4, 5, 6, 9],
        _ => &[],
    };
    for &p in stack {
        kit.pad_mut(PadId::new(p).unwrap())
            .set_retrig(Retrig::Stack);
    }
    assert!(
        mode == "today" || mode == "groups" || mode == "reserve",
        "AB_MODE today, groups or reserve"
    );
    let reserve: u8 = std::env::var("AB_RESERVE").map_or(4, |r| r.parse().unwrap());
    let mode = if mode == "reserve" {
        format!("reserve{reserve}")
    } else {
        mode
    };
    if pat >= 4 && mode == "today" {
        // Let the open HAT, the CYMBAL and the toms ring: no chokes.
        for p in [5, 6, 8, 9] {
            kit.pad_mut(PadId::new(p).unwrap())
                .set_choke(ChokeGroup::Off);
        }
    }
    let mut w = KitWriter::take().expect("the kit writer, once");
    for id in PartId::ALL {
        w.publish(id, &kit);
        let _ = w.take_hit(id);
    }
    let mut r = common::InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    for (i, part) in r.shared.parts.iter_mut().enumerate() {
        part.mix.channel = MidiChannel::new(if i < 3 { i as u8 } else { 15 }).unwrap();
    }
    if mode.starts_with("reserve") {
        r.shared.parts[0]
            .mix
            .set_voices(chimera_core::voice_limit::Asked {
                max: chimera_core::voice_limit::MaxVoices::Auto,
                reserve: chimera_core::voice_limit::Reserve::new(
                    chimera_core::voice_limit::VoiceCount::new(reserve).unwrap(),
                ),
            });
    }
    *r.shared.parts[0].params = Sound::init(EngineType::Drum).params;
    *r.shared.parts[1].params = Sound::init(EngineType::Va).params;
    *r.shared.parts[2].params = Sound::init(EngineType::Algo).params;
    let mut ev = match pat {
        1 => pattern1(),
        2 => pattern2(),
        3 => pattern3(),
        4 => pattern4(),
        5 => pattern5(),
        _ => pattern6(),
    };
    ev.sort_by_key(|&(b, _, _, v)| (b, v != 0));
    let blocks = ev.last().unwrap().0 + at(2.0);
    let (mut struck, mut refused, mut peak, mut live_peak) = (0, 0, 0u8, 0u32);
    // The FX bus and control reserve; the reverb's extra is left out.
    let fixed = FxBus::COST.0 + CONTROL_RESERVE.0;
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    let mut lr = Vec::with_capacity(blocks * BLOCK_SIZE * 2);
    let mut i = 0;
    for b in 0..blocks {
        while i < ev.len() && ev[i].0 == b {
            let (_, ch, n, v) = ev[i];
            if v == 0 {
                r.note_off(ch, n);
            } else {
                let was = r.inst.allocator().refused();
                r.note_on_vel(ch, n, v);
                if ch == 0 {
                    struck += 1;
                    refused += r.inst.allocator().refused() - was;
                }
            }
            i += 1;
        }
        let mut look = |r: &common::InstRig| {
            peak = peak.max(r.inst.cost_pct());
            let live: u32 = r
                .inst
                .allocator()
                .slots()
                .iter()
                .filter(|s| !s.dying())
                .map(|s| s.cost().0)
                .sum();
            live_peak = live_peak.max((live + fixed) * 100 / budget);
        };
        look(&r);
        r.render(1);
        look(&r);
        lr.extend_from_slice(&r.out()[0]);
    }
    let a = r.inst.allocator();
    let line = format!(
        "| p{pat} | {mode} | {} | {struck} | {refused} | {} | {} | {peak}% | {live_peak}% |",
        VoiceGroups::of(&kit).get(),
        a.refused() - refused,
        a.refused(),
    );
    println!("{line}");
    write_wav(&format!("{out}/p{pat}-{mode}.wav"), &lr);
    std::fs::write(format!("{out}/p{pat}-{mode}.txt"), line + "\n").unwrap();
}
