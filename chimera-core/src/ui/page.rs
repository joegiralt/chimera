use crate::params::ParamSnapshot;
use crate::ui::chain::ChainNav;

/// Cell type: defines display format and snap behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValFmt {
    /// Unipolar: 0 to 127. Snaps: 0, 100, 127.
    Uni,
    /// Bipolar: -64 to +63. Snaps: -64, -44, 0, +43, +63.
    Bi,
    /// Discrete integer 0..N. N is stored in the variant.
    /// Display shows the integer directly. Snaps at each integer.
    Int(u8),
}

impl ValFmt {
    /// Coarse snap points in normalized 0..1 space.
    pub fn snap_points(self) -> &'static [f32] {
        match self {
            ValFmt::Uni => &[0.0, 100.0 / 127.0, 1.0],
            ValFmt::Bi => &[0.0, 20.0 / 127.0, 64.0 / 127.0, 107.0 / 127.0, 1.0],
            // Discrete: shift-encoder jumps to 0 or max
            ValFmt::Int(_) => &[0.0, 1.0],
        }
    }

    pub fn is_bipolar(self) -> bool {
        matches!(self, ValFmt::Bi)
    }

    /// Max integer value (only meaningful for Int variant).
    pub fn max_int(self) -> u8 {
        match self {
            ValFmt::Int(n) => n,
            _ => 127,
        }
    }
}

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
    DemoWaves,
    DemoShapes,
    DemoMotion,
    DemoMatrix,
}

impl PageId {
    /// Resolve which page is active from current navigation position.
    pub fn from_nav(nav: &ChainNav) -> Self {
        use crate::ui::chain::ChainId;
        match nav.chain_id {
            ChainId::Part(_) => match nav.node {
                0 => PageId::Pizza,
                1 => PageId::Drive,
                2 => PageId::Filter,
                3 => PageId::Folder,
                4 => PageId::Vca,
                _ => PageId::Efx,
            },
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
                _ => PageId::DemoMatrix,
            },
        }
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &ParamSnapshot) -> [f32; 6] {
        match self {
            PageId::Pizza => [
                params.pizza.shape,
                params.pizza.crush,
                params.pizza.level,
                0.0,
                0.0,
                0.0,
            ],
            PageId::Filter => [
                params.filter.cutoff.normalized(),
                params.filter.resonance.normalized(),
                params.filter.drive.normalized(),
                params.filter.fm_amount.normalized(),
                params.filter.env_amount.normalized(),
                params.filter.key_track.normalized(),
            ],
            PageId::EnvAmp | PageId::Vca => read_env_values(&params.envelopes[0]),
            PageId::EnvFilter => read_env_values(&params.envelopes[1]),
            PageId::EnvAux => read_env_values(&params.envelopes[2]),
            PageId::Mixer => [
                params.volume.normalized(),
                params.pan.normalized(),
                0.5,
                0.0,
                0.5,
                0.0, // placeholders
            ],
            PageId::Drive => [
                params.drive.drive.normalized(),
                params.drive.tone.normalized(),
                params.drive.mix.normalized(),
                0.0,
                0.0,
                0.0,
            ],
            PageId::Folder => [
                params.folder.fold.normalized(),
                params.folder.symmetry.normalized(),
                params.folder.mix.normalized(),
                0.0,
                0.0,
                0.0,
            ],
            // Demo pages reuse filter + envelope params for tweaking
            PageId::DemoWaves => [
                params.drive.drive.normalized(),
                params.drive.tone.normalized(),
                params.folder.fold.normalized(),
                params.folder.symmetry.normalized(),
                params.filter.env_amount.normalized(),
                params.pan.normalized(),
            ],
            PageId::DemoShapes => [
                params.volume.normalized(),
                params.filter.cutoff.normalized(),
                params.pan.normalized(),
                params.filter.drive.normalized(),
                params.filter.resonance.normalized(),
                params.filter.fm_amount.normalized(),
            ],
            PageId::DemoMotion => [
                params.envelopes[0].attack.normalized(),
                params.envelopes[0].decay.normalized(),
                params.envelopes[0].sustain.normalized(),
                params.envelopes[0].release.normalized(),
                params.envelopes[1].attack.normalized(),
                params.envelopes[1].decay.normalized(),
            ],
            PageId::DemoMatrix => [0.0; 6],
            PageId::EngineModal1 => [
                params.modal.mode as f32 / 2.0,
                params.modal.excite,
                params.modal.decay,
                params.modal.brightness,
                params.modal.position,
                params.modal.inharm,
            ],
            PageId::EngineModal2 => [
                params.modal.ks_body,
                params.modal.ks_stiffness,
                params.modal.ks_feedback,
                params.modal.ks_ens_depth,
                params.modal.ks_ens_rate,
                params.modal.ks_ens_mix,
            ],
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
            PageId::Master => [
                params.volume.normalized(),
                params.pan.normalized(),
                0.0, 0.0, 0.0, 0.0,
            ],
        }
    }

    /// Apply an encoder delta. Each tick = 1/128 of the parameter range.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        // Direct float manipulation (not Param structs)
        match self {
            PageId::Pizza => {
                apply_pizza_encoder(idx, delta, &mut params.pizza);
                return;
            }
            PageId::EngineModal1 => {
                apply_modal1_encoder(idx, delta, &mut params.modal);
                return;
            }
            PageId::EngineModal2 => {
                apply_modal2_encoder(idx, delta, &mut params.modal);
                return;
            }
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
        if let Some(param) = self.resolve_param_mut(idx, params) {
            param.snap_to(delta, fmt.snap_points());
        }
    }

    /// Resolve the mutable Param reference for encoder `idx` on this page.
    fn resolve_param_mut<'a>(
        &self,
        idx: usize,
        params: &'a mut ParamSnapshot,
    ) -> Option<&'a mut crate::params::Param> {
        match self {
            PageId::Filter => match idx {
                0 => Some(&mut params.filter.cutoff),
                1 => Some(&mut params.filter.resonance),
                2 => Some(&mut params.filter.drive),
                3 => Some(&mut params.filter.fm_amount),
                4 => Some(&mut params.filter.env_amount),
                5 => Some(&mut params.filter.key_track),
                _ => None,
            },
            PageId::EnvAmp | PageId::Vca => resolve_env_param(&mut params.envelopes[0], idx),
            PageId::EnvFilter => resolve_env_param(&mut params.envelopes[1], idx),
            PageId::EnvAux => resolve_env_param(&mut params.envelopes[2], idx),
            PageId::Mixer => match idx {
                0 => Some(&mut params.volume),
                1 => Some(&mut params.pan),
                _ => None,
            },
            PageId::Drive => match idx {
                0 => Some(&mut params.drive.drive),
                1 => Some(&mut params.drive.tone),
                2 => Some(&mut params.drive.mix),
                _ => None,
            },
            PageId::Folder => match idx {
                0 => Some(&mut params.folder.fold),
                1 => Some(&mut params.folder.symmetry),
                2 => Some(&mut params.folder.mix),
                _ => None,
            },
            PageId::DemoWaves => match idx {
                0 => Some(&mut params.drive.drive),
                1 => Some(&mut params.drive.tone),
                2 => Some(&mut params.folder.fold),
                3 => Some(&mut params.folder.symmetry),
                4 => Some(&mut params.filter.env_amount),
                5 => Some(&mut params.pan),
                _ => None,
            },
            PageId::DemoShapes => match idx {
                0 => Some(&mut params.volume),
                1 => Some(&mut params.filter.cutoff),
                2 => Some(&mut params.pan),
                3 => Some(&mut params.filter.drive),
                4 => Some(&mut params.filter.resonance),
                5 => Some(&mut params.filter.fm_amount),
                _ => None,
            },
            PageId::DemoMotion => match idx {
                0 => Some(&mut params.envelopes[0].attack),
                1 => Some(&mut params.envelopes[0].decay),
                2 => Some(&mut params.envelopes[0].sustain),
                3 => Some(&mut params.envelopes[0].release),
                4 => Some(&mut params.envelopes[1].attack),
                5 => Some(&mut params.envelopes[1].decay),
                _ => None,
            },
            PageId::DemoMatrix => None,
            _ => None,
        }
    }
}

