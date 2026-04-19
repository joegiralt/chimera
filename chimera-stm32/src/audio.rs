//! SAI audio output via DMA.
//!
//! SAI1 Block A: master TX, 48kHz 32-bit I2S, circular DMA
//! Half-transfer ISR renders 128 samples (BLOCK_SIZE) from chimera-core.
//!
//! TODO: This module is a placeholder. Full DMA circular buffer setup
//! requires unsafe static buffers and interrupt handlers which need
//! careful implementation. For initial bringup, audio is rendered
//! in the main loop.

use chimera_hal::BLOCK_SIZE;

/// Audio DMA buffer — double-buffered, placed in D2 SRAM for DMA access.
/// Each half = BLOCK_SIZE stereo samples = 128 * 2 * 4 bytes = 1024 bytes.
#[repr(align(4))]
pub struct AudioBuffer {
    pub data: [i32; BLOCK_SIZE * 2 * 2], // double-buffer, stereo, 32-bit
}

impl AudioBuffer {
    pub const fn new() -> Self {
        Self {
            data: [0; BLOCK_SIZE * 2 * 2],
        }
    }
}

/// Convert f32 audio samples to i32 for the SAI DAC.
/// The CS4344 expects 32-bit I2S (left-justified).
pub fn f32_to_i32_stereo(input: &[f32; BLOCK_SIZE], output: &mut [i32], offset: usize) {
    for i in 0..BLOCK_SIZE {
        let sample = (input[i] * 0.7 * (i32::MAX as f32)) as i32;
        output[offset + i * 2] = sample; // Left
        output[offset + i * 2 + 1] = sample; // Right (mono for now)
    }
}
