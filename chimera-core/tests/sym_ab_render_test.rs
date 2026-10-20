//! The owner's SYM A/B renderer (sends spec § 3a): the INIT ALGO chord stab at
//! FOLD 0.3, SYM swept 0 -> 1 over the render, to `$SYM_AB_OUT/<name>.wav`
//! (48 kHz 16-bit stereo, peak-normalised to -3 dBFS).
//!
//! `SYM_AB_OUT=dir SYM_AB_NAME=sym_sweep_fold0.3 cargo test -p chimera-core
//!  --test sym_ab_render_test -- --ignored`

mod common;

use chimera_core::modulation::ModState;
use chimera_core::params::EngineType;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::orbit::write_wav;
use common::{Rig, SR, init_params};

#[test]
#[ignore = "renders the owner's A/B; needs SYM_AB_OUT and SYM_AB_NAME"]
fn render_sym_sweep() {
    let (Some(dir), Some(name)) = (
        std::env::var_os("SYM_AB_OUT"),
        std::env::var_os("SYM_AB_NAME"),
    ) else {
        panic!("set SYM_AB_OUT and SYM_AB_NAME");
    };
    let mut p = init_params(EngineType::Algo);
    p.fx.set_type(chimera_core::dsp::voice_fx::params::FxType::Fold);
    p.fx.pots[0] = 0.3;
    let mods = ModState::new();
    let blocks = 3 * SR as usize / BLOCK_SIZE;
    let off = 2 * SR as usize / BLOCK_SIZE;
    let mut rigs: Vec<Rig> = [48u8, 55, 60, 64, 67]
        .iter()
        .map(|&n| {
            let mut r = Rig::new(SR);
            r.note_on(MidiNote::new(n).unwrap(), Velocity::new(100).unwrap(), &p);
            r
        })
        .collect();
    let mut mono = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut block = [0.0f32; BLOCK_SIZE];
    for b in 0..blocks {
        p.fx.pots[1] = b as f32 / (blocks - 1) as f32;
        let mut sum = [0.0f32; BLOCK_SIZE];
        for r in &mut rigs {
            if b == off {
                r.note_off();
            }
            r.render(&mut block, &p, &mods);
            sum.iter_mut().zip(&block).for_each(|(s, x)| *s += x);
        }
        mono.extend_from_slice(&sum);
    }
    let peak = mono.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let g = 10f32.powf(-3.0 / 20.0) / peak;
    let stereo: Vec<f32> = mono.iter().flat_map(|s| [s * g; 2]).collect();
    let rms = (mono.iter().map(|s| (s * g).powi(2)).sum::<f32>() / mono.len() as f32).sqrt();
    println!(
        "raw peak {peak:.3}, normalised RMS {:.1} dBFS",
        20.0 * rms.log10()
    );
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_wav(
        &dir.join(format!("{}.wav", name.to_string_lossy())),
        &stereo,
    );
}
