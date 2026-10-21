//! The RSF Kobol Expander's VCF at transistor level: the lab's reference,
//! run by ngspice (an external tool, never linked; `util::spice`).
//!
//! The circuit is RSF's "Board" sheet (15/6/79, VCF block; the RSF service
//! manual, also at yusynth.net `schema-RSF-KOBOL.pdf` p. 8), its values from
//! the "Expander Kobol" parts lists (20/11/79):
//! - the mixer drives R24 100 K → C16 4.7 µF → CI4 pin 6 (IN 1);
//! - CI4 SSM 2040: IN→OUT 10 K (R27, R30, R33, R36), OUT→next IN 10 K
//!   (R28, R31, R34), IN to ground 220 Ω (R26, R29, R32, R35), caps C2–C5
//!   1 nF;
//! - OUT 4 (pin 10) → C6 4.7 µF → R37 4.7 K / R38 220 Ω → CI5 (CA3080)
//!   pin 2; pin 3 to ground; CI5's output current into pin 6; its I_abc
//!   from T3 (2N3906, base grounded, emitter from R46 47 K to +15 and R45
//!   from the RES CV);
//! - the same C6 node → R39 10 K / R40 220 Ω → CI3 (CA3080, "VCA
//!   linéaire") pin 3; pin 2 → R41 220 Ω to ground and R42 1 M to the "Cont
//!   Rej" trim; CI3's output into CI2A's virtual ground, R43 100 K.
//!
//! The SSM 2040 cell is the patent's (US 3,969,682 fig. 2: an NPN pair, a
//! PNP mirror into the cap, a FET follower), rewired single-ended as the
//! data sheet's block diagram (the IN pin on the pair's (−) side, (+)
//! inside at ground). The CA3080 is RCA's topology (AN6668): a simple-mirror
//! bias (I_tail = I_abc/(1 + 2/β)), PNP Wilson output mirrors, an NPN Wilson
//! return mirror, no linearising diodes. The transistor parameters are
//! generic Gummel-Poon values the lab chose, not a fit to any published
//! model. Assumptions are on `Kobol` and in `docs/ssm2040_kobol/research.md`
//! § 3.

use crate::FS;
use crate::util::harness::lerp_up;
use crate::util::sig::resample_poly;
use crate::util::spice::{run, stamp, write_series};
use std::fmt::Write as _;
use std::path::Path;

/// ngspice's rate over 48 kHz.
pub const UP: usize = 4;

/// The circuit's parts and the assumptions the sources leave open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kobol {
    /// C2–C5, 1 nF.
    pub c: f64,
    /// The 10 K chain and the 220 Ω legs.
    pub r: f64,
    pub ra: f64,
    /// R24, 100 K.
    pub rin: f64,
    /// R37/R38 into CI5, R39/R40 into CI3.
    pub r37: f64,
    pub r38: f64,
    pub r39: f64,
    pub r40: f64,
    /// Each 2040 cell's input offset (data sheet: 2 mV typical, 5 max).
    /// Assumed: one plausible chip.
    pub vos: [f64; 4],
    /// CI5's and CI3's input offsets (CA3080: 0.3 mV typical, 5 max). CI3's
    /// is what the "Cont Rej" trim leaves; the takes' note thumps say it is
    /// not nulled.
    pub vos5: f64,
    pub vos3: f64,
    /// The cell's high-side limit and the follower's sink. The low side is
    /// the pair's own collector saturation (the cap node a V_CE(sat) over
    /// the emitters, about −1.1 V at OUT). Assumed: the high side is a diode
    /// from the cap node to an internal `vm`, set so OUT tops out near +1 V
    /// (data sheet design hint 1, "±1 V"); the die's mechanism is
    /// unpublished. The sink is the data sheet's 560 µA typical.
    pub vm: f64,
    pub isink: f64,
    /// CI3's I_abc (the VCA held open). Sets only the output's scale.
    pub iabc3: f64,
}

impl Kobol {
    pub const SCHEMATIC: Kobol = Kobol {
        c: 1e-9,
        r: 10e3,
        ra: 220.0,
        rin: 100e3,
        r37: 4.7e3,
        r38: 220.0,
        r39: 10e3,
        r40: 220.0,
        vos: [1.6e-3, -2.1e-3, 0.8e-3, 2.4e-3],
        vos5: 0.4e-3,
        vos3: 0.0,
        vm: 1.2,
        isink: 560e-6,
        iabc3: 500e-6,
    };

