//! A modulator slot's page, per TYPE, MODE and FORM (spec § 1 "Page",
//! § UI), as `const` data.

use crate::block::{ParamId, ValFmt};
use crate::dsp::modulator::law::Law;
use crate::dsp::modulator::{EnvSpeed, Func, LfoForm};
use crate::params::EnvParams as E;
use crate::ui::view::EnvKind;

/// One cell of a modulator page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelSlot {
    /// A parameter of the slot's own block, with its label and readout.
    Param {
        id: ParamId,
        label: &'static str,
        fmt: ValFmt,
    },
    /// A fixed readout: dimmed and inert (FUNC's MODE, always LFO).
    Fixed {
        label: &'static str,
        text: &'static str,
    },
}

pub struct ModPanel {
    pub slots: [Option<PanelSlot>; 6],
}

const fn p(id: ParamId, label: &'static str, fmt: ValFmt) -> Option<PanelSlot> {
    Some(PanelSlot::Param { id, label, fmt })
}

const TYPE: Option<PanelSlot> = p(E::TYPE, "TYPE", ValFmt::Names(&["A", "B"]));
const MODE: Option<PanelSlot> = p(E::MODE, "MODE", ValFmt::Names(&["ENV", "LFO", "BURST"]));
/// In `EnvForm::ALL`'s order.
const FORM_ENV: Option<PanelSlot> = p(E::FORM, "FORM", ValFmt::Names(&["AD", "AHR", "CYCLE"]));
/// In `LfoForm::ALL`'s order.
const FORM_LFO: Option<PanelSlot> = p(E::FORM, "FORM", ValFmt::Names(&["FREE", "SYNC", "LFV"]));

/// A · D · S / R · H · TYPE, times on SPEED's ranges.
const fn a(s: EnvSpeed) -> ModPanel {
    ModPanel {
        slots: [
            p(E::ATTACK, "ATTACK", ValFmt::Law(Law::Attack(s))),
            p(E::DECAY, "DECAY", ValFmt::Law(Law::DecRel(s))),
            p(E::SUSTAIN, "SUSTAIN", ValFmt::Law(Law::Pct)),
            p(E::RELEASE, "RELEASE", ValFmt::Law(Law::DecRel(s))),
            p(E::HOLD, "HOLD", ValFmt::Law(Law::Hold(s))),
            TYPE,
        ],
    }
}

static A_FAST: ModPanel = a(EnvSpeed::Fast);
static A_MED: ModPanel = a(EnvSpeed::Med);
static A_SLOW: ModPanel = a(EnvSpeed::Slow);

/// MODE · RISE · FALL / SHAPE · FORM · TYPE, labels by MODE and FORM.
static B_ENV: ModPanel = ModPanel {
    slots: [
        MODE,
        p(E::RISE, "RISE", ValFmt::Law(Law::BTime)),
        p(E::FALL, "FALL", ValFmt::Law(Law::BTime)),
        p(E::SHAPE, "SHAPE", ValFmt::Law(Law::Curve)),
        FORM_ENV,
        TYPE,
    ],
};
/// FREE and SYNC (RISE reads RATE until #44 gives SYNC a clock).
static B_LFO: ModPanel = ModPanel {
    slots: [
        MODE,
        p(E::RISE, "RATE", ValFmt::Law(Law::BRate)),
        p(E::FALL, "PHASE", ValFmt::Law(Law::Phase)),
        p(E::SHAPE, "TILT", ValFmt::Law(Law::Tilt)),
        FORM_LFO,
        TYPE,
    ],
};
static B_LFV: ModPanel = ModPanel {
    slots: [
        MODE,
        p(E::RISE, "RATE", ValFmt::Law(Law::BRate)),
        p(E::FALL, "DELTA", ValFmt::Law(Law::Pct)),
        p(E::SHAPE, "SLEW", ValFmt::Law(Law::Pct)),
        FORM_LFO,
        TYPE,
    ],
};
static B_BURST: ModPanel = ModPanel {
    slots: [
        MODE,
        p(E::RISE, "RATE", ValFmt::Law(Law::BurstRate)),
        p(E::FALL, "LENGTH", ValFmt::Law(Law::BurstLen)),
        p(E::SHAPE, "TILT", ValFmt::Law(Law::Tilt)),
        FORM_ENV,
        TYPE,
    ],
};

pub fn env_panel(k: EnvKind) -> &'static ModPanel {
    match k {
        EnvKind::A(EnvSpeed::Fast) => &A_FAST,
        EnvKind::A(EnvSpeed::Med) => &A_MED,
        EnvKind::A(EnvSpeed::Slow) => &A_SLOW,
        EnvKind::B(Func::Env(_)) => &B_ENV,
        EnvKind::B(Func::Lfo(LfoForm::Lfv)) => &B_LFV,
        EnvKind::B(Func::Lfo(_)) => &B_LFO,
        EnvKind::B(Func::Burst(_)) => &B_BURST,
    }
}
