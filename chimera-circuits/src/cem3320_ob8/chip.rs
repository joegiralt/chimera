//! The chip as wired (`Chip`: the OB-8's 4-pole and the data sheet's own
//! circuit), its small-signal maps, and the two device functions every
//! model shares: the antilog side of the cell (`antilog`) and the
//! degenerated resonance pair (`ota`).

use std::f64::consts::PI;

/// The reference's pole calibration (see `Chip::g_set`), measured once at
/// 1 kHz on the OB-8 chip.
pub const CAL: f64 = 1.0093;

/// Thermal voltage at 27 °C.
pub const VT: f64 = 1.380649e-23 * 300.15 / 1.602176634e-19;
pub(crate) const Q_E: f64 = 1.602176634e-19;
pub(crate) const K_T: f64 = 1.380649e-23 * 300.15;

/// One chip as wired. Values: data sheet typicals unless marked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chip {
    /// Internal reference current, 63 µA typ (45–85).
    pub i_ref: f64,
    /// The input diode's voltage at `I_REF`, "a nominal 650 mV".
    pub vd0: f64,
    /// Ohmic emitter resistance in the diode and the antilog transistor
    /// (assumed, small-IC typical): the cell's only current distortion,
    /// and only when `G ≠ 1`. The lab's `CEM_RE`.
    pub re: f64,
    /// The antilog transistor's beta: its base current loads the diode node.
    pub beta: f64,
    /// Early voltages of the cell's npn and the pnp mirror output. Their
    /// sum sets "output impedance of gain cell … reflected back to the input
    /// as … 1 MΩ regardless of control voltage" (126 V each gives 1.00 MΩ).
    pub va_n: f64,
    pub va_p: f64,
    /// Cap-node saturation (the cell's output transistors), placed so the
    /// buffer swings 0.9 … 12.9 V: `V_OUT(p-p) = V_CC − 3 V` about 6.9 V.
    pub v_lo: f64,
    pub v_hi: f64,
    /// Buffer: an emitter follower, `V_BE` 0.65 V at its class-A sink
    /// ("sink a nominal 500 µA").
    pub vbe0: f64,
    pub i0: f64,
    pub c: f64,
    pub rf: f64,
    /// Coupling resistors into stages 2–4.
    pub rc: f64,
    /// Bias resistors from stages 2–4 to −15 V.
    pub rb: f64,
    /// Stage 1's input: `Some(R)` a voltage source through R (data sheet
    /// R_I), `None` a Norton current (the OB-8's bus through 180 K + 18 K).
    pub r_in: Option<f64>,
    /// Full scale of one digital unit: amps (Norton) or volts (R_I).
    pub fs_in: f64,
    /// Resonance OTA: emitter degeneration per side, the pin-8 resistor to
    /// ground (3.6 K), pin 8's external cap (the lab's `CEM_C8`; 0 removes
    /// it), the feedback resistor and its coupling cap.
    pub r_ota: f64,
    pub r8: f64,
    pub c8: f64,
    pub r_ri: f64,
    pub c_ri: f64,
    /// The output's load: a coupling cap into a virtual ground through R.
    pub r_load: f64,
    pub c_load: f64,
}

impl Chip {
    /// The OB-8 voice board's 4-pole (sheet 3, 1682B): +15/−5 supplies, the
    /// bus into pin 1 through CX12 1 µF + RX43 18 K (one CEM3340 saw, 10 V
    /// p-p, through RX31 180 K: 25.3 µA peak = one digital unit), R_F 100 K,
    /// 300 pF poles, pin 10 → CX20 1 µF → RX64 130 K → pin 8 with CX16
    /// 1 nF, the output into the CEM3360 through CX21 0.1 µF / RX62 75 K.
    pub const OB8: Chip = Chip {
        i_ref: 63e-6,
        vd0: 0.65,
        re: 8.0,
        beta: 100.0,
        va_n: 126.0,
        va_p: 126.0,
        v_lo: 0.9 + 0.65,
        v_hi: 12.9 + 0.65,
        vbe0: 0.65,
        i0: 0.5e-3,
        c: 300e-12,
        rf: 100e3,
        rc: 91e3,
        rb: 220e3,
        r_in: None,
        fs_in: 5.0 / 198e3,
        r_ota: 455.0,
        r8: 3.6e3,
        c8: 1e-9,
        r_ri: 130e3,
        c_ri: 1e-6,
        r_load: 75e3,
        c_load: 0.1e-6,
    };

    /// The data sheet's own 4-pole (fig. "Circuit Block and Connection"):
    /// R_I 91 K, R_RI 51 K, no pin-8 cap, unloaded output. 1 unit = 1 V in.
    pub const DATASHEET: Chip = Chip {
        r_in: Some(91e3),
        fs_in: 1.0,
        c8: 0.0,
        r_ri: 51e3,
        r_load: 1e12,
        ..Chip::OB8
    };

