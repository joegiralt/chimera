//! HC165 shift register control input.
//! Reads 24 bits (3 daisy-chained HC165s) for buttons and encoders.

use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId, NUM_BUTTONS, NUM_ENCODERS};
use stm32h7xx_hal::gpio::{Input, Output, PushPull};
use stm32h7xx_hal::hal::digital::v2::{InputPin, OutputPin};

// Type aliases for the specific GPIO pins
type DataPin = stm32h7xx_hal::gpio::PA0<Input>;
type LoadPin = stm32h7xx_hal::gpio::PA1<Output<PushPull>>;
type ClkPin = stm32h7xx_hal::gpio::PA2<Output<PushPull>>;

pub struct Stm32Controls {
    data: DataPin,
    load: LoadPin,
    clk: ClkPin,
    raw_bits: u32,
    prev_bits: u32,
    encoder_accum: [i8; NUM_ENCODERS],
    button_current: [bool; NUM_BUTTONS],
    button_previous: [bool; NUM_BUTTONS],
}

impl Stm32Controls {
    pub fn new(data: DataPin, load: LoadPin, clk: ClkPin) -> Self {
        Self {
            data,
            load,
            clk,
            raw_bits: 0,
            prev_bits: 0,
            encoder_accum: [0; NUM_ENCODERS],
            button_current: [false; NUM_BUTTONS],
            button_previous: [false; NUM_BUTTONS],
        }
    }

    /// Read all 24 bits from the HC165 chain.
    pub fn poll(&mut self) {
        self.prev_bits = self.raw_bits;
        self.button_previous = self.button_current;

        // Latch parallel inputs
        let _ = self.load.set_low();
        cortex_m::asm::delay(10);
        let _ = self.load.set_high();
        cortex_m::asm::delay(10);

        // Shift in 24 bits
        let mut bits: u32 = 0;
        for i in 0..24 {
            let _ = self.clk.set_low();
            cortex_m::asm::delay(5);
            if self.data.is_high().unwrap_or(false) {
                bits |= 1 << i;
            }
            let _ = self.clk.set_high();
            cortex_m::asm::delay(5);
        }
        self.raw_bits = bits;

        // Decode buttons (bits 0-11 = 12 buttons)
        for i in 0..NUM_BUTTONS {
            self.button_current[i] = (bits >> i) & 1 != 0;
        }

        // Decode encoders (bits 12-23 = 6 encoder pairs: A/B per encoder + main)
        // Each encoder uses 2 bits (quadrature A/B)
        // TODO: proper quadrature decoding with gray code
        for i in 0..NUM_ENCODERS.min(6) {
            let bit_a = (bits >> (12 + i * 2)) & 1;
            let bit_b = (bits >> (12 + i * 2 + 1)) & 1;
            let prev_a = (self.prev_bits >> (12 + i * 2)) & 1;

            // Simple edge detection on A channel
            if bit_a != prev_a {
                if bit_a == 1 {
                    self.encoder_accum[i] = if bit_b == 0 { 1 } else { -1 };
                } else {
                    self.encoder_accum[i] = 0;
                }
            } else {
                self.encoder_accum[i] = 0;
            }
        }
    }
}

impl Controls for Stm32Controls {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        let idx = id as usize;
        if idx < NUM_ENCODERS {
            self.encoder_accum[idx]
        } else {
            0
        }
    }

    fn button_state(&self, id: ButtonId) -> ButtonState {
        let idx = id as usize;
        if idx >= NUM_BUTTONS {
            return ButtonState::Up;
        }
        match (self.button_previous[idx], self.button_current[idx]) {
            (false, true) => ButtonState::Pressed,
            (true, true) => ButtonState::Held,
            (true, false) => ButtonState::Released,
            (false, false) => ButtonState::Up,
        }
    }
}
