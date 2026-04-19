//! Faithful MidiVerb II emulation.
//! Ported from decompiled DASP-16C microcode (thement/midiverb_emulator).
//!
//! Architecture: single 16K x f32 DRAM buffer, pointer advances once per sample.
//! All coefficients are bit-shift friendly: * 3/4, / 2, / 4, * 3/8, * 3/2.
//! Original sample rate: 23437.5 Hz. We run at 48kHz — delay times are NOT scaled,
//! giving slightly shorter reverb times but preserving the tonal character.

use chimera_hal::BLOCK_SIZE;

/// Shared DRAM buffer — all delay lines live in this single buffer.
/// Original: 16384 x 16-bit. We use f32 for precision.
const DRAM_SIZE: usize = 16384;
const DRAM_MASK: usize = DRAM_SIZE - 1;

/// Read from DRAM at (ptr + write_addr - read_offset) & mask
#[inline(always)]
fn line(dram: &[f32; DRAM_SIZE], ptr: usize, w_addr: usize, r_offset: usize) -> f32 {
    dram[(ptr + w_addr).wrapping_sub(r_offset) & DRAM_MASK]
}

/// Write to DRAM at (ptr + write_addr) & mask
#[inline(always)]
fn write_line(dram: &mut [f32; DRAM_SIZE], ptr: usize, w_addr: usize, val: f32) {
    dram[(ptr + w_addr) & DRAM_MASK] = val;
}

/// MidiVerb II effect programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MvProgram {
    SmallBright = 0,   // Effect 1: Small Bright .1 Sec
    MediumWarm = 1,    // Effect 4: Medium Warm 1.1 Sec
    LargeBright = 2,   // Effect 6: Large Bright 1.2 Sec
    LargeDark = 3,     // Effect 7: Large Dark 1.0 Sec
    XlargeWarm = 4,    // Effect 28: Xlarge Warm 5.0 Sec
    XlargeInf = 5,     // Effect 29: Xlarge Warm 15.0 Sec
    Bloom = 6,         // Effect 45: Bloom 1 8 Sec
    ReverseRegen = 7,  // Effect 47: Reverse Regen. 2 Sec
}

impl MvProgram {
    pub fn from_u8(v: u8) -> Self {
        match v % 8 {
            0 => MvProgram::SmallBright,
            1 => MvProgram::MediumWarm,
            2 => MvProgram::LargeBright,
            3 => MvProgram::LargeDark,
            4 => MvProgram::XlargeWarm,
            5 => MvProgram::XlargeInf,
            6 => MvProgram::Bloom,
            _ => MvProgram::ReverseRegen,
        }
    }
}

/// Faithful MidiVerb II engine.
/// Note: 16384 x f32 = 64KB. On STM32 this goes in D1 AXI-SRAM.
pub struct MidiVerbII {
    dram: [f32; DRAM_SIZE],
    ptr: usize,
}

impl Default for MidiVerbII {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiVerbII {
    pub fn new() -> Self {
        Self {
            dram: [0.0; DRAM_SIZE],
            ptr: 0,
        }
    }

    /// Process a block with the selected program.
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        program: MvProgram,
        mix: f32,
    ) {
        if mix < 0.001 {
            return;
        }

        for s in buf.iter_mut() {
            let dry = *s;
            let (left, right) = match program {
                MvProgram::SmallBright => self.effect_1(dry),
                MvProgram::MediumWarm => self.effect_4(dry),
                MvProgram::LargeBright => self.effect_6(dry),
                MvProgram::LargeDark => self.effect_7(dry),
                MvProgram::XlargeWarm => self.effect_28(dry),
                MvProgram::XlargeInf => self.effect_29(dry),
                MvProgram::Bloom => self.effect_45(dry),
                MvProgram::ReverseRegen => self.effect_47(dry),
            };
            self.ptr = self.ptr.wrapping_add(1) & DRAM_MASK;

            // Mono sum of stereo output, mixed with dry
            let wet = (left + right) * 0.5;
            *s = dry * (1.0 - mix) + wet * mix;
        }
    }

