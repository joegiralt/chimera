//! Modal's parameters and their specs.

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};

// ── Modal Params ────────────────────────────────────────────────────

/// Resonator model selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ResonatorMode {
    String = 0,      // KS+ (body, stiffness, position, ensemble)
    Modal = 1,       // SVF bandpass bank (Rings-style)
    Bowed = 2,       // Sustained bow friction
    Sympathetic = 3, // Multiple resonating strings (Rings-style)
}

impl DiskCode for ResonatorMode {
    fn disk_code(self) -> u8 {
        match self {
            ResonatorMode::String => 0,
            ResonatorMode::Modal => 1,
            ResonatorMode::Bowed => 2,
            ResonatorMode::Sympathetic => 3,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            ResonatorMode::String => "STRING",
            ResonatorMode::Modal => "MODAL",
            ResonatorMode::Bowed => "BOWED",
            ResonatorMode::Sympathetic => "SYMPATHETIC",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(ResonatorMode::String),
            1 => Some(ResonatorMode::Modal),
            2 => Some(ResonatorMode::Bowed),
            3 => Some(ResonatorMode::Sympathetic),
            _ => None,
        }
    }
}

impl ResonatorMode {
    pub fn from_u8(v: u8) -> Self {
        match v % 4 {
            0 => ResonatorMode::String,
            1 => ResonatorMode::Modal,
            2 => ResonatorMode::Bowed,
            _ => ResonatorMode::Sympathetic,
        }
    }
}

/// The bank's size: MODES, the billed cost with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankModes {
    M16,
    M24,
    M32,
    M48,
}

impl BankModes {
    const ALL: [Self; 4] = [Self::M16, Self::M24, Self::M32, Self::M48];

    /// Modes rung: even, at most `MAX_MODES`.
    pub const fn count(self) -> usize {
        match self {
            Self::M16 => 16,
            Self::M24 => 24,
            Self::M32 => 32,
            Self::M48 => 48,
        }
    }

    /// By the choice's value; past the end, the largest.
    pub fn from_index(v: u8) -> Self {
        Self::ALL[usize::from(v).min(Self::ALL.len() - 1)]
    }
}

const _: () = assert!(BankModes::M48.count() <= super::MAX_MODES);

impl DiskCode for BankModes {
    fn disk_code(self) -> u8 {
        match self {
            Self::M16 => 0,
            Self::M24 => 1,
            Self::M32 => 2,
            Self::M48 => 3,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Self::M16 => "M16",
            Self::M24 => "M24",
            Self::M32 => "M32",
            Self::M48 => "M48",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(Self::M16),
            1 => Some(Self::M24),
            2 => Some(Self::M32),
            3 => Some(Self::M48),
            _ => None,
        }
    }
}

/// Home: MODEL and the four macros each model reads its own way (BOWED
/// all but STRUCTURE). Then the model page (MDL2), and the exciter's (EXC).
#[derive(Clone, Copy, Debug)]
pub struct ModalParams {
    pub mode: ResonatorMode,
    pub structure: f32,
    /// 1 is brightest.
    pub bright: f32,
    /// 1 rings longest.
    pub damp: f32,
    pub pos: f32,
    pub excite: f32,
    pub body: f32,
    pub ens_depth: f32,
    pub ens_rate: f32,
    pub ens_mix: f32,
    /// How hard SYMP's main string drives its halo.
    pub couple: f32,
    /// SYMP's halo level.
    pub halo: f32,
    pub modes: BankModes,
    /// PLUCK: the noise's smoothing, 1 brightest.
    pub color: f32,
    /// STRIKE: the burst's length.
    pub burst: f32,
    /// BOW: pressure, read every block.
    pub force: f32,
    /// BOW: velocity, read every block.
    pub speed: f32,
}

impl Default for ModalParams {
    fn default() -> Self {
        Self {
            mode: ResonatorMode::String,
            structure: 0.0,
            // Today's INIT tone and ring, in the new direction.
            bright: 1.0 - 0.7,
            damp: damp_from_v1_decay(0.3),
            pos: 0.0,
            excite: 0.8,
            body: 0.3,
            ens_depth: 0.0,
            ens_rate: 0.3,
            ens_mix: 0.0,
            couple: 0.25,
            halo: 0.25,
            modes: BankModes::M32,
            color: 0.8,
            burst: 0.8,
            force: 0.5,
            speed: 0.5,
        }
    }
}

