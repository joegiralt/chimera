//! The reverb through the door (ADR 0142): open, `Reverb::play` crosses it
//! to bank 2's `Reverb::process` and returns its every bit; closed, the
//! reverb's wet is silent and the rest of the FX bus plays on. The host
//! builds its own door (`open_host`). One test: the door is a
//! process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use core::mem::MaybeUninit;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::Stereo;
use chimera_engine::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_engine::dsp::rev::Reverb;
use chimera_engine::dsp::rev::sync::Tempo;
use chimera_engine::dsp::reverb::{ReverbParams, ReverbType};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const BLOCKS: usize = 6 * 80;

fn noise(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
}

fn block(seed: &mut u32) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|_| noise(seed))
}

/// Block `b`'s knobs: every TYPE in turn, so each engine and each switch
/// runs.
fn params(b: usize) -> ReverbParams {
    ReverbParams {
        kind: ReverbType::ALL[b / 80],
        smear: 0.5,
        mix: 0.7,
        ..ReverbParams::DEFAULT
    }
}

fn bits(s: &Stereo) -> Vec<u32> {
    s.l.iter().chain(s.r.iter()).map(|x| x.to_bits()).collect()
}

fn bus() -> Box<FxBus> {
    let mut b = Box::new(MaybeUninit::<FxBus>::uninit());
    FxBus::init_in_place(&mut b);
    // SAFETY: `init_in_place` wrote every field.
    unsafe { Box::from_raw(Box::into_raw(b).cast::<FxBus>()) }
}

/// The bus's returns over `BLOCKS` blocks, every send fed noise, the
/// reverb at `mix`.
fn bus_run(mix: f32) -> Vec<u32> {
    let mut fx = bus();
    let mut seed = 7;
    let mut out = Vec::new();
    for b in 0..BLOCKS {
        let mut p = FxParams::DEFAULT;
        p.reverb = ReverbParams { mix, ..params(b) };
        let mut sends: [[f32; BLOCK_SIZE]; FX_SENDS] = core::array::from_fn(|_| block(&mut seed));
        let mut ret = Stereo::SILENT;
        fx.process(&mut sends, &p, Tempo::DEFAULT, SR, &mut ret);
        out.extend(bits(&ret));
    }
    out
}

#[test]
fn the_reverb_plays_through_the_door_and_is_silent_closed() {
    // Closed: no wet, no panic; the reverb's state is left alone.
    assert!(!DOOR_CELL.is_open());
    let mut r = Reverb::new();
    let mut seed = 1;
    for b in 0..BLOCKS {
        let mut out = Stereo {
            l: [1.0; BLOCK_SIZE],
            r: [1.0; BLOCK_SIZE],
        };
        let p = params(b);
        r.play(&block(&mut seed), &p, p.mix, Tempo::DEFAULT, SR, &mut out);
        assert!(bits(&out).iter().all(|&x| x == 0), "block {b} not silent");
    }
    // The bus closed, every send fed: chorus and delay are bank 2's too
    // (ADR 0212), so nothing returns.
    let closed = bus_run(0.7);
    assert!(closed.iter().all(|&x| x == 0), "the bus returned closed");

    // Open: the door's engine, bit for bit the engines called directly.
    DOOR_CELL.open_host();
    let (mut via, mut direct) = (Reverb::new(), Reverb::new());
    let mut seed = 1;
    let mut heard = false;
    for b in 0..BLOCKS {
        let send = block(&mut seed);
        let p = params(b);
        let (mut a, mut d) = (Stereo::SILENT, Stereo::SILENT);
        via.play(&send, &p, p.mix, Tempo::DEFAULT, SR, &mut a);
        direct.process(&send, &p, p.mix, Tempo::DEFAULT, SR, &mut d);
        assert_eq!(bits(&a), bits(&d), "block {b}");
        heard |= a.l.iter().any(|&x| x != 0.0);
    }
    assert!(heard, "the reverb returned nothing open");
    assert_eq!(via.running(), ReverbType::ALL[5]);
    let dry = bus_run(0.0);
    assert_ne!(dry, closed, "chorus and delay are missing open");
    assert_ne!(bus_run(0.7), dry, "the reverb's wet is missing open");
    DOOR_CELL.close();
}
