// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM's machine laws (ADR 0265): each machine's knob labels, curves and
//! defaults, and the typed kernel params they resolve to.

use chimera_core::block::DiskCode;
use chimera_core::dsp::drum::laws::{
    BurstParams, ClapCount, Hardness, KernelParams, Law, MetalParams, NoiseTone, PlainParams,
    ToneParams, resolve, spec,
};
use chimera_core::dsp::drum::types::{Knob, KnobSet, KnobValue, Machine, PerSecond};
use chimera_midi::value::U7;

fn kv(v: u8) -> KnobValue {
    KnobValue::new(U7::new(v).unwrap())
}

const LABELS: [(Machine, [&str; 8]); 12] = [
    (
        Machine::ToneKick,
        [
            "PITCH",
            "DECAY",
            "SWEEP",
            "SWEEP TIME",
            "CLICK",
            "DRIVE",
            "START",
            "HARMONICS",
        ],
    ),
    (
        Machine::ToneSnare,
        [
            "PITCH",
            "DECAY",
            "BUMP",
            "SNAP",
            "SNAP DECAY",
            "TONE",
            "INTERVAL",
            "DRIVE",
        ],
    ),
    (
        Machine::ToneTom,
        [
            "PITCH",
            "DECAY",
            "SWEEP",
            "SWEEP TIME",
            "NOISE",
            "DAMP",
            "DRIVE",
            "DRIVE TYPE",
        ],
    ),
    (
        Machine::ToneClap,
        [
            "TONE",
            "DECAY",
            "CLAPS",
            "SPREAD",
            "TAIL",
            "RING",
            "HARDNESS",
            "ROOM TONE",
        ],
    ),
    (
        Machine::ToneCowbell,
        [
            "PITCH", "DECAY", "INTERVAL", "TONE", "RING", "DRIVE", "DAMP", "BUMP",
        ],
    ),
    (
        Machine::ToneCymbal,
        [
            "PITCH", "DECAY", "TONE", "SPREAD", "RING", "SHIMMER", "TOP", "TOP TUNE",
        ],
    ),
    (
        Machine::MetalHat,
        [
            "PITCH",
            "DECAY",
            "TONE",
            "SPREAD",
            "RING",
            "NOISE",
            "HIGH PASS",
            "LOW PASS",
        ],
    ),
    (
        Machine::MetalJingle,
        [
            "PITCH", "DECAY", "JINGLES", "SHAKE", "TONE", "RING", "HEAD", "SPREAD",
        ],
    ),
    (
        Machine::BodyShaker,
        [
            "GRAINS",
            "DECAY",
            "GRAIN LENGTH",
            "TONE",
            "SHAKE",
            "RING",
            "HARDNESS",
            "-",
        ],
    ),
    (
        Machine::PlainSine,
        ["PITCH", "DECAY", "SWEEP", "SWEEP TIME", "-", "-", "-", "-"],
    ),
    (
        Machine::PlainNoise,
        ["DECAY", "TONE", "-", "-", "-", "-", "-", "-"],
    ),
    (
        Machine::PlainImpulse,
        [
            "PITCH",
            "DECAY",
            "SWEEP",
            "SWEEP TIME",
            "-",
            "-",
            "WIDTH",
            "-",
        ],
    ),
];

#[test]
fn labels_match_the_table() {
    assert_eq!(spec(Machine::ToneKick).knobs[0].unwrap().label, "PITCH");
    assert_eq!(
        spec(Machine::ToneKick).knobs[3].unwrap().label,
        "SWEEP TIME"
    );
    assert!(
        spec(Machine::PlainNoise).knobs[2..]
            .iter()
            .all(Option::is_none)
    );
    for (m, labels) in LABELS {
        assert_eq!(spec(m).name.replace(' ', "_"), m.disk_ident());
        for (k, want) in spec(m).knobs.iter().zip(labels) {
            match k {
                Some(k) => {
                    assert_eq!(k.label, want, "{m:?}");
                    assert!(
                        !k.short.is_empty() && k.short.len() <= 5,
                        "{m:?} {}",
                        k.short
                    );
                }
                None => assert_eq!(want, "-", "{m:?}"),
            }
        }
    }
}

