//! DRUM's hits (plan Task 5): a DRUM Part's notes strike its kit's pads,
//! RETRIG CUT and the choke groups end voices through the fade, note-off
//! never cuts a ringing pad unless the Sound's AMP routes say so, and pad
//! LEVEL and PAN reach the mix.
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test holds its writer for its whole run, so they play one at a time.

mod common;

use std::sync::{LazyLock, Mutex, MutexGuard};

use chimera_core::MidiChannel;
use chimera_core::dsp::drum::types::{
    ChokeGroup, DrumHit, Kit, Knob, KnobValue, PadId, Pan, Tonal, Transpose,
};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::dsp::engines::{EngineSlot, SlotKind};
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_PARTS, MAX_VOICES, SampleBudget};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::project::PartId;
use chimera_hal::BLOCK_SIZE;
use chimera_midi::value::U7;
use common::{InstRig, SR};

static WRITER: LazyLock<Mutex<KitWriter>> =
    LazyLock::new(|| Mutex::new(KitWriter::take().expect("the kit writer, once")));

const P1: PartId = PartId::ALL[0];

/// The writer, with every Part on `Kit::DEFAULT` (TONAL OFF) but Part 1, which plays `kit`; and a rig whose Part 1 (channel 1) is DRUM,
/// the other Parts on channel 16.
fn rig(kit: &Kit) -> (MutexGuard<'static, KitWriter>, InstRig) {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    for p in PartId::ALL {
        w.publish(p, &Kit::DEFAULT);
        let _ = w.take_hit(p);
    }
    w.publish(P1, kit);
    let mut r = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    for (i, part) in r.shared.parts.iter_mut().enumerate() {
        part.mix.channel = MidiChannel::new(if i == 0 { 0 } else { 15 }).unwrap();
    }
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Drum);
    (w, r)
}

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).expect("pads 1–12")
}

/// The hits sounding, by voice.
fn hits(r: &InstRig) -> Vec<DrumHit> {
    (0..MAX_VOICES)
        .filter(|&v| r.inst.voice(v).is_active())
        .filter_map(|v| match r.inst.voice(v).slot() {
            EngineSlot::Drum(d) => d.hit(),
            _ => None,
        })
        .collect()
}

#[test]
fn note_0_starts_one_drum_voice_on_pad_1() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    r.note_on(0, 0);
    r.render(1);
    let h = hits(&r);
    assert_eq!(h.len(), 1, "{h:?}");
    assert_eq!(h[0].pad(), pad(1));
    assert_eq!(h[0].params(), *Kit::DEFAULT.pad(pad(1)));
    assert_eq!(h[0].pitch(), Transpose::ZERO);
    assert_eq!(r.inst.sounding(), 1);
}

#[test]
fn every_pad_note_strikes_its_pad() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    for n in 0..12u8 {
        r.note_on(0, n);
        r.render(1);
        let newest = hits(&r)
            .into_iter()
            .filter(|h| h.pad() == pad(n + 1))
            .count();
        assert!(newest >= 1, "note {n}");
    }
}

/// ADR 0270: TONAL OFF repeats the kit every octave, each pad as tuned.
#[test]
fn tonal_off_repeats_the_kit_every_octave() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    for (note, p) in [(13, 2), (60, 1), (127, 8)] {
        r.note_on(0, note);
        r.render(1);
        assert!(
            hits(&r)
                .iter()
                .any(|h| (h.pad(), h.pitch()) == (pad(p), Transpose::ZERO)),
            "{note}"
        );
    }
}

/// ADR 0270: TONAL on pad 5 plays it from C1 up, its tuned pitch at
/// middle C; C0–B0 stay the kit.
#[test]
fn tonal_plays_its_pad_tuned_at_middle_c() {
    let mut k = Kit::DEFAULT;
    k.set_tonal(Tonal::Pad(pad(5)));
    let (_w, mut r) = rig(&k);
    r.note_on(0, 60);
    r.render(1);
    let h = hits(&r);
    assert_eq!(h.len(), 1, "{h:?}");
    assert_eq!((h[0].pad(), h[0].pitch()), (pad(5), Transpose::ZERO));
    for (note, st) in [(67, 7), (12, -48)] {
        r.note_on(0, note);
        r.render(1);
        assert!(
            hits(&r)
                .iter()
                .any(|h| (h.pad(), h.pitch()) == (pad(5), Transpose::new(st).unwrap())),
            "{note}"
        );
    }
    r.note_on(0, 2);
    r.render(1);
    assert!(
        hits(&r)
            .iter()
            .any(|h| (h.pad(), h.pitch()) == (pad(3), Transpose::ZERO))
    );
}

