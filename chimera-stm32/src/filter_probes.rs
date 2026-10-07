//! The filter probes (`filter-probes`), linked though nothing calls them:
//! `tools/loop_count.py` counts their loops in this ELF. Apart from the
//! bench, whose areas have no room for their ~37 KB.

use chimera_core::dsp::filter::{self, Clean};
use chimera_core::dsp::wavefolder::Wavefolder;
use chimera_core::hw::BLOCK_SIZE;
use chimera_core::params::{FilterParams, FolderParams};

/// One block of one filter path, for `tools/loop_count.py`'s static count.
type FilterProbe = fn(&mut Clean, &mut [f32; BLOCK_SIZE], &FilterParams);

/// Every filter probe, kept in the ELF though nothing calls them.
#[used]
static FILTER_PROBES: &[FilterProbe] = &[
    filter::probe::clean_probe::<2, false>,
    filter::probe::clean_probe::<2, true>,
    filter::probe::clean_probe::<7, false>,
    filter::probe::clean_probe::<7, true>,
];

/// CLEAN's other steady modes (LP1, LP2, BP2, BP4, HP4, NOTCH1).
#[used]
static CLEAN_MODE_PROBES: &[FilterProbe] = &[
    filter::probe::clean_probe::<0, false>,
    filter::probe::clean_probe::<1, false>,
    filter::probe::clean_probe::<3, false>,
    filter::probe::clean_probe::<4, false>,
    filter::probe::clean_probe::<5, false>,
    filter::probe::clean_probe::<6, false>,
];

/// The 2040's probes, REV2 (0) and EXP (1), steady and moving.
type K2040Probe = fn(&mut filter::K2040, &mut [f32; BLOCK_SIZE], &FilterParams);

#[used]
static K2040_PROBES: &[K2040Probe] = &[
    filter::probe::k2040_probe::<filter::k2040::voice::Rev2, false>,
    filter::probe::k2040_probe::<filter::k2040::voice::Rev2, true>,
    filter::probe::k2040_probe::<filter::k2040::voice::Exp, false>,
    filter::probe::k2040_probe::<filter::k2040::voice::Exp, true>,
];

/// DIRTY's probes, one a mode: LP2, BP2, HP2 and NOTCH1 (`DIRTY_MODES`).
type DirtyProbe = fn(&mut filter::Dirty, &mut [f32; BLOCK_SIZE], &FilterParams);

#[used]
static DIRTY_PROBES: &[DirtyProbe] = &[
    filter::probe::dirty_probe::<0>,
    filter::probe::dirty_probe::<1>,
    filter::probe::dirty_probe::<2>,
    filter::probe::dirty_probe::<3>,
];

/// LADDER's probes: LP4 (1) and HP4 (4), steady and moving, and the
/// other modes steady (`LADDER_MODES`).
type LadderProbe = fn(&mut filter::Ladder, &mut [f32; BLOCK_SIZE], &FilterParams);

#[used]
static LADDER_PROBES: &[LadderProbe] = &[
    filter::probe::ladder_probe::<1, false>,
    filter::probe::ladder_probe::<1, true>,
    filter::probe::ladder_probe::<4, false>,
    filter::probe::ladder_probe::<4, true>,
    filter::probe::ladder_probe::<0, false>,
    filter::probe::ladder_probe::<2, false>,
    filter::probe::ladder_probe::<3, false>,
];

/// The taps' pass over a moving block's `g` and `k`, shared by every type.
#[used]
static TAP_SHAPE_PROBE: fn(&mut [f32; BLOCK_SIZE]) = filter::probe::tap_shape_probe;

/// The tap-VCA's and the folder's tapped loops.
#[used]
static TAP_VCA_PROBE: fn(&mut [f32; BLOCK_SIZE]) = filter::probe::tap_vca_probe;

type FoldProbe = fn(&mut Wavefolder, &mut [f32; BLOCK_SIZE], &FolderParams);

#[used]
static FOLD_TAP_PROBE: FoldProbe = filter::probe::fold_tap_probe;