#[test]
fn laws_hit_their_ends() {
    let e = Law::Expo {
        lo: 20.0,
        hi: 200.0,
    };
    assert_eq!(e.at(KnobValue::new(U7::MAX)), 200.0);
    assert_eq!(e.at(KnobValue::new(U7::ZERO)), 20.0);
    let l = Law::Lin { lo: 1.2, hi: 1.8 };
    assert_eq!(l.at(KnobValue::new(U7::ZERO)), 1.2);
    assert_eq!(l.at(KnobValue::new(U7::MAX)), 1.8);
    let s = Law::Steps(&[1.0, 2.0, 3.0]);
    assert_eq!(s.at(kv(0)), 1.0);
    assert_eq!(s.at(kv(31)), 1.0);
    assert_eq!(s.at(kv(64)), 2.0);
    assert_eq!(s.at(kv(127)), 3.0);
    assert_eq!(Law::Steps(&[]).at(kv(64)), 0.0);
}

#[test]
fn defaults_resolve_to_their_kernel() {
    for m in Machine::ALL {
        assert_eq!(
            resolve(m, &KnobSet::defaults(m)).kernel(),
            m.kernel(),
            "{m:?}"
        );
    }
}

#[test]
fn defaults_are_the_prototypes_classic_settings() {
    let d = KnobSet::defaults(Machine::ToneKick);
    let want = [40, 80, 40, 40, 20, 10, 0, 0];
    for (k, w) in [
        Knob::A,
        Knob::B,
        Knob::C,
        Knob::D,
        Knob::E,
        Knob::F,
        Knob::G,
        Knob::H,
    ]
    .into_iter()
    .zip(want)
    {
        assert_eq!(d.get(k), kv(w));
    }
    assert_eq!(
        KnobSet::defaults(Machine::PlainImpulse).get(Knob::G),
        kv(117)
    );
    assert_eq!(KnobSet::defaults(Machine::MetalHat).get(Knob::H), kv(127));
}

/// The prototype's `expo` and `lin` (chimera-utils drums/parts.rs).
fn expo(k: f32, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(k)
}

fn lin(k: f32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * k
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-5 * b.abs().max(1.0)
}

#[test]
fn kick_follows_its_prototype() {
    let mut ks = KnobSet::defaults(Machine::ToneKick);
    ks.set(Knob::A, kv(100));
    let KernelParams::Tone(ToneParams::Kick(p)) = resolve(Machine::ToneKick, &ks) else {
        panic!("kick resolves to the tone kernel");
    };
    let u = |v: u8| f32::from(v) / 127.0;
    assert!(close(p.body.pitch.get(), expo(u(100), 30.0, 120.0)));
    assert_eq!(p.body.depth.get(), 48.0 * u(40));
    assert!(close(p.body.time.get(), expo(u(40), 0.002, 0.12)));
    assert!(close(p.decay.get(), expo(u(80), 0.08, 3.0)));
    assert_eq!(p.click.get(), 1.2 * u(20));
}

#[test]
fn snare_interval_and_tom_noise() {
    let KernelParams::Tone(ToneParams::Snare(s)) =
        resolve(Machine::ToneSnare, &KnobSet::defaults(Machine::ToneSnare))
    else {
        panic!();
    };
    assert!(close(s.interval.get(), lin(60.0 / 127.0, 1.2, 1.7716)));
    let mut ks = KnobSet::defaults(Machine::ToneTom);
    ks.set(Knob::A, kv(127));
    let KernelParams::Tone(ToneParams::Tom(t)) = resolve(Machine::ToneTom, &ks) else {
        panic!();
    };
    assert_eq!(t.noise_hz.get(), 1600.0);
    assert_eq!(t.body.pitch.get(), 400.0);
}

#[test]
fn hat_low_pass_is_off_at_the_top() {
    let mut ks = KnobSet::defaults(Machine::MetalHat);
    let MetalParams::Bank(b) = metal(Machine::MetalHat, &ks) else {
        panic!();
    };
    assert_eq!(b.lp, None);
    assert!(b.bands[2].is_none());
    // PITCH, SPREAD and LOW PASS at 0: the 808's six two octaves down,
    // undetuned, the low-pass at 2 kHz.
    ks.set(Knob::H, kv(0));
    ks.set(Knob::A, kv(0));
    ks.set(Knob::D, kv(0));
    let MetalParams::Bank(b) = metal(Machine::MetalHat, &ks) else {
        panic!();
    };
    assert!(close(b.lp.unwrap().get(), 2000.0));
    assert!(close(b.hz[0].get(), 205.3 / 4.0));
}

fn metal(m: Machine, ks: &KnobSet) -> MetalParams {
    match resolve(m, ks) {
        KernelParams::Metal(p) => p,
        _ => panic!("{m:?} is metal"),
    }
}