#[test]
fn a_plain_note_on_another_engine_strikes_nothing() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Va);
    r.note_on(0, 0);
    r.render(1);
    assert!(hits(&r).is_empty());
    assert_eq!(
        r.inst.slot_kinds()[0..]
            .iter()
            .filter(|k| **k == SlotKind::Drum)
            .count(),
        0
    );
}

/// Hats on pads 8 (closed) and 9 (open), both in group A.
fn hats_in_a() -> Kit {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(8)).set_choke(ChokeGroup::A);
    k.pad_mut(pad(9)).set_choke(ChokeGroup::A);
    k
}

#[test]
fn a_closed_hat_chokes_the_open_one_through_the_fade() {
    let (_w, mut r) = rig(&hats_in_a());
    r.note_on(0, 8); // OH
    r.render(4);
    assert_eq!(
        hits(&r).iter().map(|h| h.pad()).collect::<Vec<_>>(),
        [pad(9)]
    );
    r.note_on(0, 7); // CH
    // The OH fades over its 128 samples: still there the block after.
    r.render(1);
    assert_eq!(hits(&r).len(), 2);
    r.render(3);
    assert_eq!(
        hits(&r).iter().map(|h| h.pad()).collect::<Vec<_>>(),
        [pad(8)]
    );
}

#[test]
fn a_hit_leaves_other_groups_and_off_ringing() {
    let (_w, mut r) = rig(&hats_in_a());
    r.note_on(0, 0); // KICK, OFF
    r.note_on(0, 10); // COWBELL, D
    r.note_on(0, 8); // OH, A
    r.render(2);
    r.note_on(0, 7); // CH, A
    r.render(4);
    let mut pads: Vec<_> = hits(&r).iter().map(|h| h.pad()).collect();
    pads.sort_by_key(|p| p.index());
    assert_eq!(pads, [pad(1), pad(8), pad(11)]);
}

#[test]
fn a_cut_pad_struck_again_leaves_one_voice() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    r.note_on(0, 0);
    r.render(2);
    r.note_on(0, 0);
    r.render(4);
    assert_eq!(hits(&r).len(), 1);
    assert_eq!(r.inst.sounding(), 1);
}

#[test]
fn a_stack_pad_struck_again_leaves_two_voices() {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(1)).set_retrig(Retrig::Stack);
    let (_w, mut r) = rig(&k);
    r.note_on(0, 0);
    r.render(2);
    r.note_on(0, 0);
    r.render(4);
    assert_eq!(hits(&r).iter().filter(|h| h.pad() == pad(1)).count(), 2);
}

#[test]
fn note_off_never_cuts_a_ringing_pad() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    r.note_on(0, 0);
    r.render(1);
    r.note_off(0, 0);
    // 0.1 s on: the kick still rings.
    r.render(SR as usize / 10 / BLOCK_SIZE);
    assert_eq!(hits(&r).len(), 1);
    // And ends on its own, its slot free.
    r.render(3 * SR as usize / BLOCK_SIZE);
    assert_eq!(r.inst.sounding(), 0);
    assert!(r.inst.allocator().slots().iter().all(|s| s.is_free()));
}

/// Whether pad 10 (CYMBAL, DECAY full) still rings 0.6 s after its
/// note-off, with or without ENV 1 routed into VCA (its release 300 ms).
fn cymbal_rings_past_its_release(vca: bool) -> bool {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(10))
        .knobs_mut()
        .set(Knob::B, KnobValue::new(U7::MAX));
    let (_w, mut r) = rig(&k);
    if vca {
        r.shared.parts[0]
            .mod_state
            .set_fixed(ModSource::Env1.index(), FixedCol::Vca, 127);
    }
    r.note_on(0, 9);
    r.render(1);
    r.note_off(0, 9);
    r.render(6 * SR as usize / 10 / BLOCK_SIZE);
    !hits(&r).is_empty()
}

#[test]
fn the_amp_envelope_on_vca_gates_a_pad_at_note_off() {
    assert!(
        cymbal_rings_past_its_release(false),
        "no route: it rings on"
    );
    assert!(
        !cymbal_rings_past_its_release(true),
        "ENV 1's release ends it"
    );
}

/// Review Focus 3: a ringing DRUM Part switched to VA ends its voices
/// through the fade; nothing restarts on VA, keys held or not.
#[test]
fn drum_to_va_while_ringing_ends_its_voices() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    r.note_on(0, 0);
    r.note_on(0, 1);
    r.note_on(0, 8);
    r.render(2);
    r.note_off(0, 1);
    assert_eq!(r.inst.sounding(), 3);
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Va);
    r.render(1);
    // Fading, on the Sound each last played: none rebuilt as VA yet.
    for v in 0..MAX_VOICES {
        let voice = r.inst.voice(v);
        if voice.is_active() {
            assert!(voice.fading(), "voice {v}");
            assert_eq!(voice.kind(), SlotKind::Drum);
        }
    }
    r.render(3);
    assert_eq!(r.inst.sounding(), 0);
    assert!(hits(&r).is_empty());
    // Still none a block later: no held key restarted as a VA note.
    r.render(2);
    assert_eq!(r.inst.sounding(), 0);
}