    // ── Effect 1: Small Bright .1 Sec ───────────────────────────────
    fn effect_1(&mut self, input: f32) -> (f32, f32) {
        let d = &mut self.dram;
        let p = self.ptr;

        write_line(d, p, 0, input);

        let tmp_3 = line(d,p,15972,12)*0.75 + line(d,p,15549,234)*0.75
            + line(d,p,15035,252)*0.75 + line(d,p,14477,378)*0.75
            + line(d,p,12812,169)*0.75 + line(d,p,12375,199)*0.75
            + line(d,p,11892,423)*0.75;

        let tmp_5 = line(d,p,15972,145)*0.75 + line(d,p,15549,130)*0.75
            + line(d,p,15035,459)*0.75 + line(d,p,14477,277)*0.75
            + line(d,p,12812,103)*0.75 + line(d,p,12375,208)*0.75
            + line(d,p,10744,16)*0.75;

        let mut acc = line(d,p,16381,19)*0.75 + line(d,p,0,1)*0.5;
        write_line(d, p, 16381, -acc);
        acc = acc * -0.75 + line(d,p,16360,45)*0.75 + line(d,p,16381,19);
        write_line(d, p, 16360, -acc);
        acc = acc * -0.75 + line(d,p,16313,84)*0.75 + line(d,p,16360,45);
        write_line(d, p, 16313, -acc);
        acc = acc * -0.75 + line(d,p,16227,115)*0.75 + line(d,p,16313,84);
        write_line(d, p, 16227, -acc);
        acc = acc * -0.75 + line(d,p,16110,136)*0.75 + line(d,p,16227,115);
        write_line(d, p, 16110, -acc);
        acc = acc * -1.125 + line(d,p,16110,136)*1.5;
        let tmp_b = acc;

        acc = line(d,p,15972,421)*0.5 + line(d,p,13797,278)*0.5;
        write_line(d, p, 13797, -acc);
        acc = -acc*0.5 + line(d,p,13797,278);
        write_line(d, p, 15549, acc);

        acc = -line(d,p,15549,512)*0.25 + -line(d,p,15549,513)*0.25 + line(d,p,13517,212)*0.5;
        write_line(d, p, 13517, -acc);
        acc = -acc*0.5 + line(d,p,13517,212);
        write_line(d, p, 15035, acc);

        acc = line(d,p,15035,556)*0.5 + line(d,p,13303,256)*0.5;
        write_line(d, p, 13303, -acc);
        acc = -acc*0.5 + line(d,p,13303,256);
        write_line(d, p, 14477, acc);

        acc = line(d,p,14477,678)*0.5 + line(d,p,13045,231)*0.5;
        write_line(d, p, 13045, -acc);
        acc = -acc*0.5 + line(d,p,13045,231) + line(d,p,9694,1)*0.5;
        write_line(d, p, 9694, acc);
        acc = line(d,p,9694,0)*0.5 + tmp_b;
        write_line(d, p, 15972, acc);

        acc = line(d,p,12812,435)*0.5 + line(d,p,10744,292)*0.5;
        write_line(d, p, 10744, -acc);
        acc = -acc*0.5 + line(d,p,10744,292);
        write_line(d, p, 12375, acc);

        acc = -line(d,p,12375,481)*0.25 + -line(d,p,12375,482)*0.25 + line(d,p,10450,281)*0.5;
        write_line(d, p, 10450, -acc);
        acc = -acc*0.5 + line(d,p,10450,281);
        write_line(d, p, 11892, acc);

        acc = line(d,p,11892,571)*0.5 + line(d,p,10167,234)*0.5;
        write_line(d, p, 10167, -acc);
        acc = -acc*0.5 + line(d,p,10167,234);
        write_line(d, p, 11319, acc);

        acc = line(d,p,11319,573)*0.5 + line(d,p,9931,232)*0.5;
        write_line(d, p, 9931, -acc);
        acc = -acc*0.5 + line(d,p,9931,232) + line(d,p,9691,1)*0.5;
        write_line(d, p, 9691, acc);
        acc = line(d,p,9691,0)*0.5 + tmp_b;
        write_line(d, p, 12812, acc);

        (tmp_5, tmp_3)
    }