#[test]
fn cymbal_has_a_top_band_and_jingle_its_particles() {
    let MetalParams::Bank(c) = metal(Machine::ToneCymbal, &KnobSet::defaults(Machine::ToneCymbal))
    else {
        panic!();
    };
    assert!(c.bands[2].is_some());
    let MetalParams::Jingle(j) = metal(
        Machine::MetalJingle,
        &KnobSet::defaults(Machine::MetalJingle),
    ) else {
        panic!();
    };
    assert!(close(j.jingles.get(), expo(70.0 / 127.0, 300.0, 6000.0)));
    assert!(close(j.shake.get(), lin(30.0 / 127.0, 0.0, 0.6)));
    assert!(close(j.head_pitch.get(), expo(64.0 / 127.0, 120.0, 300.0)));
}

/// The CLAPS readout (its `Law::Steps`) and the count the clap plays
/// (`ClapCount::from_knob`) agree at every knob value.
#[test]
fn claps_readout_is_the_count_played() {
    let law = spec(Machine::ToneClap).knob(Knob::C).unwrap().law;
    assert!(matches!(law, Law::Steps(_)), "CLAPS reads as steps");
    for v in 0..=127 {
        let shown = law.at(kv(v));
        let played = ClapCount::from_knob(kv(v)).get();
        assert_eq!(shown, f32::from(played), "knob {v}");
    }
}

#[test]
fn clap_counts_and_shaker_hardness() {
    let mut ks = KnobSet::defaults(Machine::ToneClap);
    for (v, n) in [
        (0, 1),
        (127, 8),
        (54, 1 + (54.0f32 / 127.0 * 7.0).round() as u8),
    ] {
        ks.set(Knob::C, kv(v));
        let KernelParams::Burst(BurstParams::Clap(c)) = resolve(Machine::ToneClap, &ks) else {
            panic!();
        };
        assert_eq!(c.claps.get(), n);
    }
    let mut ks = KnobSet::defaults(Machine::BodyShaker);
    for (v, dark) in [(0, true), (63, true), (64, false), (127, false)] {
        ks.set(Knob::G, kv(v));
        let KernelParams::Burst(BurstParams::Shaker(s)) = resolve(Machine::BodyShaker, &ks) else {
            panic!();
        };
        assert_eq!(matches!(s.hardness, Hardness::Dark(_)), dark, "{v}");
    }
}

#[test]
fn noise_tone_splits_at_half() {
    let mut ks = KnobSet::defaults(Machine::PlainNoise);
    ks.set(Knob::B, kv(0));
    let KernelParams::Plain(PlainParams::Noise(n)) = resolve(Machine::PlainNoise, &ks) else {
        panic!();
    };
    assert_eq!(
        n.tone,
        NoiseTone::LowPass(chimera_core::dsp::drum::types::Hz::new(200.0))
    );
    ks.set(Knob::B, kv(127));
    let KernelParams::Plain(PlainParams::Noise(n)) = resolve(Machine::PlainNoise, &ks) else {
        panic!();
    };
    let NoiseTone::HighPass(hz) = n.tone else {
        panic!();
    };
    assert!(close(hz.get(), 10_000.0));
}

#[test]
fn every_law_is_finite_at_its_ends() {
    for m in Machine::ALL {
        for (i, ks) in spec(m).knobs.iter().enumerate() {
            let Some(ks) = ks else { continue };
            for v in [0u8, 127] {
                let x = ks.law.at(kv(v));
                assert!(x.is_finite(), "{m:?} knob {i} at {v}: {x}");
            }
        }
    }
}

