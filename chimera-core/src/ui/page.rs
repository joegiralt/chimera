use crate::block::{Block, ParamId};
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::pizza::PizzaParams;
use crate::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FmParams, FolderParams, OutParams, ParamSnapshot};
use crate::preset::ChainType;
use crate::ui::chain::ChainNav;

pub use crate::block::ValFmt;

/// Layout mode for a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageLayout {
    /// One large visualization + 3x2 parameter grid below.
    BigViz,
    /// 3x2 grid of independent cells, each with its own mini icon.
    CellGrid,
    /// Mod matrix grid — source×destination routing table.
    Matrix,
}

/// What mini icon to draw in a cell (CellGrid mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellIcon {
    /// No icon — just label + value + bar.
    None,
    /// Waveform morphing: sine -> clipped (drive)
    WaveClip,
    /// Tone/EQ tilt indicator
    ToneTilt,
    /// Dry/wet blend arc
    DryWet,
    /// Waveform selector: saw / square / tri
    WaveShape,
    /// Pulse width bar
    PulseWidth,
    /// Knob arc indicator
    Arc,
    /// Vertical level bar
    LevelBar,
    /// Pan dot on L-R line
    PanDot,
    /// Fold: sine getting folded
    WaveFold,
    /// Symmetry bias indicator
    Symmetry,
    /// Concentric ripples expanding from center (reverb, decay)
    Ripple,
    /// Rays bursting from center point (excite, attack)
    Burst,
    /// Dot orbiting center (LFO rate, modulation)
    Orbit,
    /// Dots scattering outward from center (diffusion, spread)
    Scatter,
    /// Breathing circle — pulsing size (depth, amount)
    Breathe,
    /// Horizontal lines stacking up (density, voices)
    Stack,
    /// Bouncing ball: compressed at extremes, round at center (bipolar)
    Bounce,
    /// Isometric 3D cube that fills from bottom to top
    Cube,
    /// FM algorithm topology diagram (8 algorithms, val selects which)
    FmAlgorithm,
    /// Feedback: circular arrow that tightens with value
    FeedbackLoop,
    /// Feedback: spiral expanding outward
    FeedbackSpiral,
    /// Feedback: sine getting progressively distorted
    FeedbackWave,
}

/// Identifies which page is active, derived from chain position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
    Pizza,
    EngineModal1,
    EngineModal2,
    Drive,
    Filter,
    Folder,
    Vca,
    Efx,
    Mixer,
    Chorus,
    Delay,
    MixReverb,
    Master,
    EnvAmp,
    EnvFilter,
    EnvAux,
    Lfo,
    FmAlg,
    FmOp,
    FmRatio,
    FmEnv1,
    FmEnv2,
    FmEnv3,
    FmEnv4,
    DemoWaves,
    DemoShapes,
    DemoMotion,
    DemoFm,
    DemoMatrix,
}

impl PageId {
    /// Resolve which page is active from current navigation position.
    pub fn from_nav(nav: &ChainNav) -> Self {
        use crate::ui::chain::ChainId;
        match nav.chain_id {
            ChainId::Part(_) => Self::from_part_nav(nav.chain_type, nav.node, nav.sub_page),
            ChainId::Mixer(_) => match nav.node {
                0 => PageId::Mixer,
                1 => PageId::Chorus,
                2 => PageId::Delay,
                3 => PageId::MixReverb,
                _ => PageId::Master,
            },
            ChainId::System => PageId::Pizza, // no param editing yet
            ChainId::Demo => match nav.node {
                0 => PageId::DemoWaves,
                1 => PageId::DemoShapes,
                2 => PageId::DemoMotion,
                3 => PageId::DemoFm,
                _ => PageId::DemoMatrix,
            },
        }
    }