#[test]
fn va_to_drum_while_held_starts_no_hit() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Va);
    r.note_on(0, 60);
    r.render(2);
    assert_eq!(r.inst.sounding(), 1);
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Drum);
    r.render(6);
    assert_eq!(r.inst.sounding(), 0);
    assert!(hits(&r).is_empty());
}

/// Review Focus 4: a 64th-note roll on one CUT pad at 300 BPM for 10 s
/// never holds more than the hit and the one fading, and the pool is
/// empty after 3 s of silence.
#[test]
fn a_64th_roll_at_300_bpm_returns_the_pool() {
    let (_w, mut r) = rig(&Kit::DEFAULT);
    // A 64th at 300 BPM: 60 / 300 / 16 s = 600 samples.
    let step = SR as usize * 60 / 300 / 16;
    let blocks = 10 * SR as usize / BLOCK_SIZE;
    let mut next = 0;
    let mut most = 0;
    for b in 0..blocks {
        let at = b * BLOCK_SIZE;
        if at >= next {
            r.note_on(0, 7); // a closed hat
            next += step;
        } else if at >= next - step / 2 && at < next - step / 2 + BLOCK_SIZE {
            r.note_off(0, 7);
        }
        r.render(1);
        most = most.max(r.inst.sounding());
    }
    assert!((1..=2).contains(&most), "{most} voices at once");
    r.note_off(0, 7);
    r.render(3 * SR as usize / BLOCK_SIZE);
    assert_eq!(r.inst.sounding(), 0);
    assert!(r.inst.allocator().slots().iter().all(|s| s.is_free()));
}

#[test]
fn level_scales_the_pad() {
    let peak = |level: u8| {
        let mut k = Kit::DEFAULT;
        k.pad_mut(pad(1)).set_level(U7::new(level).unwrap());
        let (_w, mut r) = rig(&k);
        r.note_on(0, 0);
        let mut m = 0.0f32;
        for _ in 0..40 {
            r.render(1);
            m = r.inst.part_bus(0).iter().fold(m, |m, x| m.max(x.abs()));
        }
        m
    };
    let (full, half, none) = (peak(127), peak(64), peak(0));
    assert!(full > 0.05, "{full}");
    assert!((half / full - 64.0 / 127.0).abs() < 0.02, "{half} / {full}");
    assert_eq!(none, 0.0);
}

/// The pair's left and right peaks over the kick's first 40 blocks.
fn pair_peaks(pan: i8) -> (f32, f32) {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(1)).set_pan(Pan::clamped(pan));
    let (_w, mut r) = rig(&k);
    r.note_on(0, 0);
    let (mut l, mut rr) = (0.0f32, 0.0f32);
    for _ in 0..40 {
        let out = r.block(&r.shared.clone());
        for f in out[0].chunks(2) {
            l = l.max(f[0].abs());
            rr = rr.max(f[1].abs());
        }
    }
    (l, rr)
}

#[test]
fn pan_places_the_pad() {
    let (cl, cr) = pair_peaks(0);
    assert!((cl - cr).abs() < 1e-6 * cl.max(1e-9), "centre {cl} {cr}");
    let (ll, lr) = pair_peaks(-64);
    assert!(lr < 1e-4 * ll, "hard left {ll} {lr}");
    let (rl, rr) = pair_peaks(63);
    assert!(rl < 0.05 * rr, "right {rl} {rr}");
}

#[test]
fn a_live_hit_tells_the_ui_its_pad() {
    let (mut w, mut r) = rig(&Kit::DEFAULT);
    assert_eq!(w.take_hit(P1), None);
    r.note_on(0, 8);
    assert_eq!(w.take_hit(P1).map(|s| s.last()), Some(pad(9)));
    assert_eq!(w.take_hit(P1), None);
    for p in PartId::ALL.iter().skip(1).take(MAX_PARTS - 1) {
        assert_eq!(w.take_hit(*p), None);
    }
}

#[test]
fn a_take_holds_every_pad_struck_since_the_last() {
    let (mut w, mut r) = rig(&Kit::DEFAULT);
    for n in [0, 7, 3] {
        r.note_on(0, n);
    }
    let s = w.take_hit(P1).expect("three hits");
    assert_eq!(s.last(), pad(4));
    assert_eq!(
        s.pads().iter().collect::<Vec<_>>(),
        [pad(1), pad(4), pad(8)]
    );
    assert_eq!(w.take_hit(P1), None);
}