fn apply_modal1_encoder(idx: usize, delta: i8, modal: &mut crate::dsp::modal::ModalParams) {
    let step = 1.0 / 128.0;
    match idx {
        0 => nudge_u8(&mut modal.mode, delta, 2),
        1 => nudge_float(&mut modal.excite, delta, step),
        2 => nudge_float(&mut modal.decay, delta, step),
        3 => nudge_float(&mut modal.brightness, delta, step),
        4 => nudge_float(&mut modal.position, delta, step),
        5 => nudge_float(&mut modal.inharm, delta, step),
        _ => {}
    }
}

fn apply_modal2_encoder(idx: usize, delta: i8, modal: &mut crate::dsp::modal::ModalParams) {
    let step = 1.0 / 128.0;
    match idx {
        0 => nudge_float(&mut modal.ks_body, delta, step),
        1 => nudge_float(&mut modal.ks_stiffness, delta, step),
        2 => nudge_float(&mut modal.ks_feedback, delta, step),
        3 => nudge_float(&mut modal.ks_ens_depth, delta, step),
        4 => nudge_float(&mut modal.ks_ens_rate, delta, step),
        5 => nudge_float(&mut modal.ks_ens_mix, delta, step),
        _ => {}
    }
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

fn read_env_values(e: &crate::params::EnvParams) -> [f32; 6] {
    [
        e.attack.normalized(),
        e.decay.normalized(),
        e.sustain.normalized(),
        e.release.normalized(),
        e.level.normalized(),
        e.vel_sens.normalized(),
    ]
}

fn nudge_float(v: &mut f32, delta: i8, step: f32) {
    *v = (*v + delta as f32 * step).clamp(0.0, 1.0);
}

fn nudge_u8(v: &mut u8, delta: i8, max: u8) {
    let n = *v as i8 + delta;
    *v = n.clamp(0, max as i8) as u8;
}

fn apply_pizza_encoder(idx: usize, delta: i8, pizza: &mut crate::dsp::pizza::PizzaParams) {
    let step = 1.0 / 128.0;
    match idx {
        0 => nudge_float(&mut pizza.shape, delta, step),
        1 => nudge_float(&mut pizza.crush, delta, step),
        2 => nudge_float(&mut pizza.level, delta, step),
        _ => {}
    }
}

fn resolve_env_param(
    env: &mut crate::params::EnvParams,
    idx: usize,
) -> Option<&mut crate::params::Param> {
    match idx {
        0 => Some(&mut env.attack),
        1 => Some(&mut env.decay),
        2 => Some(&mut env.sustain),
        3 => Some(&mut env.release),
        4 => Some(&mut env.level),
        5 => Some(&mut env.vel_sens),
        _ => None,
    }
}