    /// Resolve Part navigation to a PageId based on chain type.
    fn from_part_nav(chain_type: ChainType, node: usize, sub_page: usize) -> Self {
        match chain_type {
            ChainType::PizzaPoly => match node {
                0 => PageId::Pizza,
                1 => PageId::Drive,
                2 => PageId::Filter,
                3 => PageId::Folder,
                4 => match sub_page {
                    0 => PageId::DemoMatrix, // Mod matrix grid
                    1 => PageId::Vca,        // Envelope (ADSR)
                    _ => PageId::Lfo,        // LFO
                },
                _ => PageId::Efx,
            },
            ChainType::Modal => match node {
                0 => match sub_page {
                    0 => PageId::EngineModal1,
                    _ => PageId::EngineModal2,
                },
                1 => PageId::Filter,
                2 => match sub_page {
                    0 => PageId::DemoMatrix,
                    1 => PageId::Vca,
                    _ => PageId::Lfo,
                },
                _ => PageId::Efx,
            },
            ChainType::Fm => match node {
                0 => match sub_page {
                    0 => PageId::FmAlg,
                    1 => PageId::FmOp,
                    _ => PageId::FmRatio,
                },
                1 => PageId::Drive,
                2 => PageId::Filter,
                3 => PageId::Folder,
                4 => match sub_page {
                    0 => PageId::DemoMatrix,
                    1 => PageId::FmEnv1,
                    2 => PageId::FmEnv2,
                    3 => PageId::FmEnv3,
                    _ => PageId::FmEnv4,
                },
                _ => PageId::Efx,
            },
        }
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &ParamSnapshot) -> [f32; 6] {
        match self {
            PageId::Pizza => read_block(&params.pizza, PIZZA_PAGE),
            PageId::Filter => read_block(&params.filter, FILTER_PAGE),
            PageId::EnvAmp | PageId::Vca => read_block(&params.envelopes[0], ENV_PAGE),
            PageId::EnvFilter => read_block(&params.envelopes[1], ENV_PAGE),
            PageId::EnvAux => read_block(&params.envelopes[2], ENV_PAGE),
            PageId::Mixer => [
                params.out.normalized(OutParams::VOLUME),
                params.out.normalized(OutParams::PAN),
                0.5,
                0.0,
                0.5,
                0.0, // placeholders
            ],
            PageId::Drive => read_block(&params.drive, DRIVE_PAGE),
            PageId::Folder => read_block(&params.folder, FOLDER_PAGE),
            // Demo pages reuse filter + envelope params for tweaking
            PageId::DemoWaves => [
                params.drive.normalized(DriveParams::DRIVE),
                params.drive.normalized(DriveParams::TONE),
                params.folder.normalized(FolderParams::FOLD),
                params.folder.normalized(FolderParams::SYMMETRY),
                params.filter.normalized(FilterParams::ENV_AMOUNT),
                params.out.normalized(OutParams::PAN),
            ],
            PageId::DemoShapes => [
                params.out.normalized(OutParams::VOLUME),
                params.filter.normalized(FilterParams::CUTOFF),
                params.out.normalized(OutParams::PAN),
                params.filter.normalized(FilterParams::DRIVE),
                params.filter.normalized(FilterParams::RESONANCE),
                params.filter.normalized(FilterParams::FM_AMOUNT),
            ],
            PageId::DemoMotion => [
                params.envelopes[0].normalized(EnvParams::ATTACK),
                params.envelopes[0].normalized(EnvParams::DECAY),
                params.envelopes[0].normalized(EnvParams::SUSTAIN),
                params.envelopes[0].normalized(EnvParams::RELEASE),
                params.envelopes[1].normalized(EnvParams::ATTACK),
                params.envelopes[1].normalized(EnvParams::DECAY),
            ],
            PageId::Lfo => read_block(&params.lfo, LFO_PAGE),
            PageId::DemoFm => [
                params.fm.normalized(FmParams::ALGORITHM),
                params.fm.operators[0].normalized(FmOpParams::FEEDBACK),
                params.fm.operators[1].normalized(FmOpParams::FEEDBACK),
                params.fm.operators[2].normalized(FmOpParams::FEEDBACK),
                0.0,
                0.0,
            ],
            PageId::DemoMatrix => [0.0; 6],
            PageId::EngineModal1 => read_block(&params.modal, MODAL1_PAGE),
            PageId::EngineModal2 => read_block(&params.modal, MODAL2_PAGE),
            PageId::FmAlg => [
                params.fm.normalized(FmParams::ALGORITHM),
                0.0,
                params.out.normalized(OutParams::VOLUME),
                0.0,
                0.0,
                0.0,
            ],
            PageId::FmOp => {
                let sel = fm_selected_op();
                let mut v = read_block(&params.fm.operators[sel], FM_OP_PAGE);
                v.rotate_right(1); // slot 0 is the operator selector
                v[0] = sel as f32 / 3.0;
                v
            }
            PageId::FmRatio => [
                params.fm.operators[0].normalized(FmOpParams::COARSE),
                params.fm.operators[1].normalized(FmOpParams::COARSE),
                params.fm.operators[2].normalized(FmOpParams::COARSE),
                params.fm.operators[3].normalized(FmOpParams::COARSE),
                params.fm.operators[fm_selected_op()].normalized(FmOpParams::FINE),
                0.0,
            ],
            PageId::FmEnv1 => read_block(&params.fm.operators[0], FM_ENV_PAGE),
            PageId::FmEnv2 => read_block(&params.fm.operators[1], FM_ENV_PAGE),
            PageId::FmEnv3 => read_block(&params.fm.operators[2], FM_ENV_PAGE),
            PageId::FmEnv4 => read_block(&params.fm.operators[3], FM_ENV_PAGE),
            PageId::Chorus => [
                params.chorus.mode as f32 / 3.0,
                params.chorus.rate,
                params.chorus.depth,
                params.chorus.mix,
                0.0,
                0.0,
            ],
            PageId::Delay => [
                params.delay.time_ms / 1000.0,
                params.delay.feedback,
                params.delay.wow_flutter,
                params.delay.saturation,
                params.delay.tone,
                params.delay.mix,
            ],
            PageId::Efx | PageId::MixReverb => [
                params.reverb.reverb_type as f32 / 2.0,
                params.reverb.time,
                params.reverb.damping,
                params.reverb.size,
                params.reverb.mix,
                0.0,
            ],
            PageId::Master => read_block(&params.out, OUT_PAGE),
        }
    }