#[test]
fn the_view_reads_what_was_published() {
    let (mut w, _r) = rig(&Kit::DEFAULT);
    let mut k = hats_in_a();
    k.pad_mut(pad(3)).set_pan(Pan::clamped(-20));
    w.publish(PartId::ALL[2], &k);
    for p in PadId::ALL {
        let h = KIT_VIEW.hit(
            PartId::ALL[2],
            chimera_core::MidiNote::new(p.index() as u8).unwrap(),
        );
        assert_eq!(h.params(), *k.pad(p));
    }
}

/// A hit is booked at its kit's bill (`Voice::drum_cost`), a 2040 pad
/// over the Sound's CLEAN.
#[test]
fn a_hit_is_billed_for_its_kits_pads() {
    let booked = |kit: &Kit| {
        let (_w, mut r) = rig(kit);
        r.note_on(0, 0);
        r.render(1);
        let s = r.inst.allocator().slots();
        s.iter().map(|s| s.cost()).max().expect("a voice")
    };
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(5))
        .sound_mut()
        .filter_mut()
        .set_kind(chimera_core::dsp::filter::FilterKind::K2040);
    let gap = chimera_core::dsp::drum::sound::kit_bill(&k).0;
    assert!(gap > 0);
    assert_eq!(booked(&k).0, booked(&Kit::DEFAULT).0 + gap);
}

/// A full pool: the next hit takes a sounding voice, waits out its fade
/// (`waiting`), then strikes.
#[test]
fn a_hit_on_a_full_pool_waits_out_the_steal() {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(1)).set_retrig(Retrig::Stack);
    let (_w, mut r) = rig(&k);
    for _ in 0..MAX_VOICES {
        r.note_on(0, 0);
        r.render(1);
    }
    assert_eq!(hits(&r).len(), MAX_VOICES);
    r.note_on(0, 1);
    r.render(4);
    let h = hits(&r);
    assert_eq!(h.iter().filter(|h| h.pad() == pad(2)).count(), 1, "{h:?}");
    assert!(h.len() <= MAX_VOICES);
}

/// Part 2, moved to channel 2, on DRUM fills the pool with STACK kicks, so Part
/// 1's next hits wait out another Part's fade (`waiting`).
fn fill_with_part_2(w: &mut KitWriter, r: &mut InstRig) {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(1)).set_retrig(Retrig::Stack);
    w.publish(PartId::ALL[1], &k);
    *r.shared.parts[1].params = ParamSnapshot::for_engine(EngineType::Drum);
    r.shared.parts[1].mix.channel = MidiChannel::new(1).unwrap();
    for _ in 0..MAX_VOICES {
        r.note_on(1, 0);
        r.render(1);
    }
    assert_eq!(hits(r).len(), MAX_VOICES);
}

/// Part 1's slots, sounding or waiting.
fn part_1_slots(r: &InstRig) -> usize {
    let slots = r.inst.allocator().slots();
    slots.iter().filter(|s| s.part() == Some(0)).count()
}

#[test]
fn a_closed_hat_chokes_an_open_one_still_waiting() {
    let (mut w, mut r) = rig(&Kit::DEFAULT);
    fill_with_part_2(&mut w, &mut r);
    r.note_on(0, 8); // OH, waits
    r.note_on(0, 7); // CH, same block
    r.render(4);
    let h = hits(&r);
    assert_eq!(h.iter().filter(|h| h.pad() == pad(9)).count(), 0, "{h:?}");
    assert_eq!(h.iter().filter(|h| h.pad() == pad(8)).count(), 1, "{h:?}");
    assert_eq!(part_1_slots(&r), 1);
}

#[test]
fn a_cut_pad_struck_twice_while_waiting_leaves_one_voice() {
    let (mut w, mut r) = rig(&Kit::DEFAULT);
    fill_with_part_2(&mut w, &mut r);
    r.note_on(0, 1);
    r.note_on(0, 1);
    r.render(4);
    let h = hits(&r);
    assert_eq!(h.iter().filter(|h| h.pad() == pad(2)).count(), 1, "{h:?}");
    assert_eq!(part_1_slots(&r), 1);
}

/// A waiting hit whose Part leaves DRUM before it starts is dropped
/// unheard: it never plays as a note on the new engine.
#[test]
fn a_waiting_hit_whose_part_left_drum_is_dropped() {
    let (mut w, mut r) = rig(&Kit::DEFAULT);
    fill_with_part_2(&mut w, &mut r);
    r.note_on(0, 1);
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Va);
    r.render(4);
    assert_eq!(part_1_slots(&r), 0);
}

/// Likewise a waiting note whose Part became DRUM strikes no pad.
#[test]
fn a_waiting_note_whose_part_became_drum_is_dropped() {
    let (mut w, mut r) = rig(&Kit::DEFAULT);
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Va);
    fill_with_part_2(&mut w, &mut r);
    r.note_on(0, 60);
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Drum);
    r.render(4);
    assert_eq!(part_1_slots(&r), 0);
}
