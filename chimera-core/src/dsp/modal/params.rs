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

/// Home: MODEL and the four macros every model but BOWED reads its own
/// way. The rest is the model page (MDL2).
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
}

/// MODEL's names, by `ResonatorMode as u8`.
pub const MODEL_NAMES: [&str; 4] = ["STRING", "BANK", "BOWED", "SYMP"];

/// The old loop's gain per pass at `decay`, rung at C3, as DAMP: old
/// string patches keep their ring time. The one v1 DECAY → DAMP map on
/// STRING, SYMP and BOWED; BANK's DAMP is its DECAY.
pub fn damp_from_v1_decay(decay: f32) -> f32 {
    const C3_HZ: f32 = 130.81;
    let g = 0.999 - 0.009 * decay;
    let t60 = -3.0 / (C3_HZ * libm::log10f(g));
    (libm::logf(t60 / 0.05) / libm::logf(400.0)).clamp(0.0, 1.0)
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

/// Read at note-on or per block from the unmodulated snapshot: none are
/// modulatable yet. Retired ids 2, 5, 7 and 8 are never reused.
pub static MODAL_SPECS: [ParamSpec; 13] = [
    ParamSpec::choice(0, "MODEL", ValFmt::Names(&MODEL_NAMES), 3.0, 0.0).ident("MODE"),
    unit(13, "STRUCT", 0.0).short("STR").ident("STRUCTURE"),
    unit(3, "BRIGHT", 1.0 - 0.7).short("BRT").ident("BRIGHT"),
    unit(12, "DAMP", INIT_DAMP).short("DMP").ident("DAMP"),
    unit(4, "POS", 0.0).short("POS").ident("POS"),
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
];

/// Whether `mode` reads `id`: the one table for page cells, dimming and
/// the audio test.
pub fn reads(mode: ResonatorMode, id: ParamId) -> bool {
    use ResonatorMode::{Bowed, Modal as Bank, String, Sympathetic as Symp};
    type P = ModalParams;
    match id {
        P::MODE => true,
        P::STRUCTURE | P::BRIGHT | P::DAMP | P::POS | P::EXCITE => mode != Bowed,
        P::BODY | P::ENS_DEPTH | P::ENS_MIX => matches!(mode, String | Symp),
        P::ENS_RATE => mode == String,
        P::COUPLE | P::HALO => mode == Symp,
        P::MODES => mode == Bank,
        _ => false,
    }
}

/// MDL2's six cells for `mode`.
pub fn page_cells(mode: ResonatorMode) -> [Option<ParamId>; 6] {
    type P = ModalParams;
    match mode {
        ResonatorMode::String => [
            Some(P::EXCITE),
            Some(P::BODY),
            Some(P::ENS_DEPTH),
            Some(P::ENS_RATE),
            Some(P::ENS_MIX),
            None,
        ],
        ResonatorMode::Sympathetic => [
            Some(P::EXCITE),
            Some(P::COUPLE),
            Some(P::HALO),
            Some(P::BODY),
            Some(P::ENS_DEPTH),
            Some(P::ENS_MIX),
        ],
        ResonatorMode::Modal => [Some(P::EXCITE), Some(P::MODES), None, None, None, None],
        ResonatorMode::Bowed => [None; 6],
    }
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

    #[test]
    fn page_cells_are_what_the_model_reads() {
        for mode in MODES {
            let cells = page_cells(mode);
            for id in cells.iter().flatten() {
                assert!(reads(mode, *id), "{mode:?} shows {id:?}, unread");
            }
            for s in &MODAL_SPECS {
                let home = s.id == ModalParams::MODE || MACROS.contains(&s.id);
                if !home && reads(mode, s.id) {
                    assert!(cells.contains(&Some(s.id)), "{mode:?} hides {}", s.label);
                }
            }
        }
    }

    /// DAMP's T60 on a string, as `super::super::t60`.
    fn t60(damp: f32) -> f32 {
        0.05 * libm::powf(400.0, damp)
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

    #[test]
    fn macros_are_dimmed_only_on_bowed() {
        for id in MACROS {
            for mode in MODES {
                assert_eq!(
                    reads(mode, id),
                    mode != ResonatorMode::Bowed,
                    "{mode:?} {id:?}"
                );
            }
        }
    }
}
