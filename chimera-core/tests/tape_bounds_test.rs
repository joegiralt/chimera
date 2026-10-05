//! Review Focus 3: whatever the params, source and `held`, no model reads
//! outside the row (`SourceView::strict` panics on a tap past it) and every
//! head stays within a clamp's step of it.

mod common;

use core::mem::MaybeUninit;

use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape, SourceView};
use chimera_core::dsp::tape_engine::wave;
use chimera_core::dsp::tape_engine::{
    INC_MAX, StretchType, TapeEngine, TapeEngineParams, TapeModel,
};
use chimera_core::storage::twelve::pack12;
use chimera_hal::BLOCK_SIZE;
use common::XorShift;

/// The models these runs play (TRANSWAVE on a `Plain` is silent); Task 8
/// adds STRETCH.
const MODELS: [TapeModel; 5] = [
    TapeModel::Loop,
    TapeModel::Reverse,
    TapeModel::PingPong,
    TapeModel::Transwave,
    TapeModel::Scrub,
];

const MAX_FRAMES: usize = 100_000;

fn unit(rng: &mut XorShift) -> f32 {
    rng.next_u32() as f32 / u32::MAX as f32
}

#[test]
fn reads_stay_in_row() {
    let mut rng = XorShift(0x7A9E_0301);
    let s: Vec<i16> = (0..MAX_FRAMES)
        .map(|_| ((rng.next_u32() & 0xFFF) as i16) << 4 >> 4)
        .collect();
    let mut packed = vec![0u8; 3 * MAX_FRAMES / 2];
    pack12(&s, &mut packed);
    let rates = [4_000u16, 8_000, 24_000, 48_000];
    for run in 0..2_000 {
        let frames = 1 + rng.below(MAX_FRAMES);
        let rate = rates[rng.below(rates.len())];
        let d = Descriptor {
            shape: Shape::Plain,
            count: 1,
            rate,
            head_frames: 0,
        };
        let mut held = d.to_bytes().to_vec();
        held.extend_from_slice(&packed[..3 * frames.div_ceil(2)]);
        // Held cut short, as a head-only hold is: at least one pair.
        let keep = DESC_LEN + 3 + rng.below(held.len() - DESC_LEN - 2);
        held.truncate(keep);
        let v = SourceView::strict(&held).unwrap();
        let p = TapeEngineParams {
            model: MODELS[rng.below(MODELS.len())],
            start: unit(&mut rng),
            end: unit(&mut rng),
            position: unit(&mut rng),
            motion: 2.0 * unit(&mut rng) - 1.0,
            tape: unit(&mut rng),
            age: unit(&mut rng),
            size: unit(&mut rng),
            xfade: 0.5 * unit(&mut rng),
            drag: unit(&mut rng),
            kind: [StretchType::Mono, StretchType::Poly][rng.below(2)],
            root: 12_700.0 * unit(&mut rng),
            period: 1_000.0 * unit(&mut rng),
        };
        let mut slot = MaybeUninit::uninit();
        let e = TapeEngine::init_in_place(&mut slot);
        e.set_pitch(4_800.0 * unit(&mut rng) - 2_400.0);
        e.note_on(rng.below(128) as u8, &p, &v, 48_000);
        let mut b = [0.0; BLOCK_SIZE];
        let reach = v.frames() + INC_MAX as u32;
        assert!(p.model != TapeModel::Transwave || b.iter().all(|&y| y == 0.0));
        for _ in 0..4 {
            e.render(&mut b, &p, &v, 48_000);
            assert!(b.iter().all(|y| y.is_finite()), "run {run}: {p:?}");
            let pr = e.probe();
            assert!(
                pr.head <= reach,
                "run {run}: head {} of {}",
                pr.head,
                v.frames()
            );
            assert!(pr.other.is_none_or(|o| o <= reach), "run {run}: {pr:?}");
        }
    }
}

/// TRANSWAVE on random transwaves, whole or cut short (then silent): every
/// frame and level it reads lies inside the row (`read_cycle` panics on a
/// frame past it when strict), and the output stays finite.
#[test]
fn transwave_reads_stay_in_row() {
    let mut rng = XorShift(0x7A9E_0702);
    for run in 0..300 {
        let top = [512u32, 1_024][rng.below(2)];
        let count = 1 + rng.below(32) as u8;
        let n = count as usize * wave::frame_len(top) as usize;
        let s: Vec<i16> = (0..n)
            .map(|_| ((rng.next_u32() & 0xFFF) as i16) << 4 >> 4)
            .collect();
        let d = Descriptor {
            shape: Shape::Transwave,
            count,
            rate: 48_000,
            head_frames: 0,
        };
        let mut held = d.to_bytes().to_vec();
        held.resize(DESC_LEN + 3 * n / 2, 0);
        pack12(&s, &mut held[DESC_LEN..]);
        if rng.below(4) == 0 {
            let keep = DESC_LEN + 3 + rng.below(held.len() - DESC_LEN - 3);
            held.truncate(keep);
        }
        let v = SourceView::strict(&held).unwrap();
        let p = TapeEngineParams {
            model: TapeModel::Transwave,
            position: unit(&mut rng),
            motion: 2.0 * unit(&mut rng) - 1.0,
            size: unit(&mut rng),
            ..TapeEngineParams::default()
        };
        let mut slot = MaybeUninit::uninit();
        let e = TapeEngine::init_in_place(&mut slot);
        e.set_pitch(4_800.0 * unit(&mut rng) - 2_400.0);
        e.note_on(rng.below(128) as u8, &p, &v, 48_000);
        let mut b = [0.0; BLOCK_SIZE];
        for k in 0..8 {
            // A pitch route anywhere in ±24 st, block by block.
            e.set_pitch(4_800.0 * unit(&mut rng) - 2_400.0);
            e.render(&mut b, &p, &v, 48_000);
            assert!(b.iter().all(|y| y.is_finite()), "run {run}, block {k}");
            let pr = e.probe();
            assert!(pr.head < count as u32, "run {run}: {pr:?}");
            assert!(
                pr.other.is_some_and(|o| o < count as u32),
                "run {run}: {pr:?}"
            );
        }
    }
}