/// The resolved field a knob's law sets directly, `None` where the knob
/// is nested (listed in `NESTED`) or the field is off.
fn field(m: Machine, k: Knob, p: &KernelParams) -> Option<f32> {
    use BurstParams::{Clap, Shaker};
    use KernelParams::{Burst, Metal, Plain, Tone};
    use Knob::*;
    Some(match (p, k) {
        (Tone(ToneParams::Kick(p)), A) => p.body.pitch.get(),
        (Tone(ToneParams::Kick(p)), B) => p.decay.get(),
        (Tone(ToneParams::Kick(p)), C) => p.body.depth.get(),
        (Tone(ToneParams::Kick(p)), D) => p.body.time.get(),
        (Tone(ToneParams::Kick(p)), E) => p.click.get(),
        (Tone(ToneParams::Kick(p)), F) => p.drive.get(),
        (Tone(ToneParams::Kick(p)), G) => p.start.get(),
        (Tone(ToneParams::Kick(p)), H) => p.harmonics.get(),
        (Tone(ToneParams::Snare(p)), A) => p.pitch.get(),
        (Tone(ToneParams::Snare(p)), B) => p.decay.get(),
        (Tone(ToneParams::Snare(p)), C) => p.bump.get(),
        (Tone(ToneParams::Snare(p)), D) => p.snap.get(),
        (Tone(ToneParams::Snare(p)), E) => p.snap_decay.get(),
        (Tone(ToneParams::Snare(p)), F) => p.tone.get(),
        (Tone(ToneParams::Snare(p)), G) => p.interval.get(),
        (Tone(ToneParams::Snare(p)), H) => p.drive.get(),
        (Tone(ToneParams::Tom(p)), A) => p.body.pitch.get(),
        (Tone(ToneParams::Tom(p)), B) => p.decay.get(),
        (Tone(ToneParams::Tom(p)), C) => p.body.depth.get(),
        (Tone(ToneParams::Tom(p)), D) => p.body.time.get(),
        (Tone(ToneParams::Tom(p)), E) => p.noise.get(),
        (Tone(ToneParams::Tom(p)), F) => p.damp.get(),
        (Tone(ToneParams::Tom(p)), G) => p.drive.get(),
        (Tone(ToneParams::Tom(p)), H) => p.drive_type.get(),
        (Burst(Clap(p)), A) => p.tone.get(),
        (Burst(Clap(p)), B) => p.decay.get(),
        (Burst(Clap(p)), C) => f32::from(p.claps.get()),
        (Burst(Clap(p)), D) => p.spread.get(),
        (Burst(Clap(p)), E) => p.tail.get(),
        (Burst(Clap(p)), F) => p.ring.get(),
        (Burst(Clap(p)), G) => p.burst.get(),
        (Burst(Clap(p)), H) => p.room.get(),
        (Metal(MetalParams::Cowbell(p)), A) => p.pitch.get(),
        (Metal(MetalParams::Cowbell(p)), B) => p.decay.get(),
        (Metal(MetalParams::Cowbell(p)), C) => p.interval.get(),
        (Metal(MetalParams::Cowbell(p)), D) => p.tone.get(),
        (Metal(MetalParams::Cowbell(p)), E) => p.ring.get(),
        (Metal(MetalParams::Cowbell(p)), F) => p.drive.get(),
        (Metal(MetalParams::Cowbell(p)), G) => p.damp.get(),
        (Metal(MetalParams::Cowbell(p)), H) => p.bump.get(),
        (Metal(MetalParams::Bank(p)), B) => p.decay.get(),
        (Metal(MetalParams::Bank(p)), C) => p.bands[0]?.hz.get(),
        (Metal(MetalParams::Bank(p)), E) => p.bands[0]?.res.get(),
        (Metal(MetalParams::Bank(p)), F) if m == Machine::ToneCymbal => p.shimmer.get(),
        (Metal(MetalParams::Bank(p)), F) => p.noise.get(),
        (Metal(MetalParams::Bank(p)), G) if m == Machine::ToneCymbal => p.bands[2]?.weight.get(),
        (Metal(MetalParams::Bank(p)), H) if m == Machine::MetalHat => p.lp?.get(),
        (Metal(MetalParams::Jingle(p)), B) => p.length.get(),
        (Metal(MetalParams::Jingle(p)), C) => p.jingles.get(),
        (Metal(MetalParams::Jingle(p)), D) => p.shake.get(),
        (Metal(MetalParams::Jingle(p)), E) => p.tone.get(),
        (Metal(MetalParams::Jingle(p)), F) => p.res.get(),
        (Metal(MetalParams::Jingle(p)), G) => p.head.get(),
        (Burst(Shaker(p)), A) => p.grains.get(),
        (Burst(Shaker(p)), B) => p.length.get(),
        (Burst(Shaker(p)), C) => p.grain.get(),
        (Burst(Shaker(p)), D) => p.tone.get(),
        (Burst(Shaker(p)), E) => p.shake.get(),
        (Burst(Shaker(p)), F) => p.ring.get(),
        (Plain(PlainParams::Sine { body, .. } | PlainParams::Impulse { body, .. }), A) => {
            body.pitch.get()
        }
        (Plain(PlainParams::Sine { decay, .. } | PlainParams::Impulse { decay, .. }), B) => {
            decay.get()
        }
        (Plain(PlainParams::Sine { body, .. } | PlainParams::Impulse { body, .. }), C) => {
            body.depth.get()
        }
        (Plain(PlainParams::Sine { body, .. } | PlainParams::Impulse { body, .. }), D) => {
            body.time.get()
        }
        (Plain(PlainParams::Impulse { width, .. }), G) => width.get(),
        (Plain(PlainParams::Noise(p)), A) => p.decay.get(),
        _ => return None,
    })
}