    /// Apply an encoder delta. Each tick = 1/128 of the parameter range.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if *self == PageId::FmOp && idx == 0 {
            // Operator select: 0-3
            let cur = fm_selected_op() as i16;
            fm_set_selected_op((cur + delta as i16).clamp(0, 3) as u8);
            return;
        }
        if let Some((blk, id)) = self.resolve_mut(idx, params) {
            blk.nudge(id, delta);
            return;
        }
        // Direct float manipulation (not Param structs)
        match self {
            PageId::Chorus => {
                apply_chorus_encoder(idx, delta, &mut params.chorus);
                return;
            }
            PageId::Delay => {
                apply_delay_encoder(idx, delta, &mut params.delay);
                return;
            }
            PageId::Efx | PageId::MixReverb => {
                apply_reverb_encoder(idx, delta, &mut params.reverb);
                return;
            }
            _ => {}
        }
        if let Some(param) = self.resolve_param_mut(idx, params) {
            let step = (param.max - param.min) / 128.0;
            param.nudge(delta as f32 * step);
        }
    }

    /// Shift+encoder: snap to coarse jump points defined by the cell type.
    /// The caller must supply the `ValFmt` for encoder `idx` (from `BlockDef.params[idx].format`).
    pub fn snap_encoder(&self, idx: usize, delta: i8, fmt: ValFmt, params: &mut ParamSnapshot) {
        if let Some((blk, id)) = self.resolve_mut(idx, params) {
            blk.snap(id, delta);
            return;
        }
        if let Some(param) = self.resolve_param_mut(idx, params) {
            param.snap_to(delta, fmt.snap_points());
        }
    }

    /// Block + param bound to encoder `idx`, for pages whose block implements
    /// `Block`. Tasks 3–11 add one arm per converted block.
    fn resolve_mut<'a>(
        &self,
        idx: usize,
        params: &'a mut ParamSnapshot,
    ) -> Option<(&'a mut dyn Block, ParamId)> {
        let p = params;
        match self {
            PageId::Pizza => bind(&mut p.pizza, *PIZZA_PAGE.get(idx)?),
            PageId::EngineModal1 => bind(&mut p.modal, *MODAL1_PAGE.get(idx)?),
            PageId::EngineModal2 => bind(&mut p.modal, *MODAL2_PAGE.get(idx)?),
            PageId::Drive => bind(&mut p.drive, *DRIVE_PAGE.get(idx)?),
            PageId::DemoWaves => match idx {
                0 => bind(&mut p.drive, DriveParams::DRIVE),
                1 => bind(&mut p.drive, DriveParams::TONE),
                2 => bind(&mut p.folder, FolderParams::FOLD),
                3 => bind(&mut p.folder, FolderParams::SYMMETRY),
                4 => bind(&mut p.filter, FilterParams::ENV_AMOUNT),
                5 => bind(&mut p.out, OutParams::PAN),
                _ => None,
            },
            PageId::Filter => bind(&mut p.filter, *FILTER_PAGE.get(idx)?),
            PageId::Folder => bind(&mut p.folder, *FOLDER_PAGE.get(idx)?),
            PageId::DemoShapes => match idx {
                0 => bind(&mut p.out, OutParams::VOLUME),
                1 => bind(&mut p.filter, FilterParams::CUTOFF),
                2 => bind(&mut p.out, OutParams::PAN),
                3 => bind(&mut p.filter, FilterParams::DRIVE),
                4 => bind(&mut p.filter, FilterParams::RESONANCE),
                5 => bind(&mut p.filter, FilterParams::FM_AMOUNT),
                _ => None,
            },
            PageId::EnvAmp | PageId::Vca => bind(&mut p.envelopes[0], *ENV_PAGE.get(idx)?),
            PageId::EnvFilter => bind(&mut p.envelopes[1], *ENV_PAGE.get(idx)?),
            PageId::EnvAux => bind(&mut p.envelopes[2], *ENV_PAGE.get(idx)?),
            PageId::DemoMotion => match idx {
                0..=3 => bind(&mut p.envelopes[0], ENV_PAGE[idx]),
                4 => bind(&mut p.envelopes[1], EnvParams::ATTACK),
                5 => bind(&mut p.envelopes[1], EnvParams::DECAY),
                _ => None,
            },
            PageId::Lfo => bind(&mut p.lfo, *LFO_PAGE.get(idx)?),
            PageId::FmAlg => match idx {
                0 => bind(&mut p.fm, FmParams::ALGORITHM),
                2 => bind(&mut p.out, OutParams::VOLUME),
                _ => None,
            },
            // Slot 0 selects the operator (handled in `apply_encoder`).
            PageId::FmOp => bind(&mut p.fm.operators[fm_selected_op()], *FM_OP_PAGE.get(idx.checked_sub(1)?)?),
            PageId::FmRatio => match idx {
                0..=3 => bind(&mut p.fm.operators[idx], FmOpParams::COARSE),
                4 => bind(&mut p.fm.operators[fm_selected_op()], FmOpParams::FINE),
                _ => None,
            },
            PageId::FmEnv1 => bind(&mut p.fm.operators[0], *FM_ENV_PAGE.get(idx)?),
            PageId::FmEnv2 => bind(&mut p.fm.operators[1], *FM_ENV_PAGE.get(idx)?),
            PageId::FmEnv3 => bind(&mut p.fm.operators[2], *FM_ENV_PAGE.get(idx)?),
            PageId::FmEnv4 => bind(&mut p.fm.operators[3], *FM_ENV_PAGE.get(idx)?),
            PageId::DemoFm => match idx {
                0 => bind(&mut p.fm, FmParams::ALGORITHM),
                1..=3 => bind(&mut p.fm.operators[idx - 1], FmOpParams::FEEDBACK),
                _ => None,
            },
            PageId::Mixer | PageId::Master => bind(&mut p.out, *OUT_PAGE.get(idx)?),
            _ => None,
        }
    }

    /// Resolve the mutable Param reference for encoder `idx` on this page.
    fn resolve_param_mut<'a>(
        &self,
        idx: usize,
        params: &'a mut ParamSnapshot,
    ) -> Option<&'a mut crate::params::Param> {
        match self {
            PageId::DemoMatrix => None,
            _ => None,
        }
    }
}

