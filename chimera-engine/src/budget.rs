//! The formant budget, derived from the cost model so that ADR 0026's
//! four-voice floor (three with a FRMT, the owner's 2026-10-12 ruling)
//! holds for the costliest table Sound under the reference heavy routing (`ModRouting::HEAVY`; never typed in). Runtime admission
//! still bills each voice's real cost.

use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::engine::AlgoEngine;
use crate::dsp::algo::kinds::{Kinds, OpKind};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::plan::OPS;
use crate::dsp::filter::{CLEAN_MOVE_COST, FILTER_LEVEL_COST};
use crate::dsp::fx_bus::FxBus;
use crate::dsp::knobs::Knobs;
use crate::dsp::lfo::LfoClock;
use crate::dsp::voice::{HighNote, Voice};
use crate::hw::{CPU_HZ_REV_V, Cost, FLOOR_VOICES, FLOOR_VOICES_FORMANT, SampleBudget};
use crate::modulation::ModRouting;

const TABLE_OP: AlgoOpParams = {
    let mut o = AlgoOpParams::DEFAULT;
    (o.level, o.feedback) = (99, 1);
    o
};

/// A16/A17 with six table operators at LEVEL 99 and FDBK 1: 832.
pub const COSTLIEST_TABLE: AlgoParams =
    AlgoParams::new(AlgoId::A16, AlgoId::A17, 0, Kinds::TABLE, [TABLE_OP; OPS]);

/// What a voice costs beyond its engine at the heaviest routing.
pub const HEAVY_OVERHEAD: Cost =
    Cost(ModRouting::HEAVY.0 + CLEAN_MOVE_COST.0 + FILTER_LEVEL_COST.0 + Voice::CHAIN_COST.0);

/// What admission reserves beside the bus every block, before any voice:
/// the knobs' settle (ADR 0116) and the LFO clocks (ADR 0135).
pub const CONTROL_RESERVE: Cost = Cost(Knobs::RESERVE.0 + LfoClock::RESERVE.0);

/// What the costliest table voice leaves of a share of `floor` voices,
/// for formant and noise operators' charges, less a formant Part's reach
/// (`part` once, `voice` a voice).
const fn formant_budget(floor: u32, part: Cost, voice: Cost) -> Cost {
    Cost(
        (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0
            - FxBus::COST_TAPE.0
            - CONTROL_RESERVE.0
            - part.0)
            / floor
            - HEAVY_OVERHEAD.0
            - AlgoEngine::cost(&COSTLIEST_TABLE, &[false; OPS]).0
            - voice.0,
    )
}

/// A Sound of tables and noise bands: ADR 0026's four-voice floor.
pub const FORMANT_BUDGET: Cost = formant_budget(FLOOR_VOICES, Cost::ZERO, Cost::ZERO);

/// A Sound with a FRMT: the owner's three-voice floor (2026-10-12), with
/// recost's reach billed (ADR 0131).
pub const FORMANT_BUDGET_FRMT: Cost = formant_budget(
    FLOOR_VOICES_FORMANT,
    HighNote::PART_COST,
    HighNote::VOICE_COST,
);

/// Formant operators the three-voice budget holds, each with a link into it.
pub const FORMANT_N: u32 = FORMANT_BUDGET_FRMT.0 / OpKind::Formant.charge(true);

/// The same with no link into any: a FRMT carrier's or an unmodulated one's.
pub const FORMANT_N_UNLINKED: u32 = FORMANT_BUDGET_FRMT.0 / OpKind::Formant.charge(false);
