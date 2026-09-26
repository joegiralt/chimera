use std::mem::MaybeUninit;

use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{BLOCK_SIZE, CPU_HZ_REV_V, DAC_PAIRS, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
use chimera_core::modulation::ModState;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiChannel, MidiNote, Velocity};

const SR: u32 = chimera_hal::SAMPLE_RATE;
const BUDGET: SampleBudget = SampleBudget::for_cpu(CPU_HZ_REV_V);

// On the chip the slots are NOLOAD statics holding boot garbage, so a
// missed zero-valued field write must show up here too.
fn poisoned<T>() -> Box<MaybeUninit<T>> {
    let mut slot = Box::<T>::new_uninit();
    // SAFETY: the pointer is the box's own allocation, valid and aligned for
    // one `T`; bytes written into a `MaybeUninit` need no validity.
    unsafe { slot.as_mut_ptr().write_bytes(0xA5, 1) };
    slot
}

fn event(ch: u8, note: u8, kind: NoteKind) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(ch).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind,
    }
}

fn every_engine_and_every_effect() -> AudioShared {
    let mut s = AudioShared::default();
    s.parts[0].params = ParamSnapshot::for_engine(EngineType::Pizza);
    s.parts[1].params = ParamSnapshot::for_engine(EngineType::Fm);
    s.parts[2].params = ParamSnapshot::for_engine(EngineType::Modal);
    s.parts[2].params.modal.mode = ResonatorMode::String;
    s.parts[3].params = ParamSnapshot::for_engine(EngineType::Modal);
    s.parts[3].params.modal.mode = ResonatorMode::Sympathetic;
    for part in &mut s.parts[..4] {
        part.mix.sends = [0.3; FX_SENDS];
    }
    s.fx.chorus.mode = 1;
    s.fx.chorus.mix = 0.5;
    s.fx.delay.mix = 0.5;
    s.fx.reverb.mix = 0.5;
    s
}

fn play(inst: &mut Instrument, fx: &mut FxBus) -> Vec<u32> {
    let shared = every_engine_and_every_effect();
    let notes = [(0, 60), (1, 64), (2, 40), (3, 45)];
    let mut out: DacOut = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
    let mut bits = Vec::new();
    for b in 0..200 {
        for &(ch, n) in &notes {
            if b == 0 {
                inst.handle(event(ch, n, NoteKind::On(Velocity::DEFAULT)), &shared);
            }
            if b == 100 {
                inst.handle(event(ch, n, NoteKind::Off), &shared);
            }
        }
        inst.render(fx, &mut out, &shared);
        bits.extend(out.iter().flatten().map(|s| s.to_bits()));
    }
    bits
}

#[test]
fn instrument_built_in_place_renders_like_new() {
    let mut by_value = Box::new(Instrument::new(SR, BUDGET));
    let mut fx_a = Box::new(FxBus::new());
    let mut slot = poisoned::<Instrument>();
    let mut fx_slot = poisoned::<FxBus>();
    let in_place = Instrument::init_in_place(&mut slot, SR, BUDGET);
    let fx_b = FxBus::init_in_place(&mut fx_slot);
    assert_eq!(play(&mut by_value, &mut fx_a), play(in_place, fx_b));
    assert_eq!(in_place.allocator().budget(), BUDGET);
}

#[test]
fn fx_bus_built_in_place_processes_like_new() {
    let mut a = Box::new(FxBus::new());
    let mut slot = poisoned::<FxBus>();
    let b = FxBus::init_in_place(&mut slot);
    let mut params = every_engine_and_every_effect().fx;
    let mut x = 0x1234_5678u32;
    for block in 0..300 {
        params.reverb.reverb_type = (block / 100) as u8;
        let mut sends = [[0.0f32; BLOCK_SIZE]; FX_SENDS];
        for s in sends.iter_mut().flatten() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *s = (x as f32 / u32::MAX as f32) - 0.5;
        }
        let mut sends_b = sends;
        let (mut ret_a, mut ret_b) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        a.process(&mut sends, &params, SR, &mut ret_a);
        b.process(&mut sends_b, &params, SR, &mut ret_b);
        assert_eq!(
            ret_a.map(f32::to_bits),
            ret_b.map(f32::to_bits),
            "block {block}"
        );
    }
}

#[test]
fn voice_built_in_place_renders_like_new_for_every_engine() {
    let modes = [
        ResonatorMode::String,
        ResonatorMode::Modal,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ];
    for engine in EngineType::ALL {
        for mode in modes {
            let mut params = ParamSnapshot::for_engine(engine);
            params.modal.mode = mode;
            let mut a = Box::new(Voice::new(SR));
            let mut slot = poisoned::<Voice>();
            let b = Voice::init_in_place(&mut slot, SR);
            let note = MidiNote::new(52).unwrap();
            a.note_on(note, Velocity::DEFAULT, &params);
            b.note_on(note, Velocity::DEFAULT, &params);
            let (mut xa, mut xb) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
            for block in 0..60 {
                a.render(&mut xa, &params, &ModState::new());
                b.render(&mut xb, &params, &ModState::new());
                assert_eq!(
                    xa.map(f32::to_bits),
                    xb.map(f32::to_bits),
                    "{engine:?} {mode:?} block {block}"
                );
            }
        }
    }
}