/// Encoder slot → param id, per page. Shared by `read_values` and `resolve_mut`.
const PIZZA_PAGE: [ParamId; 3] = [PizzaParams::SHAPE, PizzaParams::CRUSH, PizzaParams::LEVEL];
const DRIVE_PAGE: [ParamId; 3] = [DriveParams::DRIVE, DriveParams::TONE, DriveParams::MIX];
const FILTER_PAGE: [ParamId; 6] = [
    FilterParams::CUTOFF,
    FilterParams::RESONANCE,
    FilterParams::DRIVE,
    FilterParams::FM_AMOUNT,
    FilterParams::ENV_AMOUNT,
    FilterParams::KEY_TRACK,
];
const FOLDER_PAGE: [ParamId; 3] = [FolderParams::FOLD, FolderParams::SYMMETRY, FolderParams::MIX];
const ENV_PAGE: [ParamId; 6] = [
    EnvParams::ATTACK,
    EnvParams::DECAY,
    EnvParams::SUSTAIN,
    EnvParams::RELEASE,
    EnvParams::LEVEL,
    EnvParams::VEL_SENS,
];
const LFO_PAGE: [ParamId; 6] = [
    LfoParams::RATE,
    LfoParams::SHAPE,
    LfoParams::SYNC,
    LfoParams::PHASE,
    LfoParams::DEPTH,
    LfoParams::OFFSET,
];
/// FM_OP slots 1..=5 (slot 0 selects the operator).
const FM_OP_PAGE: [ParamId; 5] = [
    FmOpParams::WAVEFORM,
    FmOpParams::LEVEL,
    FmOpParams::FEEDBACK,
    FmOpParams::DETUNE,
    FmOpParams::VELOCITY_SENS,
];
const FM_ENV_PAGE: [ParamId; 6] = [
    FmOpParams::ATTACK_RATE,
    FmOpParams::DECAY1_RATE,
    FmOpParams::DECAY1_LEVEL,
    FmOpParams::DECAY2_RATE,
    FmOpParams::RELEASE_RATE,
    FmOpParams::RATE_SCALING,
];
const OUT_PAGE: [ParamId; 2] = [OutParams::VOLUME, OutParams::PAN];
const MODAL1_PAGE: [ParamId; 6] = [
    ModalParams::MODE,
    ModalParams::EXCITE,
    ModalParams::DECAY,
    ModalParams::BRIGHTNESS,
    ModalParams::POSITION,
    ModalParams::INHARM,
];
const MODAL2_PAGE: [ParamId; 6] = [
    ModalParams::KS_BODY,
    ModalParams::KS_STIFFNESS,
    ModalParams::KS_FEEDBACK,
    ModalParams::KS_ENS_DEPTH,
    ModalParams::KS_ENS_RATE,
    ModalParams::KS_ENS_MIX,
];

