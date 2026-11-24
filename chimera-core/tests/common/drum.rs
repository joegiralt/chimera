// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM rigs: a Part 1 DRUM Instrument on a kit, its budgets, and the
//! hits its voices play.

use chimera_core::MidiChannel;
use chimera_core::dsp::drum::types::{DrumHit, Kit};
use chimera_core::dsp::drum::view::KitWriter;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::instrument::Instrument;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::project::PartId;

use super::InstRig;

/// Rev V's budget: what fits the unit (#951).
pub const REV_V_BUDGET: SampleBudget = SampleBudget::for_cpu(CPU_HZ_REV_V);

/// Twice rev V's: where a test pins how hits play (all twelve pads at
/// once) or master-tape's bus leaves the kit short, not what fits rev V,
/// which `drum_bill_test` pins (#951).
pub const PLAY_BUDGET: SampleBudget = SampleBudget::for_cpu(2 * CPU_HZ_REV_V);

/// `writer` locked for one test, the view reset to boot: the tests in a
/// binary share the process's one view, and each starts from the same one
/// whatever ran before it (#1138).
pub fn locked(
    writer: &'static std::sync::Mutex<KitWriter>,
) -> std::sync::MutexGuard<'static, KitWriter> {
    let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
    w.reset();
    w
}

/// A rig on `budget` whose Part 1 (channel 1) is DRUM playing `kit`, the
/// other Parts on channel 16 playing `others`; every Part's struck pads
/// taken.
pub fn drum_rig(w: &mut KitWriter, budget: SampleBudget, others: &Kit, kit: &Kit) -> InstRig {
    for p in PartId::ALL {
        w.publish(p, others);
        let _ = w.take_hit(p);
    }
    w.publish(PartId::ALL[0], kit);
    let mut r = InstRig::with_budget(budget);
    for (i, part) in r.shared.parts.iter_mut().enumerate() {
        part.mix.channel = MidiChannel::new(if i == 0 { 0 } else { 15 }).unwrap();
    }
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Drum);
    r
}

/// Each sounding voice's hit, by voice.
pub fn drum_hits(inst: &Instrument) -> impl Iterator<Item = (usize, DrumHit)> + '_ {
    (0..MAX_VOICES).filter_map(|v| inst.voice(v).drum_hit().map(|h| (v, h)))
}