impl ModalParams {
    pub const MODE: ParamId = ParamId(0);
    pub const EXCITE: ParamId = ParamId(1);
    pub const BRIGHT: ParamId = ParamId(3);
    pub const POS: ParamId = ParamId(4);
    pub const BODY: ParamId = ParamId(6);
    pub const ENS_DEPTH: ParamId = ParamId(9);
    pub const ENS_RATE: ParamId = ParamId(10);
    pub const ENS_MIX: ParamId = ParamId(11);
    pub const DAMP: ParamId = ParamId(12);
    pub const STRUCTURE: ParamId = ParamId(13);
    pub const COUPLE: ParamId = ParamId(14);
    pub const HALO: ParamId = ParamId(15);
    pub const MODES: ParamId = ParamId(16);
    pub const FORCE: ParamId = ParamId(17);
    pub const SPEED: ParamId = ParamId(18);
    pub const COLOR: ParamId = ParamId(19);
    pub const BURST: ParamId = ParamId(20);
}

/// The four macros as the loops play them: eased toward the block's
/// modulated values by `EASE` a block, snapped at a note's first block.
#[derive(Clone, Copy)]
pub(super) struct Macros {
    pub structure: f32,
    pub bright: f32,
    pub damp: f32,
    pub pos: f32,
}

/// The share of the way to the block's value each block eases.
pub(super) const EASE: f32 = 0.3;

impl Macros {
    pub fn of(p: &ModalParams) -> Self {
        Self {
            structure: p.structure,
            bright: p.bright,
            damp: p.damp,
            pos: p.pos,
        }
    }

    pub fn ease(&mut self, to: &Self) {
        for (x, t) in [
            (&mut self.structure, to.structure),
            (&mut self.bright, to.bright),
            (&mut self.damp, to.damp),
            (&mut self.pos, to.pos),
        ] {
            *x += EASE * (t - *x);
        }
    }
}

/// MODEL's names, by `ResonatorMode as u8`.
pub const MODEL_NAMES: [&str; 4] = ["STRING", "BANK", "BOWED", "SYMP"];

/// EXC's header by MODEL, by `ResonatorMode as u8`.
pub const EXCITER_NAMES: [&str; 4] = ["PLUCK", "STRIKE", "BOW", "PLUCK"];

/// Which of MODEL's two pages a cell list is for: EXC or the model page (MDL2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalPage {
    Exciter,
    Model,
}

/// The bow's force at a note-on: FORCE × (0.5 + 0.5 × velocity), velocity
/// in 0..=1. A soft key still bows.
pub fn bow_force(force: f32, vel: f32) -> f32 {
    force * (0.5 + 0.5 * vel)
}

/// POS 1's β, the pluck's, strike's or bow's place as a fraction of the
/// string from its end: the middle. A pluck at β and at 1 − β is the
/// same, so the travel stops here.
pub(super) const BETA_MAX: f32 = 0.5;
/// POS 0's β on a pluck or strike: the end, where the place shapes
/// nothing, as POS 0 always has.
pub(super) const END: f32 = 0.0;
/// POS 0's β on a bow: near the bridge, above Schelleng's floor.
pub(super) const BOW_END: f32 = 0.06;

/// POS's β, from `end` at 0 to the middle at 1.
pub(super) fn beta(pos: f32, end: f32) -> f32 {
    // Not `clamp`, which passes NaN.
    let pos = if pos >= 0.0 { pos.min(1.0) } else { 0.0 };
    end + (BETA_MAX - end) * pos
}

/// DAMP's law on a string: T60 from `T60_MIN` at 0, ×`T60_SPAN` at 1.
const T60_MIN: f32 = 0.05;
const T60_SPAN: f32 = 400.0;

/// DAMP's T60 on a string, in seconds: 0.05 at 0 to 20 at 1.
pub(super) fn t60(damp: f32) -> f32 {
    T60_MIN * libm::powf(T60_SPAN, damp)
}

/// `t60`'s inverse, held to DAMP's range.
pub fn damp_for(t60_s: f32) -> f32 {
    (libm::logf(t60_s / T60_MIN) / libm::logf(T60_SPAN)).clamp(0.0, 1.0)
}

