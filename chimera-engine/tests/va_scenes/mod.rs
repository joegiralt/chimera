//! VA's scenes around the saw's solo (ADR 0283): MODE detours into and out
//! of it, a Sound browsed (MODE and WAVE in the same block), glide, a
//! steal, a restart, SHAPE swept and at its ends. They use only the
//! engine's playing API, so the same scenes print on a tree without the
//! solo.

use core::mem::MaybeUninit;

use chimera_engine::dsp::va::{VaEngine, VaMode, VaParams};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;

pub const SR: u32 = 48_000;
/// Blocks a scene plays.
pub const BLOCKS: usize = 70;
/// WAVE's saw anchor, as `osc::SAW_ANCHOR` names it (a tree without the
/// solo has no name for it).
pub const SAW: f32 = 2.0 / 3.0;
/// WAVE off the saw's anchor, its pulse at weight 0.4.
pub const OFF: f32 = 0.8;

/// A scene: its note, its params at each block, and what happens to the
/// voice before a block (`Event`).
pub struct Scene {
    pub name: &'static str,
    pub note: u8,
    pub params: fn(usize) -> VaParams,
    pub event: fn(usize) -> Event,
}

/// What a scene does to the voice before a block.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Event {
    None,
    /// A steal's glide to this note (`glide_on`).
    Glide(u8),
    /// A steal: the ramps start on the new params (`hold`), a new note on.
    Steal(u8),
    /// The same voice's note again, its ramps running on.
    Restart,
}

/// MODE `mode` at WAVE `w` (both oscillators), BLEND ½, SHAPE off the ends,
/// a sub, STABILITY's jitter; OSC 2 a fifth up.
pub fn at(mode: VaMode, w: f32) -> VaParams {
    VaParams {
        mode,
        wave: [w; 2],
        amount: 0.5,
        shape: [0.3, 0.6],
        stability: 0.5,
        sub: 0.5,
        coarse: [0, 7],
        ..VaParams::default()
    }
}

fn no_event(_: usize) -> Event {
    Event::None
}

/// MIX, a detour through `mode` for blocks 20..40, MIX again, all on `w0`
/// until `turn`, then on the saw's anchor.
fn detour(mode: VaMode, b: usize, w0: f32, turn: usize) -> VaParams {
    let w = if b < turn { w0 } else { SAW };
    at(
        if (20..40).contains(&b) {
            mode
        } else {
            VaMode::Mix
        },
        w,
    )
}

macro_rules! detours {
    ($($from_solo:ident, $off_in:ident, $off_after:ident: $mode:expr;)*) => {
        $(
            /// From the solo through the MODE and back.
            fn $from_solo(b: usize) -> VaParams {
                detour($mode, b, SAW, 0)
            }
            /// Off the anchor into the MODE, WAVE onto it inside.
            fn $off_in(b: usize) -> VaParams {
                detour($mode, b, OFF, 25)
            }
            /// Off the anchor through the MODE, WAVE onto it with MIX back.
            fn $off_after(b: usize) -> VaParams {
                detour($mode, b, OFF, 40)
            }
        )*
    };
}

detours! {
    loopfm_from_solo, loopfm_off_in, loopfm_off_after: VaMode::LoopFm;
    fm_from_solo, fm_off_in, fm_off_after: VaMode::Fm;
    phase_from_solo, phase_off_in, phase_off_after: VaMode::Phase;
    soft_from_solo, soft_off_in, soft_off_after: VaMode::SoftSync;
}

/// SYNC (OSC 1's solo) off the anchor, LOOPFM for 20..40, SYNC on it.
fn sync_loopfm(b: usize) -> VaParams {
    match b {
        0..20 => at(VaMode::Sync, OFF),
        20..40 => at(VaMode::LoopFm, OFF),
        _ => at(VaMode::Sync, SAW),
    }
}

/// A Sound browsed: RING at WAVE 0.3, then MIX on the anchor, MODE and
/// WAVE in the same block.
fn browse_ring_mix(b: usize) -> VaParams {
    if b < 20 {
        at(VaMode::Ring, 0.3)
    } else {
        at(VaMode::Mix, SAW)
    }
}

/// Browsed further: SYNC at 0.9, XOR on the anchor, LOOPFM at 0.5, MIX on
/// the anchor, each in one block.
fn browse_chain(b: usize) -> VaParams {
    match b {
        0..20 => at(VaMode::Sync, 0.9),
        20..40 => at(VaMode::Xor, SAW),
        40..55 => at(VaMode::LoopFm, 0.5),
        _ => at(VaMode::Mix, SAW),
    }
}

fn mix_saw(_: usize) -> VaParams {
    at(VaMode::Mix, SAW)
}

