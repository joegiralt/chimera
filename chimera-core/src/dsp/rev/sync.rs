//! Tempo for the synced TYPEs (reverb TYPEs spec § Tempo sync).

/// The beat's length, in samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tempo(pub f32);

impl Tempo {
    /// 120 BPM at 48 kHz.
    pub const DEFAULT: Tempo = Tempo(24_000.0);
}