/// Coercion point so every `resolve_mut` arm has the same type.
fn bind<'a>(b: &'a mut dyn Block, id: ParamId) -> Option<(&'a mut dyn Block, ParamId)> {
    Some((b, id))
}

/// Normalized values of `ids` on `b`, padded with 0.0 to six slots.
fn read_block<const N: usize>(b: &dyn Block, ids: [ParamId; N]) -> [f32; 6] {
    let mut out = [0.0f32; 6];
    for (o, id) in out.iter_mut().zip(ids) {
        *o = b.normalized(id);
    }
    out
}

fn apply_chorus_encoder(
    idx: usize,
    delta: i8,
    chorus: &mut crate::dsp::chorus::ChorusParams,
) {
    let step = 1.0 / 128.0;
    match idx {
        0 => nudge_u8(&mut chorus.mode, delta, 3),
        1 => nudge_float(&mut chorus.rate, delta, step),
        2 => nudge_float(&mut chorus.depth, delta, step),
        3 => nudge_float(&mut chorus.mix, delta, step),
        _ => {}
    }
}

fn apply_delay_encoder(
    idx: usize,
    delta: i8,
    delay: &mut crate::dsp::delay::DelayParams,
) {
    match idx {
        0 => {
            // TIME: 10ms to 1000ms, logarithmic feel
            delay.time_ms = (delay.time_ms + delta as f32 * 8.0).clamp(10.0, 1000.0);
        }
        1 => nudge_float(&mut delay.feedback, delta, 1.0 / 128.0),
        2 => nudge_float(&mut delay.wow_flutter, delta, 1.0 / 128.0),
        3 => nudge_float(&mut delay.saturation, delta, 1.0 / 128.0),
        4 => nudge_float(&mut delay.tone, delta, 1.0 / 128.0),
        5 => nudge_float(&mut delay.mix, delta, 1.0 / 128.0),
        _ => {}
    }
}

fn apply_reverb_encoder(
    idx: usize,
    delta: i8,
    reverb: &mut crate::dsp::reverb::ReverbParams,
) {
    let step = 1.0 / 128.0;
    match idx {
        0 => nudge_u8(&mut reverb.reverb_type, delta, 2),
        1 => nudge_float(&mut reverb.time, delta, step),
        2 => nudge_float(&mut reverb.damping, delta, step),
        3 => nudge_float(&mut reverb.size, delta, step),
        4 => nudge_float(&mut reverb.mix, delta, step),
        _ => {}
    }
}

fn nudge_float(v: &mut f32, delta: i8, step: f32) {
    *v = (*v + delta as f32 * step).clamp(0.0, 1.0);
}

fn nudge_u8(v: &mut u8, delta: i8, max: u8) {
    let n = *v as i8 + delta;
    *v = n.clamp(0, max as i8) as u8;
}

// ---------------------------------------------------------------------------
// FM operator selection state (module-level, simple static)
// ---------------------------------------------------------------------------

/// Currently selected FM operator index (0-3).
/// This is UI-only state shared between FM_OP and FM_RATIO pages.
static FM_SEL_OP: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

pub fn fm_selected_op() -> usize {
    FM_SEL_OP.load(core::sync::atomic::Ordering::Relaxed) as usize
}

fn fm_set_selected_op(idx: u8) {
    FM_SEL_OP.store(idx.min(3), core::sync::atomic::Ordering::Relaxed);
}
