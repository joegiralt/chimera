//! The voice FX slot (ADR 0155) driven on rendered engine blocks: its TYPE
//! and PLACE switches crossfade over `FX_FADE`, MIX 0 is the dry signal on
//! every engine, and each engine meets the slot's reference level.

mod common;

use chimera_core::dsp::algo::engine::AlgoLive;
use chimera_core::dsp::engines::{EngineCtx, EngineSlot, SlotKind};
use chimera_core::dsp::filter::{Filter, FilterMods};
use chimera_core::dsp::modal::SymPool;
use chimera_core::dsp::voice_fx::params::{FxType, Place, VoiceFxParams};
use chimera_core::dsp::voice_fx::{FX_FADE, VoiceFx};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::taps::TapRows;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::tape::{OneSource, held_plain, sine};
use common::{SR, VEL, clicks, rms_diff};

type Block = [f32; BLOCK_SIZE];

/// INIT on `kind`, notes `notes` at `VEL` from block 0: each note's engine
/// output (the slot's PRE input), `blocks` long. TAPE plays a full-scale
/// sine.
fn engine_blocks(
    kind: EngineType,
    notes: &[u8],
    blocks: usize,
) -> (ParamSnapshot, Vec<Vec<Block>>) {
    let p = Sound::init(kind).params;
    let mut src = OneSource::new(&held_plain(
        &sine(48_000, 48_000.0 / 261.63, 96_000),
        48_000,
    ));
    let id = Some(src.id);
    let out = notes
        .iter()
        .map(|&n| {
            let mut pool = SymPool::boxed();
            let mut e = EngineSlot::new(SlotKind::of(&p));
            let live = AlgoLive::from_params(&p.algo);
            (0..blocks)
                .map(|b| {
                    let view = src.view();
                    let mut c = EngineCtx {
                        sym: &mut pool,
                        streams: None,
                        src: &view,
                        part: chimera_core::project::PartId::ALL[0],
                        tune: 0.0,
                    };
                    if b == 0 {
                        let (note, vel) = (MidiNote::new(n).unwrap(), Velocity::new(VEL).unwrap());
                        e.note_on(note, vel, &p, SR, id, &mut c);
                    }
                    let mut buf = [0.0; BLOCK_SIZE];
                    e.render(
                        &mut buf,
                        &p,
                        &live,
                        SR,
                        &mut c,
                        &mut TapRows::new([false; 2]),
                    );
                    buf
                })
                .collect()
        })
        .collect();
    (p, out)
}

/// One voice's slot and filter on `input`: PRE, the filter (unless `bare`),
/// POST, with block `b`'s setting `fx(b)`; the slot runs while TYPE isn't
/// OFF.
fn voice(
    p: &ParamSnapshot,
    input: &[Block],
    bare: bool,
    fx: impl Fn(usize) -> VoiceFxParams,
) -> Vec<f32> {
    let (mut slot, mut filter) = (VoiceFx::new(), Filter::new());
    let scale = p.engine().fx_scale();
    let mut out = Vec::with_capacity(input.len() * BLOCK_SIZE);
    for (b, x) in input.iter().enumerate() {
        let q = fx(b);
        let blk = slot.step(&q, q.ty() != FxType::Off);
        let mut buf = *x;
        slot.apply(&blk, Place::Pre, &mut buf, scale, false, None);
        if !bare {
            filter.process(&mut buf, &p.filter, SR, &FilterMods::NONE);
        }
        slot.apply(&blk, Place::Post, &mut buf, scale, false, None);
        out.extend_from_slice(&buf);
    }
    out
}

/// The chord's voices summed (`voice`) at OUT LEVEL, as the VCA sends
/// them to the mixer.
fn chord(
    p: &ParamSnapshot,
    notes: &[Vec<Block>],
    bare: bool,
    fx: impl Fn(usize) -> VoiceFxParams + Copy,
) -> Vec<f32> {
    let mut sum = vec![0.0; notes[0].len() * BLOCK_SIZE];
    for n in notes {
        for (s, v) in sum.iter_mut().zip(voice(p, n, bare, fx)) {
            *s += v * p.out.volume;
        }
    }
    sum
}

fn setting(ty: FxType, place: Place) -> VoiceFxParams {
    let mut q = VoiceFxParams::default();
    q.set_type(ty);
    q.place = place;
    q.pots = [0.3, 0.5, 0.5];
    q
}

/// The largest second difference over blocks `a..b`, from the two samples
/// before `a`: a snap at `a`'s first sample is in it.
fn d2(out: &[f32], a: usize, b: usize) -> f32 {
    out[a * BLOCK_SIZE - 2..b * BLOCK_SIZE]
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0, f32::max)
}

const FADE_BLOCKS: usize = FX_FADE / BLOCK_SIZE;

/// Each switch at block `s` crossfades over `FADE_BLOCKS`: no click, and no
/// sharper bend than the steady blocks either side (`the_gate_never_clicks`).
fn switches_are_click_free(out: &[f32], at: &[usize], label: &str) {
    assert!(clicks(out).is_empty(), "{label}: {:?}", &clicks(out)[..1]);
    for &s in at {
        let e = s + FADE_BLOCKS;
        let steady = d2(out, s - 10, s).max(d2(out, e, e + 8));
        let switch = d2(out, s, e);
        assert!(
            switch < 1.5 * steady,
            "{label} at {s}: {switch} vs {steady}"
        );
    }
}

