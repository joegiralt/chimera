pub mod dma;
pub mod engine;
pub mod sai;

use chimera_board::AudioStart;
use chimera_board::clocks;
use chimera_engine::audio_out::Half;
use chimera_engine::clock_plan::pll3_for;
use chimera_engine::hw::SampleBudget;

pub fn render_half(half: Half) {
    engine::render_half(half);
}

pub fn prefill() {
    render_half(Half::First);
    render_half(Half::Second);
}

/// Start-up step 2 (`chimera_board::synth`): the engine built, then the
/// clocks, the SAI and its DMA, and MIDI DIN.
pub fn start(a: AudioStart<'_>) {
    engine::init(
        SampleBudget::for_cpu(a.clk.cpu_hz),
        a.shared,
        a.notes,
        a.scope,
        a.formant,
        a.orbit,
        a.sources,
    );
    let pll3 = pll3_for(clocks::HSE_HZ, chimera_hal::SAMPLE_RATE, a.clk.rev);
    clocks::init_pll3(&pll3);
    sai::init(a.clk.rev.new_sai(), pll3.mckdiv);
    dma::clear();
    prefill();
    dma::init(a.nvic);
    dma::start();
    sai::start();
    #[cfg(feature = "midi-din")]
    crate::midi_din::init(a.nvic, a.pclk2_hz, a.din, a.din_ccs);
    #[cfg(not(feature = "midi-din"))]
    let _ = a.pclk2_hz;
}
