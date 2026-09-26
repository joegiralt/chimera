pub mod dma;
pub mod engine;
pub mod sai;

use chimera_core::audio_out::Half;

pub fn render_half(half: Half) {
    engine::render_half(half);
}

pub fn prefill() {
    render_half(Half::First);
    render_half(Half::Second);
}
