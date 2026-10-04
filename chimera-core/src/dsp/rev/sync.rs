//! Tempo for the synced TYPEs (reverb TYPEs spec § Tempo sync). The note
//! values, the fold and SYNC's conversion land with Task 12.

/// The tempo, in BPM: free of any sample rate, which the engine applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tempo(pub f32);

impl Tempo {
    pub const DEFAULT: Tempo = Tempo(120.0);

    /// One beat, in ms.
    pub fn beat_ms(self) -> f32 {
        60_000.0 / self.0
    }
}