    /// The owner's unit: `SCHEMATIC` with CI5's offset at 1.5 mV (in spec:
    /// 0.4 typical, 5 max), read off the recording's ring H2 (−39 dBc).
    pub const UNIT: Kobol = Kobol {
        vos5: 1.5e-3,
        ..Kobol::SCHEMATIC
    };

    /// kT/q at 27 °C.
    pub const VT: f64 = 0.025_865;

    /// Stages 2–4's node gain `(1/R)/ΣG`.
    pub fn a(&self) -> f64 {
        (1.0 / self.r) / (2.0 / self.r + 1.0 / self.ra)
    }

    /// The tail current that puts stages 2–4's pole at `fc` (small signal).
    pub fn ic_of(&self, fc: f64) -> f64 {
        2.0 * std::f64::consts::PI * fc * self.c * 2.0 * Self::VT / self.a()
    }

    /// CI5's divider.
    pub fn a5(&self) -> f64 {
        self.r38 / (self.r37 + self.r38)
    }

    /// CI5's I_abc for nominal loop gain `k = g_m·a₅·R27`, `g_m = I_abc/2V_T`.
    pub fn iabc5_of(&self, k: f64) -> f64 {
        k * 2.0 * Self::VT / (self.a5() * self.r)
    }

    /// CI3's small-signal gain, volts at CI2A per volt at OUT 4.
    pub fn out_gain(&self) -> f64 {
        let a3 = self.r40 / (self.r39 + self.r40);
        a3 * self.iabc3 / (2.0 * Self::VT) * 100e3
    }

    /// The circuit's output (CI2A volts) to EXP-C's scale: OUT 4 at
    /// R40/R39 through CI3, then `1/E_FULL`; negative, as CI2A inverts.
    pub fn exp_c_scale(&self) -> f64 {
        -1.0 / (self.out_gain() * f64::from(super::fw::E_FULL))
    }
}

/// Full scale at the mixer, volts: the Kobol's full VCO1 (`fw::E_FULL`
/// over R27/R24; research.md § 3 A5).
pub const V_FULL: f64 = 2.8;

/// One render's controls at 48 kHz: the mixer's output in volts, FREQ in
/// Hz, and the nominal loop gain.
pub struct Case {
    pub x: Vec<f64>,
    pub fc: Vec<f64>,
    pub k: Vec<f64>,
}

/// What a render returns, at 48 kHz: the output (CI2A, volts), OUT 1 and
/// OUT 4 (volts).
pub struct Out {
    pub y: Vec<f64>,
    pub o1: Vec<f64>,
    pub o4: Vec<f64>,
}

const MODELS: &str = "\
.model qn npn(is=1e-14 bf=200 vaf=1e4 br=2 rb=50 rc=20 re=1 cje=0.5p cjc=0.4p tf=0.3n)
.model qp pnp(is=1e-14 bf=100 vaf=1e4 br=2 rb=80 rc=40 re=2 cje=0.6p cjc=0.6p tf=1n)
.model qpl pnp(is=2e-14 bf=40 vaf=50 br=2 rb=100 rc=50 re=3 cje=1p cjc=1p tf=20n)
.model qn3 npn(is=1e-14 bf=150 vaf=100 br=2 rb=60 rc=20 re=1 cje=0.5p cjc=0.4p tf=0.4n)
.model dcl d(is=1e-14 n=1)
.model nf nmos(level=1 vto=0.626 kp=2 lambda=0.002)
";

/// The CA3080, RCA's topology.
const CA3080: &str = "\
.subckt ca3080 inm inp out iabc vcc vee
Qd iabc iabc vee qn3
Qt et iabc vee qn3
Qa ca inm et qn3
Qb cb inp et qn3
QP2a pba pba vcc qpl
QP1a ca pba vcc qpl
QP3a xa ca pba qpl
QP2b pbb pbb vcc qpl
QP1b cb pbb vcc qpl
QP3b out cb pbb qpl
QN2 nb nb vee qn3
QN1 xa nb vee qn3
QN3 out xa nb qn3
.ends
";