/// Knobs whose law feeds a formula, not a field: each has its own check.
const NESTED: [(Machine, Knob); 10] = [
    (Machine::ToneCymbal, Knob::A),
    (Machine::ToneCymbal, Knob::D),
    (Machine::ToneCymbal, Knob::H),
    (Machine::MetalHat, Knob::A),
    (Machine::MetalHat, Knob::D),
    (Machine::MetalHat, Knob::G),
    (Machine::MetalJingle, Knob::A),
    (Machine::MetalJingle, Knob::H),
    (Machine::BodyShaker, Knob::G),
    (Machine::PlainNoise, Knob::B),
];

const KNOBS: [Knob; 8] = [
    Knob::A,
    Knob::B,
    Knob::C,
    Knob::D,
    Knob::E,
    Knob::F,
    Knob::G,
    Knob::H,
];

#[test]
fn each_knob_shows_what_the_sound_uses() {
    for m in Machine::ALL {
        for k in KNOBS {
            let Some(ks) = spec(m).knob(k) else { continue };
            let nested = NESTED.contains(&(m, k));
            for v in [0u8, 64, 127] {
                let mut set = KnobSet::defaults(m);
                set.set(k, kv(v));
                let got = field(m, k, &resolve(m, &set));
                match got {
                    Some(f) => {
                        assert!(!nested, "{m:?} {k:?} is listed nested");
                        assert_eq!(f, ks.law.at(kv(v)), "{m:?} {k:?} at {v}");
                    }
                    // HAT's LOW PASS is off at the top.
                    None if m == Machine::MetalHat && k == Knob::H && v == 127 => {}
                    None => assert!(nested, "{m:?} {k:?} has no field and is not nested"),
                }
            }
        }
    }
}

#[test]
fn nested_knobs_follow_their_laws() {
    for v in [0u8, 64, 127] {
        // JINGLE PITCH: the bank's semitones, an octave over the hat's.
        let mut set = KnobSet::defaults(Machine::MetalJingle);
        set.set(Knob::A, kv(v));
        set.set(Knob::H, kv(0));
        let MetalParams::Jingle(j) = metal(Machine::MetalJingle, &set) else {
            panic!();
        };
        let semis = spec(Machine::MetalJingle)
            .knob(Knob::A)
            .unwrap()
            .law
            .at(kv(v));
        assert!(
            close(j.hz[0].get(), 2.0 * 205.3 * (semis / 12.0).exp2()),
            "{v}"
        );
        // HAT and CYMBAL PITCH alike.
        for m in [Machine::MetalHat, Machine::ToneCymbal] {
            let mut set = KnobSet::defaults(m);
            set.set(Knob::A, kv(v));
            set.set(Knob::D, kv(0));
            let MetalParams::Bank(b) = metal(m, &set) else {
                panic!();
            };
            let semis = spec(m).knob(Knob::A).unwrap().law.at(kv(v));
            assert!(
                close(b.hz[0].get(), 205.3 * (semis / 12.0).exp2()),
                "{m:?} {v}"
            );
        }
    }
}

#[test]
fn clap_count_is_the_steps_law() {
    let law = spec(Machine::ToneClap).knob(Knob::C).unwrap().law;
    for v in 0..=127u8 {
        assert_eq!(
            f32::from(ClapCount::from_knob(kv(v)).get()),
            law.at(kv(v)),
            "{v}"
        );
    }
}

#[test]
fn event_rates_are_per_second() {
    let KernelParams::Burst(BurstParams::Shaker(s)) =
        resolve(Machine::BodyShaker, &KnobSet::defaults(Machine::BodyShaker))
    else {
        panic!();
    };
    let _: PerSecond = s.grains;
    let MetalParams::Jingle(j) = metal(
        Machine::MetalJingle,
        &KnobSet::defaults(Machine::MetalJingle),
    ) else {
        panic!();
    };
    let _: PerSecond = j.jingles;
}

/// JINGLE RING shows, and plays, at most 0.945: its top held under CLEAN's
/// self-oscillation onset (0.95), the rest of its travel the prototype's.
#[test]
fn jingle_ring_stops_short_of_self_oscillation() {
    let law = spec(Machine::MetalJingle).knob(Knob::F).unwrap().law;
    assert_eq!(law.at(kv(127)), 0.945);
    assert_eq!(law.at(kv(64)), 0.5 + (0.95 - 0.5) * (64.0 / 127.0));
}
