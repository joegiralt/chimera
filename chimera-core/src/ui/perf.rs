/// Performance counters fed by the platform, displayed by the renderer.
/// All times in microseconds.
#[derive(Clone, Copy, Debug)]
pub struct PerfStats {
    /// UI render time (microseconds)
    pub render_us: u32,
    /// Total frame time including sleep (microseconds)
    pub frame_us: u32,
    /// Audio block render time (microseconds) — 0 if not measured
    pub audio_us: u32,
    /// Audio CPU load as percentage of budget (128 samples @ 48kHz = 2667us)
    pub audio_load_pct: u8,
}

impl PerfStats {
    pub const fn zero() -> Self {
        Self {
            render_us: 0,
            frame_us: 0,
            audio_us: 0,
            audio_load_pct: 0,
        }
    }
}

/// Rolling average over N frames to smooth jitter.
pub struct PerfTracker {
    render_sum: u32,
    frame_sum: u32,
    audio_sum: u32,
    count: u32,
    pub stats: PerfStats,
}

const AVG_FRAMES: u32 = 8;

impl Default for PerfTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl PerfTracker {
    pub const fn new() -> Self {
        Self {
            render_sum: 0,
            frame_sum: 0,
            audio_sum: 0,
            count: 0,
            stats: PerfStats::zero(),
        }
    }

    /// Record one frame's measurements.
    pub fn record(&mut self, render_us: u32, frame_us: u32, audio_us: u32) {
        self.render_sum += render_us;
        self.frame_sum += frame_us;
        self.audio_sum += audio_us;
        self.count += 1;

        if self.count >= AVG_FRAMES {
            self.stats.render_us = self.render_sum / AVG_FRAMES;
            self.stats.frame_us = self.frame_sum / AVG_FRAMES;
            self.stats.audio_us = self.audio_sum / AVG_FRAMES;
            // Audio budget: 128 samples @ 48kHz = 2667us
            self.stats.audio_load_pct = if self.stats.audio_us > 0 {
                ((self.stats.audio_us as u64 * 100) / 2667) as u8
            } else {
                0
            };
            self.render_sum = 0;
            self.frame_sum = 0;
            self.audio_sum = 0;
            self.count = 0;
        }
    }
}