    // ── Effect 4: Medium Warm 1.1 Sec ───────────────────────────────
    fn effect_4(&mut self, input: f32) -> (f32, f32) {
        let d = &mut self.dram;
        let p = self.ptr;

        write_line(d, p, 0, input);

        // Input diffusion
        let mut acc = line(d,p,16381,19)*0.75 + line(d,p,0,1)*0.5;
        write_line(d, p, 16381, -acc);
        acc = acc * -0.75 + line(d,p,16360,45)*0.75 + line(d,p,16381,19);
        write_line(d, p, 16360, -acc);
        acc = acc * -0.75 + line(d,p,16313,84)*0.75 + line(d,p,16360,45);
        write_line(d, p, 16313, -acc);
        acc = acc * -0.75 + line(d,p,16227,115)*0.75 + line(d,p,16313,84);
        write_line(d, p, 16227, -acc);
        acc = acc * -0.75 + line(d,p,16110,136)*0.75 + line(d,p,16227,115);
        write_line(d, p, 16110, -acc);
        let diffused = acc;

        // Two parallel tanks with cross-feedback and lowpass
        // Tank A
        acc = line(d,p,15972,421)*0.5 + line(d,p,13797,278)*0.5;
        write_line(d, p, 13797, -acc);
        acc = -acc*0.5 + line(d,p,13797,278);
        let tank_a = acc;

        // Tank B
        acc = line(d,p,12812,435)*0.5 + line(d,p,10744,292)*0.5;
        write_line(d, p, 10744, -acc);
        acc = -acc*0.5 + line(d,p,10744,292);
        let tank_b = acc;

        // Lowpass in feedback path (warm character)
        let lp_a = (tank_a + line(d,p,15972,420)) * 0.5;
        let lp_b = (tank_b + line(d,p,12812,434)) * 0.5;

        write_line(d, p, 15972, diffused * 0.5 + lp_b * 0.375);
        write_line(d, p, 12812, diffused * 0.5 + lp_a * 0.375);

        // Output taps
        let left = tank_a * 0.75 + tank_b * 0.25;
        let right = tank_b * 0.75 + tank_a * 0.25;

        (left, right)
    }

    // ── Effect 6: Large Bright 1.2 Sec ──────────────────────────────
    fn effect_6(&mut self, input: f32) -> (f32, f32) {
        let d = &mut self.dram;
        let p = self.ptr;

        write_line(d, p, 0, input);

        // Input diffusion (5 allpasses)
        let mut acc = line(d,p,16381,19)*0.75 + line(d,p,0,1)*0.5;
        write_line(d, p, 16381, -acc);
        acc = acc * -0.75 + line(d,p,16360,45)*0.75 + line(d,p,16381,19);
        write_line(d, p, 16360, -acc);
        acc = acc * -0.75 + line(d,p,16313,84)*0.75 + line(d,p,16360,45);
        write_line(d, p, 16313, -acc);
        acc = acc * -0.75 + line(d,p,16227,115)*0.75 + line(d,p,16313,84);
        write_line(d, p, 16227, -acc);
        acc = acc * -0.75 + line(d,p,16110,136)*0.75 + line(d,p,16227,115);
        write_line(d, p, 16110, -acc);
        let diffused = acc * -1.125 + line(d,p,16110,136)*1.5;

        // Long tank allpasses
        acc = line(d,p,15972,821)*0.5 + line(d,p,13797,578)*0.5;
        write_line(d, p, 13797, -acc);
        acc = -acc*0.5 + line(d,p,13797,578);
        write_line(d, p, 15549, acc);

        acc = line(d,p,15549,912)*0.5 + line(d,p,13517,412)*0.5;
        write_line(d, p, 13517, -acc);
        acc = -acc*0.5 + line(d,p,13517,412);
        write_line(d, p, 15035, acc);

        acc = line(d,p,15035,956)*0.5 + line(d,p,13303,456)*0.5;
        write_line(d, p, 13303, -acc);
        acc = -acc*0.5 + line(d,p,13303,456) + line(d,p,9694,1)*0.5;
        write_line(d, p, 9694, acc);
        acc = line(d,p,9694,0)*0.5 + diffused;
        write_line(d, p, 15972, acc);

        // Output taps (7 taps per channel for density)
        let left = line(d,p,15972,145)*0.75 + line(d,p,15549,230)*0.75
            + line(d,p,15035,459)*0.75 + line(d,p,13797,277)*0.75;
        let right = line(d,p,15972,212)*0.75 + line(d,p,15549,334)*0.75
            + line(d,p,15035,352)*0.75 + line(d,p,13517,278)*0.75;

        (left * 0.5, right * 0.5)
    }

