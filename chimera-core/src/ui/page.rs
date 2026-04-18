use crate::params::ParamSnapshot;
use crate::ui::chain::ChainNav;

/// Cell type: defines display format and snap behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValFmt {
    /// Unipolar: 0 to 127. Snaps: 0, 100, 127.
    Uni,
    /// Bipolar: -64 to +63. Snaps: -64, -44, 0, +43, +63.
    Bi,
}

impl ValFmt {
    /// Coarse snap points in normalized 0..1 space.
    pub fn snap_points(self) -> &'static [f32] {
        match self {
            // 0, 100, 127
            ValFmt::Uni => &[0.0, 100.0 / 127.0, 1.0],
            // -64, -44, 0, +43, +63 → midi 0, 20, 64, 107, 127
            ValFmt::Bi => &[0.0, 20.0 / 127.0, 64.0 / 127.0, 107.0 / 127.0, 1.0],
        }
    }

    pub fn is_bipolar(self) -> bool {
        matches!(self, ValFmt::Bi)
    }
}

/// Layout mode for a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageLayout {
    /// One large visualization + 3x2 parameter grid below.
    BigViz,
    /// 3x2 grid of independent cells, each with its own mini icon.
    CellGrid,
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
    EngineFm,
    EngineModal,
    EngineVa,
    Drive,
    Filter,
    Folder,
    Vca,
    Efx,
    Mixer,
    Routing,
    Compressor,
    GlobalEfx,
    EnvAmp,
    EnvFilter,
    EnvAux,
    DemoWaves,
    DemoShapes,
    DemoMotion,
}

impl PageId {
    /// Resolve which page is active from current navigation position.
    pub fn from_nav(nav: &ChainNav) -> Self {
        match nav.chain {
            0 => match nav.node {
                0 => match nav.sub_page {
                    1 => PageId::EngineModal,
                    2 => PageId::EngineVa,
                    _ => PageId::EngineFm,
                },
                1 => PageId::Drive,
                2 => PageId::Filter,
                3 => PageId::Folder,
                4 => PageId::Vca,
                _ => PageId::Efx,
            },
            1 => match nav.node {
                0 => PageId::Mixer,
                1 => PageId::Routing,
                2 => PageId::Drive,
                3 => PageId::Compressor,
                _ => PageId::GlobalEfx,
            },
            2 => match nav.node {
                0 => PageId::EnvAmp,
                1 => PageId::EnvFilter,
                _ => PageId::EnvAux,
            },
            5 => match nav.node {
                0 => PageId::DemoWaves,
                1 => PageId::DemoShapes,
                _ => PageId::DemoMotion,
            },
            _ => PageId::EngineFm,
        }
    }

    /// Which layout mode this page uses.
    pub fn layout(&self) -> PageLayout {
        match self {
            // Big viz: pages with a single unified visualization
            PageId::Filter | PageId::EnvAmp | PageId::EnvFilter | PageId::EnvAux
            | PageId::Vca | PageId::EngineFm | PageId::Routing | PageId::Compressor => {
                PageLayout::BigViz
            }
            // Demo + everything else: cell grid
            _ => PageLayout::CellGrid,
        }
    }

    /// Icons for each of the 6 encoder cells (CellGrid mode only).
    pub fn cell_icons(&self) -> [CellIcon; 6] {
        match self {
            PageId::Drive => [
                CellIcon::WaveClip,  // DRIVE — sine morphs to square
                CellIcon::ToneTilt,  // TONE — EQ tilt
                CellIcon::DryWet,    // MIX — blend arc
                CellIcon::None,
                CellIcon::None,
                CellIcon::None,
            ],
            PageId::Folder => [
                CellIcon::WaveFold,  // FOLD — sine getting folded
                CellIcon::Symmetry,  // SYM — bias indicator
                CellIcon::DryWet,    // MIX — blend arc
                CellIcon::None,
                CellIcon::None,
                CellIcon::None,
            ],
            PageId::EngineVa => [
                CellIcon::WaveShape, // WAVE — saw/square/tri
                CellIcon::PulseWidth,// PW — pulse width
                CellIcon::Arc,       // SYNC
                CellIcon::Arc,       // SUB
                CellIcon::Arc,       // DETUNE
                CellIcon::DryWet,    // MIX
            ],
            PageId::EngineModal => [
                CellIcon::Arc,       // EXCITE
                CellIcon::Arc,       // DECAY
                CellIcon::Arc,       // DAMP
                CellIcon::Arc,       // PITCH
                CellIcon::Arc,       // BRIGHT
                CellIcon::Arc,       // POS
            ],
            PageId::Mixer => [
                CellIcon::LevelBar,  // VOL
                CellIcon::PanDot,    // PAN
                CellIcon::Arc,       // VOICES
                CellIcon::Arc,       // MIDI
                CellIcon::Arc,       // PITCH
                CellIcon::Arc,       // GLIDE
            ],
            PageId::Efx | PageId::GlobalEfx => [
                CellIcon::Arc, CellIcon::Arc, CellIcon::Arc,
                CellIcon::Arc, CellIcon::Arc, CellIcon::Arc,
            ],
            // Demo storybook: page 1 — waveform icons
            PageId::DemoWaves => [
                CellIcon::WaveClip,
                CellIcon::WaveShape,
                CellIcon::PulseWidth,
                CellIcon::WaveFold,
                CellIcon::ToneTilt,
                CellIcon::Symmetry,
            ],
            // Demo storybook: page 2 — shape icons
            PageId::DemoShapes => [
                CellIcon::Arc,
                CellIcon::LevelBar,
                CellIcon::PanDot,
                CellIcon::DryWet,
                CellIcon::Cube,
                CellIcon::Stack,
            ],
            // Demo storybook: page 3 — motion icons
            PageId::DemoMotion => [
                CellIcon::Ripple,
                CellIcon::Burst,
                CellIcon::Orbit,
                CellIcon::Scatter,
                CellIcon::Bounce,
                CellIcon::Breathe,
            ],
            _ => [CellIcon::None; 6],
        }
    }

