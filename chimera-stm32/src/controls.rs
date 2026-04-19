//! HC165 shift register control input.

use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId, NUM_BUTTONS, NUM_ENCODERS};
use stm32h7xx_hal::hal::digital::v2::{InputPin, OutputPin};

pub struct Stm32Controls<DATA, LOAD, CLK> {
    data: DATA,
    load: LOAD,
    clk: CLK,
    prev_bits: u32,
    encoder_accum: [i8; NUM_ENCODERS],
    button_current: [bool; NUM_BUTTONS],
    button_previous: [bool; NUM_BUTTONS],
}

impl<DATA, LOAD, CLK> Stm32Controls<DATA, LOAD, CLK>
where
    DATA: InputPin,
    LOAD: OutputPin,
    CLK: OutputPin,
{
    pub fn new(data: DATA, load: LOAD, clk: CLK) -> Self {
        Self {
            data,
            load,
            clk,
            prev_bits: 0,
            encoder_accum: [0; NUM_ENCODERS],
            button_current: [false; NUM_BUTTONS],
            button_previous: [false; NUM_BUTTONS],
        }
    }

    pub fn poll(&mut self) {
        self.button_previous = self.button_current;

        // Latch
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

        // Buttons (bits 0-11)
        for i in 0..NUM_BUTTONS {
            self.button_current[i] = (bits >> i) & 1 != 0;
        }

        // Encoders (bits 12-23, quadrature pairs)
        for i in 0..NUM_ENCODERS.min(6) {
            let bit_a = (bits >> (12 + i * 2)) & 1;
            let bit_b = (bits >> (12 + i * 2 + 1)) & 1;
            let prev_a = (self.prev_bits >> (12 + i * 2)) & 1;

            if bit_a != prev_a {
                self.encoder_accum[i] = if bit_a == 1 {
                    if bit_b == 0 { 1 } else { -1 }
                } else {
                    0
                };
            } else {
                self.encoder_accum[i] = 0;
            }
        }

        self.prev_bits = bits;
    }
}

impl<DATA, LOAD, CLK> Controls for Stm32Controls<DATA, LOAD, CLK>
where
    DATA: InputPin,
    LOAD: OutputPin,
    CLK: OutputPin,
{
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