/// The whole VCF: the input, FREQ's tail current and CI5's I_abc from
/// `in.txt`, `ic.txt` and `ir.txt` in `dir` (µA for the currents); OUT,
/// OUT 1 and OUT 4 to `out.txt`, Gear-2 at `UP` × 48 kHz.
pub fn netlist(c: &Kobol, dir: &Path, t_end: f64) -> String {
    let f = |n: &str| dir.join(n).display().to_string();
    let mut s = String::new();
    let _ = writeln!(
        s,
        "* RSF Kobol Expander VCF, transistor level (chimera lab)"
    );
    for (node, file) in [("vin", "in.txt"), ("icn", "ic.txt"), ("irn", "ir.txt")] {
        let _ = writeln!(s, "a_{node} %v([{node}]) src_{node}");
        let _ = writeln!(
            s,
            ".model src_{node} filesource (file=\"{}\" amploffset=[0] amplscale=[1] \
             timeoffset=0 timescale=1 timerelative=false amplstep=false)",
            f(file)
        );
    }
    s.push_str(MODELS);
    s.push_str(CA3080);
    let _ = writeln!(s, "Vcc vcc 0 15\nVee vee 0 -15\nVm vm 0 {}", c.vm);
    // The input: R24 then C16 onto IN 1.
    let _ = writeln!(s, "R24 vin j16 {}\nC16 j16 n1 4.7u", c.rin);
    for i in 1..=4 {
        let _ = writeln!(s, "Q2_{i} cap{i} n{i} e{i} qn\nQ1_{i} m{i} b{i} e{i} qn");
        let _ = writeln!(s, "Vos{i} b{i} 0 {}", c.vos[i - 1]);
        let _ = writeln!(s, "Bt{i} e{i} vee I = v(icn)*1e-6");
        let _ = writeln!(
            s,
            "Q3_{i} m{i} m{i} vcc qp\nQ4_{i} cap{i} m{i} vcc qp\nDc{i} cap{i} vm dcl"
        );
        let _ = writeln!(s, "C{i} cap{i} 0 {}", c.c);
        let _ = writeln!(
            s,
            "M5_{i} vcc cap{i} o{i} o{i} nf\nIs{i} o{i} vee {}",
            c.isink
        );
        let _ = writeln!(s, "Rf{i} o{i} n{i} {}\nRa{i} n{i} 0 {}", c.r, c.ra);
        if i > 1 {
            let _ = writeln!(s, "Ri{i} o{} n{i} {}", i - 1, c.r);
        }
        // The cell's resting point, so the DC solve starts off the rails.
        let _ = writeln!(
            s,
            ".ic v(cap{i})=0.65 v(o{i})=0 v(n{i})=0 v(m{i})=14.35 v(e{i})=-0.55"
        );
    }
    // The resonance: C6, R37/R38, CI5 into IN 1.
    let _ = writeln!(s, "C6 o4 c6 4.7u\nR37 c6 p5 {}\nR38 p5 0 {}", c.r37, c.r38);
    let _ = writeln!(s, "Vo5 q5 0 {}", c.vos5);
    let _ = writeln!(s, "X5 p5 q5 n1 ia5 vcc vee ca3080");
    let _ = writeln!(s, "Bi5 0 ia5 I = v(irn)*1e-6");
    // The output: R39/R40, CI3, CI2A with R43.
    let _ = writeln!(
        s,
        "R39 c6 ci3p {}\nR40 ci3p 0 {}\nR41 ci3m 0 220\nVo3 ci3mx ci3m {}",
        c.r39, c.r40, c.vos3
    );
    let _ = writeln!(
        s,
        "X3 ci3mx ci3p v3 ia3 vcc vee ca3080\nI3 0 ia3 {}",
        c.iabc3
    );
    let _ = writeln!(s, "R43 v3 ov 100k\nE2 ov 0 0 v3 1e5");
    let step = 1.0 / (FS * UP as f64);
    let _ = writeln!(
        s,
        ".options method=gear reltol=1e-5 abstol=1e-13 vntol=1e-7 temp=27 tnom=27 interp"
    );
    let _ = writeln!(
        s,
        ".control\ntran {step:e} {t_end:e} 0 {:e} uic\nwrdata {} v(ov) v(o1) v(o4)\n.endc\n.end",
        step / 4.0,
        f("out.txt")
    );
    s
}

