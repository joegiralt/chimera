//! The formant budget, derived from the cost model so that ADR 0026's
//! four-voice floor holds for the costliest table Sound under the reference
//! heavy routing (`ModRouting::HEAVY`; never typed in). Runtime admission
//! still bills each voice's real cost.

use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::engine::AlgoEngine;
use crate::dsp::algo::kinds::{Kinds, OpKind};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::plan::OPS;
use crate::dsp::filter::{CLEAN_MOVE_COST, FILTER_LEVEL_COST};
use crate::dsp::fx_bus::FxBus;
use crate::dsp::voice::Voice;
use crate::hw::{CPU_HZ_REV_V, Cost, FLOOR_VOICES, SampleBudget};
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

/// What the costliest table voice leaves of a floor voice's share, for
/// formant and noise operators' charges.
pub const FORMANT_BUDGET: Cost = Cost(
    (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0 - FxBus::COST_TAPE.0) / FLOOR_VOICES
        - HEAVY_OVERHEAD.0
        - AlgoEngine::cost(&COSTLIEST_TABLE, &[false; OPS]).0,
);

/// Formant operators the budget allows at six table-equivalent others.
pub const FORMANT_N: u32 = FORMANT_BUDGET.0 / OpKind::Formant.charge();