/// The old loop's gain per pass at `decay`, rung at C3, as DAMP: old
/// string patches keep their ring time. The one v1 DECAY → DAMP map on
/// STRING, SYMP and BOWED; BANK's DAMP is its DECAY.
pub fn damp_from_v1_decay(decay: f32) -> f32 {
    const C3_HZ: f32 = 130.81;
    let g = 0.999 - 0.009 * decay;
    damp_for(-3.0 / (C3_HZ * libm::log10f(g)))
}

/// A v1 Modal block into today's params (spec § 3), after its live values
/// are written: DECAY → DAMP, STIFF or INHARM → STRUCTURE, FDBK dropped.
/// The string models' BRIGHT flips to the new direction. BANK's BURST is
/// its EXCITE, the old strike's length. Bowed never read BRIGHT, DAMP or
/// POS, and its old sound was the bug (#240): it loads an in-tune bow, an
/// eighth of the string from the bridge, whatever the file held.
pub fn translate_v1(old: &crate::storage::Retired, blk: &mut dyn Block) {
    const DECAY: ParamId = ParamId(2);
    const INHARM: ParamId = ParamId(5);
    const STIFF: ParamId = ParamId(7);
    type P = ModalParams;
    let mode = blk
        .enum_code(P::MODE)
        .and_then(ResonatorMode::from_disk_code);
    let bank = mode == Some(ResonatorMode::Modal);
    if let Some(d) = old.get(DECAY) {
        blk.set(P::DAMP, if bank { d } else { damp_from_v1_decay(d) });
    }
    let structure = match mode {
        Some(ResonatorMode::Modal | ResonatorMode::Sympathetic) => INHARM,
        _ => STIFF,
    };
    if let Some(s) = old.get(structure) {
        blk.set(P::STRUCTURE, s);
    }
    if !bank {
        blk.set(P::BRIGHT, 1.0 - blk.get(P::BRIGHT));
    }
    if bank {
        blk.set(P::BURST, blk.get(P::EXCITE));
    }
    if mode == Some(ResonatorMode::Bowed) {
        blk.set(P::DAMP, damp_for(0.5));
        blk.set(P::BRIGHT, 0.5);
        blk.set(P::POS, 0.15);
    }
}

/// `damp_from_v1_decay(0.3)`, for the const spec table: a test pins it.
const INIT_DAMP: f32 = 0.943_377_4;

const fn unit(id: u8, label: &'static str, default: f32) -> ParamSpec {
    ParamSpec::continuous(
        id,
        label,
        ValFmt::Uni,
        0.0,
        1.0,
        default,
        1.0 / 128.0,
        false,
    )
}

/// A home macro: read every block from the modulated params.
const fn macro_(id: u8, label: &'static str, default: f32) -> ParamSpec {
    let mut s = unit(id, label, default);
    s.modulatable = true;
    s
}

/// The four macros are modulatable (`Macros`). FORCE and SPEED are read
/// every block and eased, not modulatable; the rest of the model and
/// exciter pages is read at note-on. Retired ids 2, 5, 7 and 8 are never
/// reused.
pub static MODAL_SPECS: [ParamSpec; 17] = [
    ParamSpec::choice(0, "MODEL", ValFmt::Names(&MODEL_NAMES), 3.0, 0.0).ident("MODE"),
    macro_(13, "STRUCT", 0.0).short("STR").ident("STRUCTURE"),
    macro_(3, "BRIGHT", 1.0 - 0.7).short("BRT").ident("BRIGHT"),
    macro_(12, "DAMP", INIT_DAMP).short("DMP").ident("DAMP"),
    macro_(4, "POS", 0.0).short("POS").ident("POS"),
    unit(1, "EXCITE", 0.8).ident("EXCITE"),
    unit(6, "BODY", 0.3).ident("BODY"),
    unit(9, "ENS.D", 0.0).ident("E.DPT"),
    unit(10, "ENS.R", 0.3).ident("E.RAT"),
    unit(11, "ENS.M", 0.0).ident("E.MIX"),
    unit(14, "COUPLE", 0.25).ident("COUPLE"),
    unit(15, "HALO", 0.25).ident("HALO"),
    ParamSpec::choice(
        16,
        "MODES",
        ValFmt::Names(&["16", "24", "32", "48"]),
        3.0,
        2.0,
    )
    .ident("MODES"),
    unit(19, "COLOR", 0.8).ident("COLOR"),
    unit(20, "BURST", 0.8).ident("BURST"),
    unit(17, "FORCE", 0.5).ident("FORCE"),
    unit(18, "SPEED", 0.5).ident("SPEED"),
];