/// The columns of an ngspice `wrdata` file (`t v t v …`): each row's
/// values, the times dropped.
fn wrdata(path: &Path) -> Result<Vec<Vec<f64>>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .map(|l| {
            l.split_whitespace()
                .filter_map(|t| t.parse().ok())
                .skip(1)
                .step_by(2)
                .collect::<Vec<f64>>()
        })
        .filter(|v| !v.is_empty())
        .collect())
}

/// ngspice's render of `case` in `dir` (kept, keyed on its inputs: a
/// second call on the same case reads the last run back).
pub fn spice(c: &Kobol, case: &Case, dir: &Path) -> Result<Out, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dt = 1.0 / (FS * UP as f64);
    let x = resample_poly(&case.x, UP, 1);
    let ic: Vec<f64> = lerp_up(&case.fc, UP)
        .iter()
        .map(|&f| c.ic_of(f) * 1e6)
        .collect();
    let ir: Vec<f64> = lerp_up(&case.k, UP)
        .iter()
        .map(|&k| c.iabc5_of(k) * 1e6)
        .collect();
    let t_end = (x.len() - 1) as f64 * dt;
    let net = netlist(c, dir, t_end);
    write_series(&dir.join("in.txt"), &x, dt)?;
    write_series(&dir.join("ic.txt"), &ic, dt)?;
    write_series(&dir.join("ir.txt"), &ir, dt)?;
    run(
        dir,
        "kobol.cir",
        &net,
        &stamp(x.iter().chain(&ic).chain(&ir)),
    )?;
    let rows: Vec<Vec<f64>> = wrdata(&dir.join("out.txt"))?
        .into_iter()
        .filter(|r| r.len() >= 3)
        .collect();
    let dec = |j: usize| {
        let v: Vec<f64> = rows.iter().map(|r| r[j]).collect();
        let mut d = resample_poly(&v, 1, UP);
        d.resize(case.x.len(), 0.0);
        d
    };
    Ok(Out {
        y: dec(0),
        o1: dec(1),
        o4: dec(2),
    })
}

/// One 2040 cell wired as stage 1 (R24 in, R27/R26, a 10 K load), its
/// tail at FREQ `fc`, swept at DC from −30 to +30 V at the input: the
/// curve EXP-C's swing is fitted to (`spice/dccell`). `v(o)` to `out.txt`
/// in `dir`.
pub fn dc_cell_netlist(c: &Kobol, fc: f64, dir: &Path) -> String {
    let mut s = String::from("* one 2040 cell as stage 1, DC\n");
    s.push_str(MODELS);
    let _ = writeln!(
        s,
        "Vcc vcc 0 15\nVee vee 0 -15\nVm vm 0 {}\nVin vin 0 0\nR24 vin n1 {}",
        c.vm, c.rin
    );
    let _ = writeln!(
        s,
        "Q2 cap n1 e qn\nQ1 m b e qn\nVos b 0 0\nIt e vee {:e}",
        c.ic_of(fc)
    );
    let _ = writeln!(
        s,
        "Q3 m m vcc qp\nQ4 cap m vcc qp\nDc cap vm dcl\nM5 vcc cap o o nf\nIs o vee {:e}",
        c.isink
    );
    let _ = writeln!(s, "Rf o n1 {}\nRa n1 0 {}\nRl o 0 10k", c.r, c.ra);
    s.push_str(".nodeset v(cap)=0.65 v(o)=0\n");
    let _ = writeln!(
        s,
        ".control\ndc Vin -30 30 0.05\nwrdata {} v(o)\n.endc\n.end",
        dir.join("out.txt").display()
    );
    s
}

/// The cell's DC curve at FREQ `fc`: (OUT unlimited, `−V_in·R27/R24`;
/// OUT), from `dc_cell_netlist` run in `dir`.
pub fn dc_cell(c: &Kobol, fc: f64, dir: &Path) -> Result<Vec<(f64, f64)>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    run(
        dir,
        "cell.cir",
        &dc_cell_netlist(c, fc, dir),
        &format!("{fc}"),
    )?;
    let text = std::fs::read_to_string(dir.join("out.txt")).map_err(|e| e.to_string())?;
    Ok(text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace().filter_map(|t| t.parse::<f64>().ok());
            Some((-it.next()? * c.r / c.rin, it.next()?))
        })
        .collect())
}
