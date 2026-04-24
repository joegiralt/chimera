use crate::params::ParamSnapshot;
use crate::preset::ChainType;
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
    /// FM algorithm topology diagram (8 algorithms, val selects which)
    FmAlgorithm,
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
            PageId::Lfo => [
                params.lfo.rate / 20.0,           // normalized 0-1 (0-20 Hz)
                params.lfo.shape as f32 / 4.0,    // 0-4 shapes
                params.lfo.sync as f32,            // 0 or 1
                params.lfo.phase_offset,           // 0-1
                params.lfo.depth,                  // 0-1
                (params.lfo.offset + 1.0) / 2.0,  // -1..1 → 0..1 for display
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
            PageId::FmAlg => [
                params.fm.algorithm.normalized(),
                0.0,
                params.volume.normalized(),
                0.0,
                0.0,
                0.0,
            ],
            PageId::FmOp => {
                // Selected operator index stored in first slot as a UI concept.
                // For read_values, show op0 by default (operator selection is
                // handled by the encoder apply logic via a static index).
                let op = &params.fm.operators[fm_selected_op()];
                [
                    fm_selected_op() as f32 / 3.0,
                    op.waveform.normalized(),
                    op.level.normalized(),
                    op.feedback.normalized(),
                    op.detune.normalized(),
                    op.velocity_sens.normalized(),
                ]
            },
            PageId::FmRatio => [
                params.fm.operators[0].coarse.normalized(),
                params.fm.operators[1].coarse.normalized(),
                params.fm.operators[2].coarse.normalized(),
                params.fm.operators[3].coarse.normalized(),
                params.fm.operators[fm_selected_op()].fine.normalized(),
                0.0,
            ],
            PageId::FmEnv1 => read_fm_env_values(&params.fm.operators[0]),
            PageId::FmEnv2 => read_fm_env_values(&params.fm.operators[1]),
            PageId::FmEnv3 => read_fm_env_values(&params.fm.operators[2]),
            PageId::FmEnv4 => read_fm_env_values(&params.fm.operators[3]),
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
            PageId::Lfo => {
                apply_lfo_encoder(idx, delta, &mut params.lfo);
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
            PageId::FmAlg => {
                apply_fm_alg_encoder(idx, delta, &mut params.fm, &mut params.volume);
                return;
            }
            PageId::FmOp => {
                apply_fm_op_encoder(idx, delta, params);
                return;
            }
            PageId::FmRatio => {
                apply_fm_ratio_encoder(idx, delta, params);
                return;
            }
            PageId::FmEnv1 => {
                apply_fm_env_encoder(idx, delta, &mut params.fm.operators[0]);
                return;
            }
            PageId::FmEnv2 => {
                apply_fm_env_encoder(idx, delta, &mut params.fm.operators[1]);
                return;
            }
            PageId::FmEnv3 => {
                apply_fm_env_encoder(idx, delta, &mut params.fm.operators[2]);
                return;
            }
            PageId::FmEnv4 => {
                apply_fm_env_encoder(idx, delta, &mut params.fm.operators[3]);
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
            PageId::Lfo => None, // handled by apply_encoder special case
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

fn apply_lfo_encoder(idx: usize, delta: i8, lfo: &mut crate::dsp::lfo::LfoParams) {
    let step = 1.0 / 128.0;
    match idx {
        0 => {
            // Rate: 0.01 to 20 Hz, logarithmic feel
            lfo.rate = (lfo.rate + delta as f32 * 0.15).clamp(0.01, 20.0);
        }
        1 => nudge_u8(&mut lfo.shape, delta, 4),    // 5 shapes (0-4)
        2 => nudge_u8(&mut lfo.sync, delta, 1),     // free/sync
        3 => nudge_float(&mut lfo.phase_offset, delta, step),
        4 => nudge_float(&mut lfo.depth, delta, step),
        5 => {
            // Offset: -1.0 to +1.0
            lfo.offset = (lfo.offset + delta as f32 * step * 2.0).clamp(-1.0, 1.0);
        }
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

// ---------------------------------------------------------------------------
// FM operator selection state (module-level, simple static)
// ---------------------------------------------------------------------------

/// Currently selected FM operator index (0-3).
/// This is UI-only state shared between FM_OP and FM_RATIO pages.
static FM_SEL_OP: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

fn fm_selected_op() -> usize {
    FM_SEL_OP.load(core::sync::atomic::Ordering::Relaxed) as usize
}

fn fm_set_selected_op(idx: u8) {
    FM_SEL_OP.store(idx.min(3), core::sync::atomic::Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// FM encoder handlers
// ---------------------------------------------------------------------------

fn apply_fm_alg_encoder(
    idx: usize,
    delta: i8,
    fm: &mut crate::params::FmParams,
    volume: &mut crate::params::Param,
) {
    match idx {
        0 => {
            // Algorithm: integer 0-7
            let cur = fm.algorithm.value as i8;
            fm.algorithm.value = (cur + delta).clamp(0, 7) as f32;
        }
        2 => {
            // Level (overall volume)
            let step = (volume.max - volume.min) / 128.0;
            volume.nudge(delta as f32 * step);
        }
        _ => {}
    }
}

fn apply_fm_op_encoder(idx: usize, delta: i8, params: &mut ParamSnapshot) {
    match idx {
        0 => {
            // Operator select: 0-3
            let cur = fm_selected_op() as i8;
            fm_set_selected_op((cur + delta).clamp(0, 3) as u8);
        }
        _ => {
            let sel = fm_selected_op();
            let op = &mut params.fm.operators[sel];
            match idx {
                1 => {
                    // Waveform: integer 0-7
                    let cur = op.waveform.value as i8;
                    op.waveform.value = (cur + delta).clamp(0, 7) as f32;
                }
                2 => {
                    // Level: integer 0-99
                    let cur = op.level.value as i8;
                    op.level.value = (cur as i16 + delta as i16).clamp(0, 99) as f32;
                }
                3 => {
                    // Feedback: integer 0-7
                    let cur = op.feedback.value as i8;
                    op.feedback.value = (cur + delta).clamp(0, 7) as f32;
                }
                4 => {
                    // Detune: integer -7 to 7
                    let cur = op.detune.value as i8;
                    op.detune.value = (cur + delta).clamp(-7, 7) as f32;
                }
                5 => {
                    // Velocity sensitivity: integer 0-7
                    let cur = op.velocity_sens.value as i8;
                    op.velocity_sens.value = (cur + delta).clamp(0, 7) as f32;
                }
                _ => {}
            }
        }
    }
}

fn read_fm_env_values(op: &crate::params::FmOpParams) -> [f32; 6] {
    [
        op.attack_rate.normalized(),
        op.decay1_rate.normalized(),
        op.decay1_level.normalized(),
        op.decay2_rate.normalized(),
        op.release_rate.normalized(),
        op.rate_scaling.normalized(),
    ]
}

fn apply_fm_env_encoder(idx: usize, delta: i8, op: &mut crate::params::FmOpParams) {
    match idx {
        0 => {
            let cur = op.attack_rate.value as i8;
            op.attack_rate.value = (cur as i16 + delta as i16).clamp(0, 31) as f32;
        }
        1 => {
            let cur = op.decay1_rate.value as i8;
            op.decay1_rate.value = (cur as i16 + delta as i16).clamp(0, 31) as f32;
        }
        2 => {
            let cur = op.decay1_level.value as i8;
            op.decay1_level.value = (cur + delta).clamp(0, 15) as f32;
        }
        3 => {
            let cur = op.decay2_rate.value as i8;
            op.decay2_rate.value = (cur as i16 + delta as i16).clamp(0, 31) as f32;
        }
        4 => {
            let cur = op.release_rate.value as i8;
            op.release_rate.value = (cur + delta).clamp(1, 15) as f32;
        }
        5 => {
            let cur = op.rate_scaling.value as i8;
            op.rate_scaling.value = (cur + delta).clamp(0, 3) as f32;
        }
        _ => {}
    }
}

fn apply_fm_ratio_encoder(idx: usize, delta: i8, params: &mut ParamSnapshot) {
    match idx {
        0..=3 => {
            // Coarse ratio for op 0-3: integer 0-63
            let op = &mut params.fm.operators[idx];
            let cur = op.coarse.value as i8;
            op.coarse.value = (cur as i16 + delta as i16).clamp(0, 63) as f32;
        }
        4 => {
            // Fine for selected op: integer 0-15
            let sel = fm_selected_op();
            let op = &mut params.fm.operators[sel];
            let cur = op.fine.value as i8;
            op.fine.value = (cur + delta).clamp(0, 15) as f32;
        }
        _ => {}
    }
}
