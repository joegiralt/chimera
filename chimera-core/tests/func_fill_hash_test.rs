//! Envelope B's block fill (#793), bit for bit as main gave it (bc20ae7c):
//! ENV and BURST in every FORM over SHAPE (TILT) from 0 to 1, fast and slow
//! slides, the key held then let go; and the per-sample `tick` beside it.

use chimera_core::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use chimera_core::dsp::modulator::{EnvForm, Func, FuncParams, LfoForm};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn fnv(h: &mut u64, x: f32) {
    for byte in x.to_bits().to_le_bytes() {
        *h = (*h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
}

/// 300 blocks of `fill` (or `tick`) from a note-on, the key up at 120.
fn hash(p: &FuncParams, per_sample: bool) -> u64 {
    let c = BCoefs::new(p, &Slides::default(), SR, true);
    let mut g = FuncGen::new();
    g.set(&c);
    g.note_on(p.func());
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut b = [0.0f32; BLOCK_SIZE];
    for n in 0..300 {
        let key = n < 120;
        if n == 120 {
            g.key_up();
        }
        g.set(&c);
        if per_sample {
            for o in b.iter_mut() {
                *o = g.tick(&c, key);
            }
        } else {
            g.fill(&c, key, &mut b);
        }
        b.iter().for_each(|&x| fnv(&mut h, x));
    }
    h
}

fn hashes() -> Vec<(String, u64)> {
    let mut v = Vec::new();
    let funcs = [
        Func::Env(EnvForm::Ad),
        Func::Env(EnvForm::Ahr),
        Func::Env(EnvForm::Cycle),
        Func::Burst(EnvForm::Ad),
        Func::Burst(EnvForm::Ahr),
        Func::Burst(EnvForm::Cycle),
        Func::Lfo(LfoForm::Free),
    ];
    for f in funcs {
        for (rise, fall) in [(0.05, 0.1), (0.2, 0.5), (0.6, 0.3)] {
            for shape in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
                let mut p = FuncParams {
                    rise,
                    fall,
                    shape,
                    ..FuncParams::ENV
                };
                p.set_func(f);
                for per_sample in [false, true] {
                    v.push((
                        format!("{f:?} r{rise} f{fall} s{shape} {per_sample}"),
                        hash(&p, per_sample),
                    ));
                }
            }
        }
    }
    v
}

/// Taken on main (bc20ae7c).
const BEFORE: &[(&str, u64)] = &[
    ("Env(Ad) r0.05 f0.1 s0 false", 0x7f9d721f42a03060),
    ("Env(Ad) r0.05 f0.1 s0 true", 0x7f9d721f42a03060),
    ("Env(Ad) r0.05 f0.1 s0.1 false", 0xf042368f8b62a537),
    ("Env(Ad) r0.05 f0.1 s0.1 true", 0xf042368f8b62a537),
    ("Env(Ad) r0.05 f0.1 s0.25 false", 0x286aaa58a2996c35),
    ("Env(Ad) r0.05 f0.1 s0.25 true", 0x286aaa58a2996c35),
    ("Env(Ad) r0.05 f0.1 s0.5 false", 0xd3aed6100dfbf35f),
    ("Env(Ad) r0.05 f0.1 s0.5 true", 0xd3aed6100dfbf35f),
    ("Env(Ad) r0.05 f0.1 s0.75 false", 0x34015cfe07c75edb),
    ("Env(Ad) r0.05 f0.1 s0.75 true", 0x34015cfe07c75edb),
    ("Env(Ad) r0.05 f0.1 s0.9 false", 0xc8b4e1be2b6a1f4b),
    ("Env(Ad) r0.05 f0.1 s0.9 true", 0xc8b4e1be2b6a1f4b),
    ("Env(Ad) r0.05 f0.1 s1 false", 0x722aa0f34fde03bd),
    ("Env(Ad) r0.05 f0.1 s1 true", 0x722aa0f34fde03bd),
    ("Env(Ad) r0.2 f0.5 s0 false", 0x2daea75bbc23df30),
    ("Env(Ad) r0.2 f0.5 s0 true", 0x2daea75bbc23df30),
    ("Env(Ad) r0.2 f0.5 s0.1 false", 0x66e21c282cd56606),
    ("Env(Ad) r0.2 f0.5 s0.1 true", 0x66e21c282cd56606),
    ("Env(Ad) r0.2 f0.5 s0.25 false", 0x4e4bef037848672b),
    ("Env(Ad) r0.2 f0.5 s0.25 true", 0x4e4bef037848672b),
    ("Env(Ad) r0.2 f0.5 s0.5 false", 0xf9932ad21c3756bc),
    ("Env(Ad) r0.2 f0.5 s0.5 true", 0xf9932ad21c3756bc),
    ("Env(Ad) r0.2 f0.5 s0.75 false", 0x12dba41ea5522c98),
    ("Env(Ad) r0.2 f0.5 s0.75 true", 0x12dba41ea5522c98),
    ("Env(Ad) r0.2 f0.5 s0.9 false", 0xea4ba94a52499e65),
    ("Env(Ad) r0.2 f0.5 s0.9 true", 0xea4ba94a52499e65),
    ("Env(Ad) r0.2 f0.5 s1 false", 0x89b511fe44f1790e),
    ("Env(Ad) r0.2 f0.5 s1 true", 0x89b511fe44f1790e),
    ("Env(Ad) r0.6 f0.3 s0 false", 0xdbd4854686fdcc3d),
    ("Env(Ad) r0.6 f0.3 s0 true", 0xdbd4854686fdcc3d),
    ("Env(Ad) r0.6 f0.3 s0.1 false", 0xcfd7e7bbd4bf47af),
    ("Env(Ad) r0.6 f0.3 s0.1 true", 0xcfd7e7bbd4bf47af),
    ("Env(Ad) r0.6 f0.3 s0.25 false", 0xbc97532e5f974040),
    ("Env(Ad) r0.6 f0.3 s0.25 true", 0xbc97532e5f974040),
    ("Env(Ad) r0.6 f0.3 s0.5 false", 0x90e55fc48f8c5def),
    ("Env(Ad) r0.6 f0.3 s0.5 true", 0x90e55fc48f8c5def),
    ("Env(Ad) r0.6 f0.3 s0.75 false", 0x37d74670ffff00cb),
    ("Env(Ad) r0.6 f0.3 s0.75 true", 0x37d74670ffff00cb),
    ("Env(Ad) r0.6 f0.3 s0.9 false", 0xd904b68949c0c4da),
    ("Env(Ad) r0.6 f0.3 s0.9 true", 0xd904b68949c0c4da),
    ("Env(Ad) r0.6 f0.3 s1 false", 0x2b02303cce072844),
    ("Env(Ad) r0.6 f0.3 s1 true", 0x2b02303cce072844),
    ("Env(Ahr) r0.05 f0.1 s0 false", 0xe253f5578f2ca806),
    ("Env(Ahr) r0.05 f0.1 s0 true", 0xe253f5578f2ca806),
    ("Env(Ahr) r0.05 f0.1 s0.1 false", 0x8bcbefcba48db71f),
    ("Env(Ahr) r0.05 f0.1 s0.1 true", 0x8bcbefcba48db71f),
    ("Env(Ahr) r0.05 f0.1 s0.25 false", 0x3a8131a327ecbb44),
    ("Env(Ahr) r0.05 f0.1 s0.25 true", 0x3a8131a327ecbb44),
    ("Env(Ahr) r0.05 f0.1 s0.5 false", 0xb370f13d80082687),
    ("Env(Ahr) r0.05 f0.1 s0.5 true", 0xb370f13d80082687),
    ("Env(Ahr) r0.05 f0.1 s0.75 false", 0xea3bd68ccc56b838),
    ("Env(Ahr) r0.05 f0.1 s0.75 true", 0xea3bd68ccc56b838),
    ("Env(Ahr) r0.05 f0.1 s0.9 false", 0x479e717e93564510),
    ("Env(Ahr) r0.05 f0.1 s0.9 true", 0x479e717e93564510),
    ("Env(Ahr) r0.05 f0.1 s1 false", 0xf584d5d974ab3fff),
    ("Env(Ahr) r0.05 f0.1 s1 true", 0xf584d5d974ab3fff),
    ("Env(Ahr) r0.2 f0.5 s0 false", 0xb75dba5104cb95af),
    ("Env(Ahr) r0.2 f0.5 s0 true", 0xb75dba5104cb95af),
    ("Env(Ahr) r0.2 f0.5 s0.1 false", 0xefa32e2ee67b44f9),
    ("Env(Ahr) r0.2 f0.5 s0.1 true", 0xefa32e2ee67b44f9),
    ("Env(Ahr) r0.2 f0.5 s0.25 false", 0xedb054959a95e418),
    ("Env(Ahr) r0.2 f0.5 s0.25 true", 0xedb054959a95e418),
    ("Env(Ahr) r0.2 f0.5 s0.5 false", 0xce8fd7cd570943b7),
    ("Env(Ahr) r0.2 f0.5 s0.5 true", 0xce8fd7cd570943b7),
    ("Env(Ahr) r0.2 f0.5 s0.75 false", 0x594c459240960e5a),
    ("Env(Ahr) r0.2 f0.5 s0.75 true", 0x594c459240960e5a),
    ("Env(Ahr) r0.2 f0.5 s0.9 false", 0x92e7fbfa65f0231e),
    ("Env(Ahr) r0.2 f0.5 s0.9 true", 0x92e7fbfa65f0231e),
    ("Env(Ahr) r0.2 f0.5 s1 false", 0x46fe42dbd50d3340),
    ("Env(Ahr) r0.2 f0.5 s1 true", 0x46fe42dbd50d3340),
    ("Env(Ahr) r0.6 f0.3 s0 false", 0xebace3f00294d039),
    ("Env(Ahr) r0.6 f0.3 s0 true", 0xebace3f00294d039),
    ("Env(Ahr) r0.6 f0.3 s0.1 false", 0x5301ce5bc1b3055f),
    ("Env(Ahr) r0.6 f0.3 s0.1 true", 0x5301ce5bc1b3055f),
    ("Env(Ahr) r0.6 f0.3 s0.25 false", 0xbb3e48b9fbda866c),
    ("Env(Ahr) r0.6 f0.3 s0.25 true", 0xbb3e48b9fbda866c),
    ("Env(Ahr) r0.6 f0.3 s0.5 false", 0xf5a7f5681093f63a),
    ("Env(Ahr) r0.6 f0.3 s0.5 true", 0xf5a7f5681093f63a),
    ("Env(Ahr) r0.6 f0.3 s0.75 false", 0xfc4374e3b43f0a9a),
    ("Env(Ahr) r0.6 f0.3 s0.75 true", 0xfc4374e3b43f0a9a),
    ("Env(Ahr) r0.6 f0.3 s0.9 false", 0xcd81251c545f8ed7),
    ("Env(Ahr) r0.6 f0.3 s0.9 true", 0xcd81251c545f8ed7),
    ("Env(Ahr) r0.6 f0.3 s1 false", 0x2dfe9aba863fc691),
    ("Env(Ahr) r0.6 f0.3 s1 true", 0x2dfe9aba863fc691),
    ("Env(Cycle) r0.05 f0.1 s0 false", 0xc7511aebd2558895),
    ("Env(Cycle) r0.05 f0.1 s0 true", 0xc7511aebd2558895),
    ("Env(Cycle) r0.05 f0.1 s0.1 false", 0x3647fbd1bc0c26f5),
    ("Env(Cycle) r0.05 f0.1 s0.1 true", 0x3647fbd1bc0c26f5),
    ("Env(Cycle) r0.05 f0.1 s0.25 false", 0xd2238f574cf681cf),
    ("Env(Cycle) r0.05 f0.1 s0.25 true", 0xd2238f574cf681cf),
    ("Env(Cycle) r0.05 f0.1 s0.5 false", 0xb645cb99941ff4f4),
    ("Env(Cycle) r0.05 f0.1 s0.5 true", 0xb645cb99941ff4f4),
    ("Env(Cycle) r0.05 f0.1 s0.75 false", 0x4155dd8e6a919378),
    ("Env(Cycle) r0.05 f0.1 s0.75 true", 0x4155dd8e6a919378),
    ("Env(Cycle) r0.05 f0.1 s0.9 false", 0x1332c8e672dc98e4),
    ("Env(Cycle) r0.05 f0.1 s0.9 true", 0x1332c8e672dc98e4),
    ("Env(Cycle) r0.05 f0.1 s1 false", 0x14503dd5b6f256bb),
    ("Env(Cycle) r0.05 f0.1 s1 true", 0x14503dd5b6f256bb),
    ("Env(Cycle) r0.2 f0.5 s0 false", 0x0be5122e7fa681b1),
    ("Env(Cycle) r0.2 f0.5 s0 true", 0x0be5122e7fa681b1),
    ("Env(Cycle) r0.2 f0.5 s0.1 false", 0xbf876e6b54059b86),
    ("Env(Cycle) r0.2 f0.5 s0.1 true", 0xbf876e6b54059b86),
    ("Env(Cycle) r0.2 f0.5 s0.25 false", 0x4327d9abeb2ab280),
    ("Env(Cycle) r0.2 f0.5 s0.25 true", 0x4327d9abeb2ab280),
    ("Env(Cycle) r0.2 f0.5 s0.5 false", 0xe6633f8f2cded86d),
    ("Env(Cycle) r0.2 f0.5 s0.5 true", 0xe6633f8f2cded86d),
    ("Env(Cycle) r0.2 f0.5 s0.75 false", 0x46f983321a97b31e),
    ("Env(Cycle) r0.2 f0.5 s0.75 true", 0x46f983321a97b31e),
    ("Env(Cycle) r0.2 f0.5 s0.9 false", 0xc9afc1cbbe3b81ab),
    ("Env(Cycle) r0.2 f0.5 s0.9 true", 0xc9afc1cbbe3b81ab),
    ("Env(Cycle) r0.2 f0.5 s1 false", 0x15af7a680e2988b9),
    ("Env(Cycle) r0.2 f0.5 s1 true", 0x15af7a680e2988b9),
    ("Env(Cycle) r0.6 f0.3 s0 false", 0xc453c542e9f5fe85),
    ("Env(Cycle) r0.6 f0.3 s0 true", 0xc453c542e9f5fe85),
    ("Env(Cycle) r0.6 f0.3 s0.1 false", 0x817e00d910930c12),
    ("Env(Cycle) r0.6 f0.3 s0.1 true", 0x817e00d910930c12),
    ("Env(Cycle) r0.6 f0.3 s0.25 false", 0x36bed7ee54fcf6cd),
    ("Env(Cycle) r0.6 f0.3 s0.25 true", 0x36bed7ee54fcf6cd),
    ("Env(Cycle) r0.6 f0.3 s0.5 false", 0xfee0c83b01e06984),
    ("Env(Cycle) r0.6 f0.3 s0.5 true", 0xfee0c83b01e06984),
    ("Env(Cycle) r0.6 f0.3 s0.75 false", 0x6d3b10c7e40ea8a2),
    ("Env(Cycle) r0.6 f0.3 s0.75 true", 0x6d3b10c7e40ea8a2),
    ("Env(Cycle) r0.6 f0.3 s0.9 false", 0xe8ccc66fecaf6cab),
    ("Env(Cycle) r0.6 f0.3 s0.9 true", 0xe8ccc66fecaf6cab),
    ("Env(Cycle) r0.6 f0.3 s1 false", 0x62afe7ece178d4af),
    ("Env(Cycle) r0.6 f0.3 s1 true", 0x62afe7ece178d4af),
    ("Burst(Ad) r0.05 f0.1 s0 false", 0xe7f49fa72d392793),
    ("Burst(Ad) r0.05 f0.1 s0 true", 0xe7f49fa72d392793),
    ("Burst(Ad) r0.05 f0.1 s0.1 false", 0x4f5b3635c36b9f74),
    ("Burst(Ad) r0.05 f0.1 s0.1 true", 0x4f5b3635c36b9f74),
    ("Burst(Ad) r0.05 f0.1 s0.25 false", 0x6e48a9bf6e232160),
    ("Burst(Ad) r0.05 f0.1 s0.25 true", 0x6e48a9bf6e232160),
    ("Burst(Ad) r0.05 f0.1 s0.5 false", 0xac97fa334bbd8b86),
    ("Burst(Ad) r0.05 f0.1 s0.5 true", 0xac97fa334bbd8b86),
    ("Burst(Ad) r0.05 f0.1 s0.75 false", 0x2ec0603ea5483431),
    ("Burst(Ad) r0.05 f0.1 s0.75 true", 0x2ec0603ea5483431),
    ("Burst(Ad) r0.05 f0.1 s0.9 false", 0x5b611358cc098184),
    ("Burst(Ad) r0.05 f0.1 s0.9 true", 0x5b611358cc098184),
    ("Burst(Ad) r0.05 f0.1 s1 false", 0x1a847646d680d31c),
    ("Burst(Ad) r0.05 f0.1 s1 true", 0x1a847646d680d31c),
    ("Burst(Ad) r0.2 f0.5 s0 false", 0x8ee9fa86282c2fcb),
    ("Burst(Ad) r0.2 f0.5 s0 true", 0x8ee9fa86282c2fcb),
    ("Burst(Ad) r0.2 f0.5 s0.1 false", 0x814a7300b79964ec),
    ("Burst(Ad) r0.2 f0.5 s0.1 true", 0x814a7300b79964ec),
    ("Burst(Ad) r0.2 f0.5 s0.25 false", 0x683d057f11fbef7f),
    ("Burst(Ad) r0.2 f0.5 s0.25 true", 0x683d057f11fbef7f),
    ("Burst(Ad) r0.2 f0.5 s0.5 false", 0xa35c0681ad249da4),
    ("Burst(Ad) r0.2 f0.5 s0.5 true", 0xa35c0681ad249da4),
    ("Burst(Ad) r0.2 f0.5 s0.75 false", 0x0ecca38a636f98f0),
    ("Burst(Ad) r0.2 f0.5 s0.75 true", 0x0ecca38a636f98f0),
    ("Burst(Ad) r0.2 f0.5 s0.9 false", 0xdde97042a9fe2d12),
    ("Burst(Ad) r0.2 f0.5 s0.9 true", 0xdde97042a9fe2d12),
    ("Burst(Ad) r0.2 f0.5 s1 false", 0x6d761eca4b3cbf75),
    ("Burst(Ad) r0.2 f0.5 s1 true", 0x6d761eca4b3cbf75),
    ("Burst(Ad) r0.6 f0.3 s0 false", 0x092239e4249880bb),
    ("Burst(Ad) r0.6 f0.3 s0 true", 0x092239e4249880bb),
    ("Burst(Ad) r0.6 f0.3 s0.1 false", 0xffbf9656ee045ad1),
    ("Burst(Ad) r0.6 f0.3 s0.1 true", 0xffbf9656ee045ad1),
    ("Burst(Ad) r0.6 f0.3 s0.25 false", 0xcabb3cda36afcd81),
    ("Burst(Ad) r0.6 f0.3 s0.25 true", 0xcabb3cda36afcd81),
    ("Burst(Ad) r0.6 f0.3 s0.5 false", 0xd28ac8c060c11170),
    ("Burst(Ad) r0.6 f0.3 s0.5 true", 0xd28ac8c060c11170),
    ("Burst(Ad) r0.6 f0.3 s0.75 false", 0xbfa5c5e6b778c825),
    ("Burst(Ad) r0.6 f0.3 s0.75 true", 0xbfa5c5e6b778c825),
    ("Burst(Ad) r0.6 f0.3 s0.9 false", 0x1883f4dc33468675),
    ("Burst(Ad) r0.6 f0.3 s0.9 true", 0x1883f4dc33468675),
    ("Burst(Ad) r0.6 f0.3 s1 false", 0x81d6d63a72d6753e),
    ("Burst(Ad) r0.6 f0.3 s1 true", 0x81d6d63a72d6753e),
    ("Burst(Ahr) r0.05 f0.1 s0 false", 0x33fefa6e09383793),
    ("Burst(Ahr) r0.05 f0.1 s0 true", 0x33fefa6e09383793),
    ("Burst(Ahr) r0.05 f0.1 s0.1 false", 0x2238104bc9b8b4af),
    ("Burst(Ahr) r0.05 f0.1 s0.1 true", 0x2238104bc9b8b4af),
    ("Burst(Ahr) r0.05 f0.1 s0.25 false", 0x8e20e6b6440c9d90),
    ("Burst(Ahr) r0.05 f0.1 s0.25 true", 0x8e20e6b6440c9d90),
    ("Burst(Ahr) r0.05 f0.1 s0.5 false", 0x34fa051d60950987),
    ("Burst(Ahr) r0.05 f0.1 s0.5 true", 0x34fa051d60950987),
    ("Burst(Ahr) r0.05 f0.1 s0.75 false", 0x0dd5e07f251fe28a),
    ("Burst(Ahr) r0.05 f0.1 s0.75 true", 0x0dd5e07f251fe28a),
    ("Burst(Ahr) r0.05 f0.1 s0.9 false", 0x40cc75e02db6cf2e),
    ("Burst(Ahr) r0.05 f0.1 s0.9 true", 0x40cc75e02db6cf2e),
    ("Burst(Ahr) r0.05 f0.1 s1 false", 0x4ec94e14ce9488ec),
    ("Burst(Ahr) r0.05 f0.1 s1 true", 0x4ec94e14ce9488ec),
    ("Burst(Ahr) r0.2 f0.5 s0 false", 0xfa86943510341bb5),
    ("Burst(Ahr) r0.2 f0.5 s0 true", 0xfa86943510341bb5),
    ("Burst(Ahr) r0.2 f0.5 s0.1 false", 0x2bdf7fa074876cef),
    ("Burst(Ahr) r0.2 f0.5 s0.1 true", 0x2bdf7fa074876cef),
    ("Burst(Ahr) r0.2 f0.5 s0.25 false", 0x64a91ffeba218046),
    ("Burst(Ahr) r0.2 f0.5 s0.25 true", 0x64a91ffeba218046),
    ("Burst(Ahr) r0.2 f0.5 s0.5 false", 0xa35c0681ad249da4),
    ("Burst(Ahr) r0.2 f0.5 s0.5 true", 0xa35c0681ad249da4),
    ("Burst(Ahr) r0.2 f0.5 s0.75 false", 0x0ecca38a636f98f0),
    ("Burst(Ahr) r0.2 f0.5 s0.75 true", 0x0ecca38a636f98f0),
    ("Burst(Ahr) r0.2 f0.5 s0.9 false", 0xdde97042a9fe2d12),
    ("Burst(Ahr) r0.2 f0.5 s0.9 true", 0xdde97042a9fe2d12),
    ("Burst(Ahr) r0.2 f0.5 s1 false", 0x6d761eca4b3cbf75),
    ("Burst(Ahr) r0.2 f0.5 s1 true", 0x6d761eca4b3cbf75),
    ("Burst(Ahr) r0.6 f0.3 s0 false", 0x7b9853225fccd58a),
    ("Burst(Ahr) r0.6 f0.3 s0 true", 0x7b9853225fccd58a),
    ("Burst(Ahr) r0.6 f0.3 s0.1 false", 0x303f7e93fa1632e2),
    ("Burst(Ahr) r0.6 f0.3 s0.1 true", 0x303f7e93fa1632e2),
    ("Burst(Ahr) r0.6 f0.3 s0.25 false", 0xbd0f414a40245b00),
    ("Burst(Ahr) r0.6 f0.3 s0.25 true", 0xbd0f414a40245b00),
    ("Burst(Ahr) r0.6 f0.3 s0.5 false", 0x9f38a7a04ce56d4d),
    ("Burst(Ahr) r0.6 f0.3 s0.5 true", 0x9f38a7a04ce56d4d),
    ("Burst(Ahr) r0.6 f0.3 s0.75 false", 0xf620cdcc3b883fed),
    ("Burst(Ahr) r0.6 f0.3 s0.75 true", 0xf620cdcc3b883fed),
    ("Burst(Ahr) r0.6 f0.3 s0.9 false", 0x38cc654ab52748ad),
    ("Burst(Ahr) r0.6 f0.3 s0.9 true", 0x38cc654ab52748ad),
    ("Burst(Ahr) r0.6 f0.3 s1 false", 0x643e2df8e14c09df),
    ("Burst(Ahr) r0.6 f0.3 s1 true", 0x643e2df8e14c09df),
    ("Burst(Cycle) r0.05 f0.1 s0 false", 0xfdb5c2033db6930a),
    ("Burst(Cycle) r0.05 f0.1 s0 true", 0xfdb5c2033db6930a),
    ("Burst(Cycle) r0.05 f0.1 s0.1 false", 0xe168a62dae789eb3),
    ("Burst(Cycle) r0.05 f0.1 s0.1 true", 0xe168a62dae789eb3),
    ("Burst(Cycle) r0.05 f0.1 s0.25 false", 0xf688a2a3dabc65f5),
    ("Burst(Cycle) r0.05 f0.1 s0.25 true", 0xf688a2a3dabc65f5),
    ("Burst(Cycle) r0.05 f0.1 s0.5 false", 0x60d1771e329b4128),
    ("Burst(Cycle) r0.05 f0.1 s0.5 true", 0x60d1771e329b4128),
    ("Burst(Cycle) r0.05 f0.1 s0.75 false", 0xdce8b872e34dafa9),
    ("Burst(Cycle) r0.05 f0.1 s0.75 true", 0xdce8b872e34dafa9),
    ("Burst(Cycle) r0.05 f0.1 s0.9 false", 0xf7dbc4e62d44cdfa),
    ("Burst(Cycle) r0.05 f0.1 s0.9 true", 0xf7dbc4e62d44cdfa),
    ("Burst(Cycle) r0.05 f0.1 s1 false", 0xece775043157772e),
    ("Burst(Cycle) r0.05 f0.1 s1 true", 0xece775043157772e),
    ("Burst(Cycle) r0.2 f0.5 s0 false", 0xbdb22591609ac39d),
    ("Burst(Cycle) r0.2 f0.5 s0 true", 0xbdb22591609ac39d),
    ("Burst(Cycle) r0.2 f0.5 s0.1 false", 0xc8c4b17734ed2d49),
    ("Burst(Cycle) r0.2 f0.5 s0.1 true", 0xc8c4b17734ed2d49),
    ("Burst(Cycle) r0.2 f0.5 s0.25 false", 0xabd08ae45546f552),
    ("Burst(Cycle) r0.2 f0.5 s0.25 true", 0xabd08ae45546f552),
    ("Burst(Cycle) r0.2 f0.5 s0.5 false", 0x79fb54e77255ece4),
    ("Burst(Cycle) r0.2 f0.5 s0.5 true", 0x79fb54e77255ece4),
    ("Burst(Cycle) r0.2 f0.5 s0.75 false", 0x95680ee2c096f044),
    ("Burst(Cycle) r0.2 f0.5 s0.75 true", 0x95680ee2c096f044),
    ("Burst(Cycle) r0.2 f0.5 s0.9 false", 0x72bbf4a368793120),
    ("Burst(Cycle) r0.2 f0.5 s0.9 true", 0x72bbf4a368793120),
    ("Burst(Cycle) r0.2 f0.5 s1 false", 0x05fcddfc3e498fe9),
    ("Burst(Cycle) r0.2 f0.5 s1 true", 0x05fcddfc3e498fe9),
    ("Burst(Cycle) r0.6 f0.3 s0 false", 0xa082562b18ee67c1),
    ("Burst(Cycle) r0.6 f0.3 s0 true", 0xa082562b18ee67c1),
    ("Burst(Cycle) r0.6 f0.3 s0.1 false", 0x10ff804400e1bf72),
    ("Burst(Cycle) r0.6 f0.3 s0.1 true", 0x10ff804400e1bf72),
    ("Burst(Cycle) r0.6 f0.3 s0.25 false", 0x500a48c7542d31fa),
    ("Burst(Cycle) r0.6 f0.3 s0.25 true", 0x500a48c7542d31fa),
    ("Burst(Cycle) r0.6 f0.3 s0.5 false", 0x30c291472da4589f),
    ("Burst(Cycle) r0.6 f0.3 s0.5 true", 0x30c291472da4589f),
    ("Burst(Cycle) r0.6 f0.3 s0.75 false", 0x044d7ea0d2fff3ad),
    ("Burst(Cycle) r0.6 f0.3 s0.75 true", 0x044d7ea0d2fff3ad),
    ("Burst(Cycle) r0.6 f0.3 s0.9 false", 0x10010b2b71267c4d),
    ("Burst(Cycle) r0.6 f0.3 s0.9 true", 0x10010b2b71267c4d),
    ("Burst(Cycle) r0.6 f0.3 s1 false", 0x8548a1f1124e4d0e),
    ("Burst(Cycle) r0.6 f0.3 s1 true", 0x8548a1f1124e4d0e),
    ("Lfo(Free) r0.05 f0.1 s0 false", 0x30717b153406f033),
    ("Lfo(Free) r0.05 f0.1 s0 true", 0x30717b153406f033),
    ("Lfo(Free) r0.05 f0.1 s0.1 false", 0x274de94c625a31d6),
    ("Lfo(Free) r0.05 f0.1 s0.1 true", 0x274de94c625a31d6),
    ("Lfo(Free) r0.05 f0.1 s0.25 false", 0x5e3806e40726c878),
    ("Lfo(Free) r0.05 f0.1 s0.25 true", 0x5e3806e40726c878),
    ("Lfo(Free) r0.05 f0.1 s0.5 false", 0x8b9b173dfb0e81ac),
    ("Lfo(Free) r0.05 f0.1 s0.5 true", 0x8b9b173dfb0e81ac),
    ("Lfo(Free) r0.05 f0.1 s0.75 false", 0xc011c6b6e9cf0a11),
    ("Lfo(Free) r0.05 f0.1 s0.75 true", 0xc011c6b6e9cf0a11),
    ("Lfo(Free) r0.05 f0.1 s0.9 false", 0xadfab9050067ecbe),
    ("Lfo(Free) r0.05 f0.1 s0.9 true", 0xadfab9050067ecbe),
    ("Lfo(Free) r0.05 f0.1 s1 false", 0xa3c05067fa6aac93),
    ("Lfo(Free) r0.05 f0.1 s1 true", 0xa3c05067fa6aac93),
    ("Lfo(Free) r0.2 f0.5 s0 false", 0x12fe3ca9855feb08),
    ("Lfo(Free) r0.2 f0.5 s0 true", 0x12fe3ca9855feb08),
    ("Lfo(Free) r0.2 f0.5 s0.1 false", 0xb6378cbe725a6158),
    ("Lfo(Free) r0.2 f0.5 s0.1 true", 0xb6378cbe725a6158),
    ("Lfo(Free) r0.2 f0.5 s0.25 false", 0xf0827763ee176108),
    ("Lfo(Free) r0.2 f0.5 s0.25 true", 0xf0827763ee176108),
    ("Lfo(Free) r0.2 f0.5 s0.5 false", 0x1600438a4ddde336),
    ("Lfo(Free) r0.2 f0.5 s0.5 true", 0x1600438a4ddde336),
    ("Lfo(Free) r0.2 f0.5 s0.75 false", 0xe742ba934be406a9),
    ("Lfo(Free) r0.2 f0.5 s0.75 true", 0xe742ba934be406a9),
    ("Lfo(Free) r0.2 f0.5 s0.9 false", 0x87958c0e7e7444e2),
    ("Lfo(Free) r0.2 f0.5 s0.9 true", 0x87958c0e7e7444e2),
    ("Lfo(Free) r0.2 f0.5 s1 false", 0x5da8f59742315008),
    ("Lfo(Free) r0.2 f0.5 s1 true", 0x5da8f59742315008),
    ("Lfo(Free) r0.6 f0.3 s0 false", 0xdf4084f6a723e22e),
    ("Lfo(Free) r0.6 f0.3 s0 true", 0xdf4084f6a723e22e),
    ("Lfo(Free) r0.6 f0.3 s0.1 false", 0x8c20c0b749c4bb6a),
    ("Lfo(Free) r0.6 f0.3 s0.1 true", 0x8c20c0b749c4bb6a),
    ("Lfo(Free) r0.6 f0.3 s0.25 false", 0x53e4f581fcb14143),
    ("Lfo(Free) r0.6 f0.3 s0.25 true", 0x53e4f581fcb14143),
    ("Lfo(Free) r0.6 f0.3 s0.5 false", 0xc9825da3dfdfee41),
    ("Lfo(Free) r0.6 f0.3 s0.5 true", 0xc9825da3dfdfee41),
    ("Lfo(Free) r0.6 f0.3 s0.75 false", 0x83740984265f1bc3),
    ("Lfo(Free) r0.6 f0.3 s0.75 true", 0x83740984265f1bc3),
    ("Lfo(Free) r0.6 f0.3 s0.9 false", 0xb6005470c431441b),
    ("Lfo(Free) r0.6 f0.3 s0.9 true", 0xb6005470c431441b),
    ("Lfo(Free) r0.6 f0.3 s1 false", 0xf0a3d3dc95b412c8),
    ("Lfo(Free) r0.6 f0.3 s1 true", 0xf0a3d3dc95b412c8),
];

#[test]
fn envelope_b_fills_as_main_did() {
    let h = hashes();
    if std::env::var_os("EFF_PRINT").is_some() {
        for (name, x) in &h {
            println!("    ({name:?}, 0x{x:016x}),");
        }
    }
    let got: Vec<(&str, u64)> = h.iter().map(|(n, x)| (n.as_str(), *x)).collect();
    assert_eq!(got, BEFORE);
}