const CHORD: [u8; 3] = [60, 64, 67];

/// VA INIT's saw chord through a 1.2 kHz filter: its edges rounded, so a
/// click heard is the slot's, not the saw's.
fn held_chord(blocks: usize) -> (ParamSnapshot, Vec<Vec<Block>>) {
    let (mut p, notes) = engine_blocks(EngineType::Va, &CHORD, blocks);
    p.filter.cutoff = 1200.0;
    (p, notes)
}

#[test]
fn a_type_change_mid_note_is_click_free() {
    let (p, notes) = held_chord(120);
    let out = chord(&p, &notes, false, |b| {
        let ty = match b {
            0..40 => FxType::Soft,
            40..65 => FxType::Fold,
            65..90 => FxType::Off,
            _ => FxType::Soft,
        };
        // POST: the filter can't round a snap off.
        setting(ty, Place::Post)
    });
    switches_are_click_free(&out, &[40, 65, 90], "TYPE");
}

#[test]
fn a_place_change_mid_note_is_click_free() {
    let (p, notes) = held_chord(95);
    let out = chord(&p, &notes, false, |b| {
        let place = if (40..65).contains(&b) {
            Place::Post
        } else {
            Place::Pre
        };
        setting(FxType::Soft, place)
    });
    switches_are_click_free(&out, &[40, 65], "PLACE");
}

/// FUZZ PRE to POST and back: each side runs its own `Fuzz` on its own
/// signal, the side fading in from fresh memory.
#[test]
fn a_fuzz_place_change_mid_note_is_click_free() {
    let (p, notes) = held_chord(95);
    let out = chord(&p, &notes, false, |b| {
        let place = if (40..65).contains(&b) {
            Place::Post
        } else {
            Place::Pre
        };
        setting(FxType::Fuzz, place)
    });
    switches_are_click_free(&out, &[40, 65], "FUZZ PLACE");
}

/// TYPE turned every block for `n` blocks: changes mid-fade wait for its
/// end and the last one wins. One arriving just after a fade starts waits
/// it out, so the bound after the last change is two fades, 2·`FX_FADE`:
/// past it the slot is a fresh voice's on the final setting, whatever the
/// turning's length. The equality is the slot's alone (`bare`): the
/// filter's memory of the fade isn't the slot's.
#[test]
fn turning_type_through_all_while_a_chord_sounds_never_sticks() {
    let (p, notes) = held_chord(40 + 45 + 2 * FADE_BLOCKS + 2 + 8);
    for n in 20..=45 {
        let last = 40 + n - 1;
        let turn = |b: usize| {
            let mut q = setting(FxType::Soft, Place::Post);
            for k in 40..=b.min(last) {
                q.set_type(FxType::LIST[(k + 1) % FxType::LIST.len()]);
            }
            q
        };
        let out = chord(&p, &notes, true, turn);
        let fresh = chord(&p, &notes, true, |_| turn(last));
        let from = (last + 2 * FADE_BLOCKS + 2) * BLOCK_SIZE;
        let diff = rms_diff(&out[from..], &fresh[from..]);
        assert!(diff < 1e-6, "{n} blocks: {diff}");
        let heard = chord(&p, &notes, false, turn);
        assert!(
            clicks(&heard).is_empty(),
            "{n} blocks: {:?}",
            &clicks(&heard)[..1]
        );
    }
}

/// The reference scale cancels: at MIX 0 any TYPE, PRE or POST, steady or
/// switched in mid-note, is the dry signal bit for bit.
#[test]
fn mix_zero_is_dry_on_every_engine() {
    let bits = |x: &[f32]| x.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    for kind in EngineType::ALL {
        let (p, notes) = engine_blocks(kind, &[60], 40);
        let n = &notes[0];
        let dry = bits(&voice(&p, n, false, |_| setting(FxType::Off, Place::Pre)));
        for ty in FxType::LIST {
            for place in [Place::Pre, Place::Post] {
                let wet = |b: usize| {
                    let mut q = setting(ty, place);
                    (q.pots, q.mix) = ([1.0; 3], 0.0);
                    if b < 10 {
                        q.set_type(FxType::Off);
                    }
                    q.place = place;
                    q
                };
                let got = bits(&voice(&p, n, false, wet));
                assert!(got == dry, "{kind:?} {ty:?} {place:?}");
            }
        }
    }
}

#[test]
fn each_engine_meets_the_fx_reference() {
    for kind in EngineType::ALL {
        let (_, notes) = engine_blocks(kind, &[60], 100);
        let peak = notes[0]
            .iter()
            .flatten()
            .fold(0.0f32, |m, x| m.max(x.abs()));
        let at = peak * kind.fx_scale();
        assert!(
            (0.7..=1.4).contains(&at),
            "{kind:?}: peak {peak} × {}",
            kind.fx_scale()
        );
    }
}