    /// `R_F ‖ reflected`: the data sheet's R_EQ.
    pub fn r_eq(&self) -> f64 {
        1.0 / (1.0 / self.rf + self.i_ref * (1.0 / self.va_n + 1.0 / self.va_p))
    }

    /// The cell gain `A_IO·e^(−V_C/V_T)` that puts each pole at `fc`.
    pub fn g_of(&self, fc: f64) -> f64 {
        2.0 * PI * fc * self.r_eq() * self.c
    }

    /// The cell gain that puts the reference's measured pole on `fc`: the
    /// antilog transistor's base current costs `1 + G/β`; `CAL` is the rest
    /// (the diode node's impedance against R_F, the Early reference),
    /// measured once at 1 kHz.
    pub fn g_set(&self, fc: f64) -> f64 {
        let want = self.g_of(fc) * CAL;
        // Small-signal: G_eff = G/(1 + G/β)·(1 + ρ)/(1 + G·ρ), ρ = re·I_REF/V_T.
        let rho = self.re * self.i_ref / VT;
        let mut g = want;
        for _ in 0..8 {
            g = want * (1.0 + g / self.beta) * (1.0 + g * rho) / (1.0 + rho);
        }
        g
    }

    /// The OTA's small-signal gm at tail current `i`: `i/(2V_T + R_E·i)`.
    pub fn gm(&self, i: f64) -> f64 {
        i / (2.0 * VT + self.r_ota * i)
    }

    /// Pin 8's divider and the loop gain per mho (stage 1's R_EQ, three
    /// unity-ish stages).
    pub fn k_per_gm(&self) -> f64 {
        let a8 = self.r8 / (self.r8 + self.r_ri);
        let g = self.r_eq() / self.rc;
        a8 * self.r_eq() * g * g * g
    }

    /// Pin 9's current for loop gain `k` (inverse of `gm`).
    pub fn icr_of_k(&self, k: f64) -> f64 {
        let gm = (k / self.k_per_gm()).min(0.999 / self.r_ota);
        gm * 2.0 * VT / (1.0 - gm * self.r_ota)
    }

    /// Volts at pin 10 (deviation) per digital unit at DC, RES 0.
    pub fn out_per_unit(&self) -> f64 {
        let g = self.r_eq() / self.rc;
        let s1 = match self.r_in {
            Some(r) => self.fs_in * self.r_eq() / r,
            None => self.fs_in * self.r_eq(),
        };
        s1 * g * g * g
    }
}

#[inline]
pub(crate) fn ex(x: f64) -> f64 {
    x.min(80.0).exp()
}

/// The antilog transistor's collector current for diode current `id`, with
/// the shared ohmic `re`: `I_C = G·I_D·e^{re[(I_D − I_C) − (1 − G)I_REF]/V_T}`.
/// Returns `(I_C, dI_C/dI_D)`.
pub fn antilog(id: f64, g: f64, ch: &Chip) -> (f64, f64) {
    let id = id.max(0.0);
    if ch.re == 0.0 {
        return (g * id, g);
    }
    let r = ch.re / VT;
    let mut ic = g * id;
    let mut e = 1.0;
    for _ in 0..30 {
        e = ex(r * ((id - ic) - (1.0 - g) * ch.i_ref));
        let h = ic - g * id * e;
        let dh = 1.0 + g * id * e * r;
        let d = h / dh;
        ic -= d;
        if d.abs() < 1e-18 + 1e-13 * ic.abs() {
            break;
        }
    }
    let dic = g * e * (1.0 + id * r) / (1.0 + g * id * e * r);
    (ic, dic)
}

/// The degenerated pair: `v = 2V_T·atanh(x) + R_E·I·x`, `I_out = I·x`.
/// Returns `(I_out, dI_out/dv)`.
pub fn ota(v: f64, i: f64, ch: &Chip) -> (f64, f64) {
    if i <= 0.0 {
        return (0.0, 0.0);
    }
    let (a, b) = (2.0 * VT, ch.r_ota * i);
    let mut y = v / (a + b);
    for _ in 0..60 {
        let t = y.tanh();
        let f = a * y + b * t - v;
        let d = f / (a + b * (1.0 - t * t));
        y -= d;
        if d.abs() < 1e-14 {
            break;
        }
    }
    let t = y.tanh();
    (i * t, i * (1.0 - t * t) / (a + b * (1.0 - t * t)))
}

/// The pair's large-signal knee: the pin-8 voltage at which `I_out` reaches
/// 90 % of the tail `i` (1e-4 V steps, as the lab measured it).
pub fn ota_knee(i: f64, ch: &Chip) -> f64 {
    let mut v = 0.0;
    while ota(v, i, ch).0 < 0.9 * i {
        v += 1e-4;
    }
    v
}