fn glide_events(b: usize) -> Event {
    match b {
        20 => Event::Glide(55),
        45 => Event::Glide(48),
        _ => Event::None,
    }
}

/// MIX off the anchor; a steal at 20 lands on it, its ramps on their
/// values, the oscillators' held constants the old note's.
fn steal_params(b: usize) -> VaParams {
    if b < 20 {
        at(VaMode::Mix, OFF)
    } else {
        at(VaMode::Mix, SAW)
    }
}

fn steal_events(b: usize) -> Event {
    if b == 20 {
        Event::Steal(60)
    } else {
        Event::None
    }
}

fn restart_events(b: usize) -> Event {
    if b == 30 { Event::Restart } else { Event::None }
}

/// SHAPE swept both ways across the scene, on the anchor.
fn shape_sweep(b: usize) -> VaParams {
    let x = b as f32 / BLOCKS as f32;
    VaParams {
        shape: [x, 1.0 - x],
        ..at(VaMode::Mix, SAW)
    }
}

/// SYNC (OSC 1's solo) with SHAPE swept.
fn sync_shape_sweep(b: usize) -> VaParams {
    VaParams {
        mode: VaMode::Sync,
        ..shape_sweep(b)
    }
}

/// SHAPE at its ends, swapped halfway.
fn shape_ends(b: usize) -> VaParams {
    VaParams {
        shape: if b < 35 { [0.0, 1.0] } else { [1.0, 0.0] },
        ..at(VaMode::Mix, SAW)
    }
}

pub const SCENES: [Scene; 21] = {
    const fn s(
        name: &'static str,
        params: fn(usize) -> VaParams,
        event: fn(usize) -> Event,
    ) -> Scene {
        Scene {
            name,
            note: 48,
            params,
            event,
        }
    }
    [
        s("LOOPFM from the solo", loopfm_from_solo, no_event),
        s("LOOPFM off, onto it inside", loopfm_off_in, no_event),
        s("LOOPFM off, onto it after", loopfm_off_after, no_event),
        s("FM from the solo", fm_from_solo, no_event),
        s("FM off, onto it inside", fm_off_in, no_event),
        s("FM off, onto it after", fm_off_after, no_event),
        s("PHASE from the solo", phase_from_solo, no_event),
        s("PHASE off, onto it inside", phase_off_in, no_event),
        s("PHASE off, onto it after", phase_off_after, no_event),
        s("SOFT from the solo", soft_from_solo, no_event),
        s("SOFT off, onto it inside", soft_off_in, no_event),
        s("SOFT off, onto it after", soft_off_after, no_event),
        s("SYNC through LOOPFM", sync_loopfm, no_event),
        s("browse RING to MIX", browse_ring_mix, no_event),
        s("browse SYNC XOR LOOPFM MIX", browse_chain, no_event),
        s("glide", mix_saw, glide_events),
        s("steal onto the anchor", steal_params, steal_events),
        s("restart", mix_saw, restart_events),
        s("SHAPE swept", shape_sweep, no_event),
        s("SHAPE ends", shape_ends, no_event),
        s("SYNC SHAPE swept", sync_shape_sweep, no_event),
    ]
};

/// `scene`'s blocks from a fresh voice, `seen` after each block.
pub fn play(scene: &Scene, mut seen: impl FnMut(usize, &VaEngine)) -> Vec<[f32; BLOCK_SIZE]> {
    let part = PartId::new(0).unwrap();
    let mut slot = Box::new(MaybeUninit::<VaEngine>::uninit());
    let e = VaEngine::init_in_place(&mut slot);
    e.hold();
    e.note_on(scene.note, &(scene.params)(0), part, SR);
    (0..BLOCKS)
        .map(|b| {
            let p = (scene.params)(b);
            match (scene.event)(b) {
                Event::None => {}
                Event::Glide(n) => e.glide_on(n, &p, part, (SR, 0.05)),
                Event::Steal(n) => {
                    e.hold();
                    e.note_on(n, &p, part, SR);
                }
                Event::Restart => e.note_on(scene.note, &p, part, SR),
            }
            let mut out = [0.0f32; BLOCK_SIZE];
            e.render(&mut out, &p, SR);
            seen(b, e);
            out
        })
        .collect()
}

/// FNV-1a over the blocks' bits.
pub fn print(blocks: &[[f32; BLOCK_SIZE]]) -> u64 {
    blocks.iter().flatten().fold(0xcbf2_9ce4_8422_2325, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Every scene's print.
pub fn prints() -> Vec<u64> {
    SCENES.iter().map(|s| print(&play(s, |_, _| {}))).collect()
}