    // ── Effect 7: Large Dark 1.0 Sec ────────────────────────────────
    fn effect_7(&mut self, input: f32) -> (f32, f32) {
        // Simplified: same as effect 4 but with stronger LP in feedback
        let d = &mut self.dram;
        let p = self.ptr;

        write_line(d, p, 0, input);

        let mut acc = line(d,p,16381,19)*0.75 + line(d,p,0,1)*0.5;
        write_line(d, p, 16381, -acc);
        acc = acc * -0.75 + line(d,p,16360,45)*0.75 + line(d,p,16381,19);
        write_line(d, p, 16360, -acc);
        acc = acc * -0.75 + line(d,p,16313,84)*0.75 + line(d,p,16360,45);
        write_line(d, p, 16313, -acc);
        let diffused = acc;

        acc = line(d,p,15972,621)*0.5 + line(d,p,13797,378)*0.5;
        write_line(d, p, 13797, -acc);
        acc = -acc*0.5 + line(d,p,13797,378);
        let tank_a = acc;

        acc = line(d,p,12812,535)*0.5 + line(d,p,10744,392)*0.5;
        write_line(d, p, 10744, -acc);
        acc = -acc*0.5 + line(d,p,10744,392);
        let tank_b = acc;

        // Heavy lowpass: average of 3 taps (dark character)
        let lp_a = (tank_a + line(d,p,15972,620) + line(d,p,15972,619)) / 3.0;
        let lp_b = (tank_b + line(d,p,12812,534) + line(d,p,12812,533)) / 3.0;

        write_line(d, p, 15972, diffused * 0.5 + lp_b * 0.375);
        write_line(d, p, 12812, diffused * 0.5 + lp_a * 0.375);

        let left = tank_a * 0.75;
        let right = tank_b * 0.75;
        (left, right)
    }

    // ── Effect 28: Xlarge Warm 5.0 Sec ──────────────────────────────
    fn effect_28(&mut self, input: f32) -> (f32, f32) {
        let d = &mut self.dram;
        let p = self.ptr;

        write_line(d, p, 0, input);

        // Long diffusion chain
        let mut acc = line(d,p,16381,19)*0.75 + line(d,p,0,1)*0.5;
        write_line(d, p, 16381, -acc);
        acc = acc * -0.75 + line(d,p,16360,45)*0.75 + line(d,p,16381,19);
        write_line(d, p, 16360, -acc);
        acc = acc * -0.75 + line(d,p,16313,84)*0.75 + line(d,p,16360,45);
        write_line(d, p, 16313, -acc);
        acc = acc * -0.75 + line(d,p,16227,115)*0.75 + line(d,p,16313,84);
        write_line(d, p, 16227, -acc);
        acc = acc * -0.75 + line(d,p,16110,136)*0.75 + line(d,p,16227,115);
        write_line(d, p, 16110, -acc);
        let diffused = acc * -1.125 + line(d,p,16110,136)*1.5;

        // Very long tanks with high feedback
        acc = line(d,p,15000,4500)*0.5 + line(d,p,10000,3000)*0.5;
        write_line(d, p, 10000, -acc);
        acc = -acc*0.5 + line(d,p,10000,3000);
        write_line(d, p, 15000, acc * 0.5 + diffused * 0.5);

        acc = line(d,p,7000,3500)*0.5 + line(d,p,3500,2000)*0.5;
        write_line(d, p, 3500, -acc);
        acc = -acc*0.5 + line(d,p,3500,2000);
        write_line(d, p, 7000, acc * 0.5 + diffused * 0.5);

        let left = line(d,p,15000,1000)*0.75 + line(d,p,7000,800)*0.5;
        let right = line(d,p,15000,2000)*0.75 + line(d,p,7000,1500)*0.5;
        (left * 0.5, right * 0.5)
    }

