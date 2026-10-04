//! The owner's A/B renderer (reverb TYPEs spec § A/B): one stimulus from
//! `tools/ab/stimuli.py` through the real bus (`FxBus`), QUAD at its
//! defaults but GRIT, MIX 1, dry off. `tools/ab/render.sh` drives it.
//!
//! `AB_STIMULUS` names `$AB_DIR/stimuli/<name>.wav`; `AB_GRIT` is GRIT;
//! `AB_DIR` defaults to the workspace's `target/ab`. Writes
//! `<name>_g<grit>.wav` there, 6 dB down, and `<name>_g<grit>.txt`: the
//! GRIT's noise floor and correlated-product figure (`reverb_grit_test`'s
//! measures, through the same bus), the render's peak and its last 2 s, in
//! dBFS before the 6 dB.

mod common;

use std::path::PathBuf;

use chimera_core::dsp::Stereo;
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::orbit::write_wav;
use common::rev::{burst_products_db, sine_floor_dbfs};

/// A fresh bus with only the reverb on, QUAD's defaults at `grit`, MIX 1.
fn bus(grit: f32) -> (Box<FxBus>, FxParams) {
    let mut fx = FxParams::default();
    (fx.reverb.grit, fx.reverb.mix) = (grit, 1.0);
    (Box::new(FxBus::new()), fx)
}

fn tick(
    bus: &mut FxBus,
    fx: &FxParams,
    x: &[f32; BLOCK_SIZE],
) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]) {
    let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
    sends[2] = *x;
    let mut ret = Stereo::SILENT;
    bus.process(&mut sends, fx, Tempo::DEFAULT, SR, &mut ret);
    (ret.l, ret.r)
}

/// A mono 32-bit float WAV's samples (`stimuli.py`'s format).
fn read_wav(path: &std::path::Path) -> Vec<f32> {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(&b[..4], b"RIFF");
    assert_eq!(u16::from_le_bytes([b[20], b[21]]), 3, "not float");
    assert_eq!(u16::from_le_bytes([b[22], b[23]]), 1, "not mono");
    let at = b.windows(4).position(|w| w == b"data").expect("no data") + 8;
    b[at..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&c| f32::from_le_bytes(c))
        .collect()
}

fn dbfs(x: &[f32]) -> f32 {
    let ms = x.iter().map(|&s| s as f64 * s as f64).sum::<f64>() / x.len() as f64;
    (10.0 * (ms / 0.5).log10()) as f32
}

#[test]
#[ignore]
fn render() {
    let name = std::env::var("AB_STIMULUS").expect("AB_STIMULUS");
    let grit: f32 = std::env::var("AB_GRIT")
        .expect("AB_GRIT")
        .parse()
        .expect("GRIT");
    let dir = std::env::var_os("AB_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/ab"),
        PathBuf::from,
    );
    let x = read_wav(&dir.join("stimuli").join(format!("{name}.wav")));
    let (mut b, fx) = bus(grit);
    let mut out = Vec::with_capacity(2 * x.len());
    for blk in x.chunks(BLOCK_SIZE) {
        let mut send = [0.0; BLOCK_SIZE];
        send[..blk.len()].copy_from_slice(blk);
        let (l, r) = tick(&mut b, &fx, &send);
        out.extend(l.iter().zip(&r).flat_map(|(&l, &r)| [l, r]));
    }
    assert!(out.iter().all(|s| s.is_finite()), "non-finite");
    let stem = format!("{name}_g{grit}");
    let wav = dir.join(format!("{stem}.wav"));
    // 6 dB down: at MIX 1 the return peaks at 0 dBFS, and the WAV clips.
    write_wav(&wav, &out.iter().map(|&s| 0.5 * s).collect::<Vec<_>>());
    assert!(wav.exists());

    let floor = {
        let (mut b, fx) = bus(grit);
        sine_floor_dbfs(&mut |s| tick(&mut b, &fx, s))
    };
    let (products, worst) = {
        let (mut b, fx) = bus(grit);
        burst_products_db(&mut |s| tick(&mut b, &fx, s))
    };
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let last = dbfs(&out[out.len() - 4 * SR as usize..]);
    std::fs::write(
        dir.join(format!("{stem}.txt")),
        format!(
            "{name}\t{grit}\t{floor:.1}\t{products:.1}\t{worst:.1}\t{:.1}\t{last:.1}\n",
            20.0 * peak.log10()
        ),
    )
    .expect("the figures");
}