    /// Display format for each encoder's value.
    pub fn val_formats(&self) -> [ValFmt; 6] {
        use ValFmt::*;
        match self {
            // Filter env amount is bipolar (-64..+63)
            PageId::Filter => [Uni, Uni, Uni, Uni, Bi, Uni],
            // Pan is bipolar
            PageId::Mixer => [Uni, Bi, Uni, Uni, Bi, Uni],
            // Tone + Mix bipolar
            PageId::Drive => [Uni, Bi, Bi, Uni, Uni, Uni],
            // Symmetry + Mix bipolar
            PageId::Folder => [Uni, Bi, Bi, Uni, Uni, Uni],
            // Mix bipolar
            PageId::EngineVa => [Uni, Uni, Uni, Uni, Uni, Bi],
            PageId::DemoWaves => [Uni, Uni, Uni, Uni, Bi, Bi],
            PageId::DemoShapes => [Uni, Uni, Bi, Bi, Uni, Uni],
            PageId::DemoMotion => [Uni, Uni, Uni, Uni, Bi, Uni],
            _ => [Uni; 6],
        }
    }

    /// 6 encoder labels for this page (3x2 grid: a-f).
    pub fn encoder_labels(&self) -> [&'static str; 6] {
        match self {
            PageId::EngineFm => ["ALGO", "RATIO", "WAVE", "FDBK", "DEPTH", "DETUNE"],
            PageId::EngineModal => ["EXCITE", "DECAY", "DAMP", "PITCH", "BRIGHT", "POS"],
            PageId::EngineVa => ["WAVE", "PW", "SYNC", "SUB", "DETUNE", "MIX"],
            PageId::Filter => ["CUTOFF", "RESO", "DRIVE", "FM", "ENV", "TRACK"],
            PageId::Folder => ["FOLD", "SYM", "MIX", "--", "--", "--"],
            PageId::Vca => ["ATK", "DEC", "SUS", "REL", "LEVEL", "VEL"],
            PageId::EnvAmp => ["ATK", "DEC", "SUS", "REL", "LEVEL", "VEL"],
            PageId::EnvFilter => ["ATK", "DEC", "SUS", "REL", "LEVEL", "VEL"],
            PageId::EnvAux => ["ATK", "DEC", "SUS", "REL", "LEVEL", "VEL"],
            PageId::Mixer => ["VOL", "PAN", "VOICES", "MIDI", "PITCH", "GLIDE"],
            PageId::Drive => ["DRIVE", "TONE", "MIX", "--", "--", "--"],
            PageId::Routing | PageId::Compressor | PageId::GlobalEfx | PageId::Efx => {
                ["--", "--", "--", "--", "--", "--"]
            }
            PageId::DemoWaves => ["CLIP", "WAVE", "PW", "FOLD", "TILT", "SYM"],
            PageId::DemoShapes => ["ARC", "LEVEL", "PAN", "D/W", "CUBE", "STACK"],
            PageId::DemoMotion => ["RIPPL", "BURST", "ORBIT", "SCATR", "BOUNC", "PULSE"],
        }
    }

    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &ParamSnapshot) -> [f32; 6] {
        match self {
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
                0.5, 0.0, 0.5, 0.0, // placeholders
            ],
            PageId::Drive => [
                params.drive.drive.normalized(),
                params.drive.tone.normalized(),
                params.drive.mix.normalized(),
                0.0, 0.0, 0.0,
            ],
            PageId::Folder => [
                params.folder.fold.normalized(),
                params.folder.symmetry.normalized(),
                params.folder.mix.normalized(),
                0.0, 0.0, 0.0,
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
            _ => [0.5; 6], // placeholder pages
        }
    }

    /// Apply an encoder delta. Each tick = 1/128 of the parameter range.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        if let Some(param) = self.resolve_param_mut(idx, params) {
            let step = (param.max - param.min) / 128.0;
            param.nudge(delta as f32 * step);
        }
    }

    /// Shift+encoder: snap to coarse jump points defined by the cell type.
    pub fn snap_encoder(&self, idx: usize, delta: i8, params: &mut ParamSnapshot) {
        let fmt = self.val_formats()[idx];
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
            _ => None,
        }
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