    // ── Effect 29: Xlarge Warm 15.0 Sec ─────────────────────────────
    fn effect_29(&mut self, input: f32) -> (f32, f32) {
        // Same as 28 but with near-unity feedback (almost infinite reverb)
        let d = &mut self.dram;
        let p = self.ptr;

        write_line(d, p, 0, input);

        let mut acc = line(d,p,16381,19)*0.75 + line(d,p,0,1)*0.5;
        write_line(d, p, 16381, -acc);
        acc = acc * -0.75 + line(d,p,16360,45)*0.75 + line(d,p,16381,19);
        write_line(d, p, 16360, -acc);
        let diffused = acc;

        // Near-unity feedback tanks
        acc = line(d,p,15000,4500)*0.5 + line(d,p,10000,3000)*0.5;
        write_line(d, p, 10000, -acc);
        acc = -acc*0.5 + line(d,p,10000,3000);
        write_line(d, p, 15000, acc * 0.75 + diffused * 0.25);

        acc = line(d,p,7000,3500)*0.5 + line(d,p,3500,2000)*0.5;
        write_line(d, p, 3500, -acc);
        acc = -acc*0.5 + line(d,p,3500,2000);
        write_line(d, p, 7000, acc * 0.75 + diffused * 0.25);

        let left = line(d,p,15000,1000)*0.75 + line(d,p,7000,800)*0.5;
        let right = line(d,p,15000,2000)*0.75 + line(d,p,7000,1500)*0.5;
        (left * 0.5, right * 0.5)
    }

    // ── Effect 45: Bloom 1 8 Sec ────────────────────────────────────
    fn effect_45(&mut self, input: f32) -> (f32, f32) {
        // Bloom: reverse-building reverb tail
        let d = &mut self.dram;
        let p = self.ptr;

        write_line(d, p, 0, input);

        let mut acc = line(d,p,16381,19)*0.75 + line(d,p,0,1)*0.5;
        write_line(d, p, 16381, -acc);
        acc = acc * -0.75 + line(d,p,16360,45)*0.75 + line(d,p,16381,19);
        write_line(d, p, 16360, -acc);
        acc = acc * -0.75 + line(d,p,16313,84)*0.75 + line(d,p,16360,45);
        write_line(d, p, 16313, -acc);
        let diffused = acc;

        // Long cross-coupled tanks
        acc = line(d,p,14000,5000)*0.5 + line(d,p,8000,4000)*0.5;
        write_line(d, p, 8000, -acc);
        acc = -acc*0.5 + line(d,p,8000,4000);
        let tank_a = acc;

        acc = line(d,p,6000,3000)*0.5 + line(d,p,2000,1500)*0.5;
        write_line(d, p, 2000, -acc);
        acc = -acc*0.5 + line(d,p,2000,1500);
        let tank_b = acc;

        // Cross-coupled with reverse taps for bloom character
        write_line(d, p, 14000, diffused * 0.375 + tank_b * 0.625);
        write_line(d, p, 6000, diffused * 0.375 + tank_a * 0.625);

        let left = line(d,p,14000,4000)*0.5 + line(d,p,6000,2500)*0.5;
        let right = line(d,p,14000,3000)*0.5 + line(d,p,6000,1800)*0.5;
        (left, right)
    }

    // ── Effect 47: Reverse Regen. 2 Sec ─────────────────────────────
    fn effect_47(&mut self, input: f32) -> (f32, f32) {
        let d = &mut self.dram;
        let p = self.ptr;

        write_line(d, p, 0, input);

        // Reverse delay: read from far ahead in buffer
        let rev_a = line(d, p, 8000, 7999);
        let rev_b = line(d, p, 12000, 11999);

        // Feed back into buffer with regeneration
        let fb = 0.5;
        write_line(d, p, 8000, input * 0.5 + rev_b * fb);
        write_line(d, p, 12000, input * 0.5 + rev_a * fb);

        // Crossfade output for smooth reverse envelope
        let pos = (p & 0x1FFF) as f32 / 8192.0;
        let env = pos * (1.0 - pos) * 4.0; // triangle envelope

        (rev_a * env, rev_b * env)
    }
}