/// Whether `mode` reads `id`, at note-on or every block: the one table
/// for page cells, dimming and the audio test.
pub fn reads(mode: ResonatorMode, id: ParamId) -> bool {
    use ResonatorMode::{Bowed, Modal as Bank, String, Sympathetic as Symp};
    type P = ModalParams;
    match id {
        P::MODE => true,
        P::BRIGHT | P::DAMP | P::POS => true,
        P::STRUCTURE | P::EXCITE => mode != Bowed,
        P::COLOR => matches!(mode, String | Symp),
        P::BURST => mode == Bank,
        P::FORCE | P::SPEED => mode == Bowed,
        P::BODY | P::ENS_DEPTH | P::ENS_MIX => matches!(mode, String | Symp),
        P::ENS_RATE => mode == String,
        P::COUPLE | P::HALO => mode == Symp,
        P::MODES => mode == Bank,
        _ => false,
    }
}

/// `page`'s six cells for `mode`, in order. BOW's FORCE and SPEED move a
/// held note; the other EXC and MDL2 cells, the next.
pub fn page_cells(page: ModalPage, mode: ResonatorMode) -> [Option<ParamId>; 6] {
    use ResonatorMode::{Bowed, Modal as Bank, String, Sympathetic as Symp};
    type P = ModalParams;
    let ids: &[ParamId] = match (page, mode) {
        (ModalPage::Exciter, String | Symp) => &[P::EXCITE, P::COLOR],
        (ModalPage::Exciter, Bank) => &[P::EXCITE, P::BURST],
        (ModalPage::Exciter, Bowed) => &[P::FORCE, P::SPEED],
        (ModalPage::Model, String) => &[P::BODY, P::ENS_DEPTH, P::ENS_RATE, P::ENS_MIX],
        (ModalPage::Model, Symp) => &[P::COUPLE, P::HALO, P::BODY, P::ENS_DEPTH, P::ENS_MIX],
        (ModalPage::Model, Bank) => &[P::MODES],
        (ModalPage::Model, Bowed) => &[],
    };
    core::array::from_fn(|k| ids.get(k).copied())
}

impl Block for ModalParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &MODAL_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODE => self.mode as u8 as f32,
            Self::STRUCTURE => self.structure,
            Self::BRIGHT => self.bright,
            Self::DAMP => self.damp,
            Self::POS => self.pos,
            Self::EXCITE => self.excite,
            Self::BODY => self.body,
            Self::ENS_DEPTH => self.ens_depth,
            Self::ENS_RATE => self.ens_rate,
            Self::ENS_MIX => self.ens_mix,
            Self::COUPLE => self.couple,
            Self::HALO => self.halo,
            Self::MODES => self.modes as u8 as f32,
            Self::COLOR => self.color,
            Self::BURST => self.burst,
            Self::FORCE => self.force,
            Self::SPEED => self.speed,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODE => self.mode = ResonatorMode::from_u8(v as u8),
            Self::STRUCTURE => self.structure = v,
            Self::BRIGHT => self.bright = v,
            Self::DAMP => self.damp = v,
            Self::POS => self.pos = v,
            Self::EXCITE => self.excite = v,
            Self::BODY => self.body = v,
            Self::ENS_DEPTH => self.ens_depth = v,
            Self::ENS_RATE => self.ens_rate = v,
            Self::ENS_MIX => self.ens_mix = v,
            Self::COUPLE => self.couple = v,
            Self::HALO => self.halo = v,
            Self::MODES => self.modes = BankModes::from_index(v as u8),
            Self::COLOR => self.color = v,
            Self::BURST => self.burst = v,
            Self::FORCE => self.force = v,
            Self::SPEED => self.speed = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::MODE => Some(self.mode.disk_code()),
            Self::MODES => Some(self.modes.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::MODE => Some(self.mode.disk_ident()),
            Self::MODES => Some(self.modes.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::MODE => apply_code(ResonatorMode::from_disk_code(code), |m| self.mode = m),
            Self::MODES => apply_code(BankModes::from_disk_code(code), |m| self.modes = m),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    const MODES: [ResonatorMode; 4] = [
        ResonatorMode::String,
        ResonatorMode::Modal,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ];
    const MACROS: [ParamId; 4] = [
        ModalParams::STRUCTURE,
        ModalParams::BRIGHT,
        ModalParams::DAMP,
        ModalParams::POS,
    ];

    const PAGES: [ModalPage; 2] = [ModalPage::Exciter, ModalPage::Model];

    #[test]
    fn page_cells_are_what_the_model_reads() {
        for mode in MODES {
            for page in PAGES {
                for id in page_cells(page, mode).iter().flatten() {
                    assert!(reads(mode, *id), "{mode:?} {page:?} shows {id:?}, unread");
                }
            }
            for s in &MODAL_SPECS {
                let home = s.id == ModalParams::MODE || MACROS.contains(&s.id);
                if !home && reads(mode, s.id) {
                    let on = PAGES
                        .iter()
                        .filter(|&&pg| page_cells(pg, mode).contains(&Some(s.id)))
                        .count();
                    assert_eq!(on, 1, "{mode:?} {}: on {on} pages", s.label);
                }
            }
        }
    }

    #[test]
    fn the_exciter_page_holds_each_models_exciter() {
        type P = ModalParams;
        let want = |mode| match mode {
            ResonatorMode::String | ResonatorMode::Sympathetic => &[P::EXCITE, P::COLOR][..],
            ResonatorMode::Modal => &[P::EXCITE, P::BURST],
            ResonatorMode::Bowed => &[P::FORCE, P::SPEED],
        };
        for mode in MODES {
            let cells = page_cells(ModalPage::Exciter, mode);
            let got: std::vec::Vec<_> = cells.iter().flatten().copied().collect();
            assert_eq!(got.as_slice(), want(mode), "{mode:?}");
            assert!(cells[got.len()..].iter().all(Option::is_none), "{mode:?}");
            for id in page_cells(ModalPage::Model, mode).iter().flatten() {
                assert!(
                    ![P::EXCITE, P::COLOR, P::BURST, P::FORCE, P::SPEED].contains(id),
                    "{mode:?}: MDL2 shows {id:?}"
                );
            }
        }
    }

    #[test]
    fn old_decay_keeps_its_ring_time_at_c3() {
        for (decay, want) in [(0.3, 14.2), (0.6, 8.2)] {
            let got = t60(damp_from_v1_decay(decay));
            assert!((got / want - 1.0).abs() < 0.05, "DECAY {decay}: {got} s");
        }
        let mut last = f32::INFINITY;
        for i in 0..=100 {
            let d = damp_from_v1_decay(i as f32 / 100.0);
            assert!((0.0..=1.0).contains(&d) && d <= last, "DECAY {i}%: {d}");
            last = d;
        }
    }

    #[test]
    fn init_damp_is_old_decay_0_3() {
        let init = damp_from_v1_decay(0.3);
        assert_eq!(ModalParams::default().damp, init);
        let spec = MODAL_SPECS
            .iter()
            .find(|s| s.id == ModalParams::DAMP)
            .unwrap();
        assert_eq!(spec.default, init);
    }

    /// Each block eases `EASE` of the way; at its target, no bit moves.
    #[test]
    fn macros_ease_a_share_a_block() {
        let p = ModalParams::default();
        let mut m = Macros::of(&p);
        let held = m;
        m.ease(&Macros::of(&p));
        assert_eq!(m.damp.to_bits(), held.damp.to_bits());
        let to = Macros {
            pos: 1.0,
            ..Macros::of(&p)
        };
        m.ease(&to);
        assert!((m.pos - EASE).abs() < 1e-6, "{}", m.pos);
        for _ in 0..40 {
            m.ease(&to);
        }
        assert!((m.pos - 1.0).abs() < 1e-5, "{}", m.pos);
    }

    #[test]
    fn bowed_reads_three_macros() {
        for mode in MODES {
            for id in [ModalParams::BRIGHT, ModalParams::DAMP, ModalParams::POS] {
                assert!(reads(mode, id), "{mode:?} {id:?}");
            }
            assert_eq!(
                reads(mode, ModalParams::STRUCTURE),
                mode != ResonatorMode::Bowed,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn bow_force_scales_with_velocity() {
        assert_eq!(bow_force(0.5, 1.0).to_bits(), 0.5f32.to_bits());
        assert_eq!(bow_force(0.5, 0.0), 0.25);
        assert_eq!(bow_force(0.0, 1.0), 0.0);
        assert!(bow_force(1.0, 20.0 / 127.0) > 0.5);
    }
}
