//! `Block` trait semantics (spec §1) and per-block conformance.

use chimera_core::block::{apply_offset, Block, ParamId, ParamKind, ParamSpec, ValFmt};
use chimera_core::dsp::pizza::PizzaParams;

/// A block with one param of each kind.
#[derive(Default)]
struct Probe {
    c: f32,
    s: f32,
    e: u8,
}

const C: ParamId = ParamId(0);
const S: ParamId = ParamId(1);
const E: ParamId = ParamId(2);

static PROBE_SPECS: [ParamSpec; 3] = [
    ParamSpec::continuous(0, "C", ValFmt::Bi, -1.0, 1.0, 0.0, 0.25, true),
    ParamSpec::stepped(1, "S", ValFmt::Int(10), 0.0, 10.0, 5.0, true),
    ParamSpec::choice(2, "E", ValFmt::Int(3), 3.0, 0.0),
];

impl Block for Probe {
    fn specs(&self) -> &'static [ParamSpec] {
        &PROBE_SPECS
    }
    fn get(&self, id: ParamId) -> f32 {
        match id {
            C => self.c,
            S => self.s,
            E => self.e as f32,
            _ => 0.0,
        }
    }
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            C => self.c = v,
            S => self.s = v,
            E => self.e = v as u8,
            _ => {}
        }
    }
}

#[test]
fn choice_is_enum_and_never_modulatable() {
    assert_eq!(PROBE_SPECS[2].kind, ParamKind::Enum);
    assert_eq!(PROBE_SPECS[2].min, 0.0);
    assert!(!PROBE_SPECS[2].modulatable);
}

#[test]
fn set_clamps_to_range() {
    let mut p = Probe::default();
    p.set(C, 5.0);
    assert_eq!(p.c, 1.0);
    p.set(C, -5.0);
    assert_eq!(p.c, -1.0);
}

#[test]
fn set_keeps_continuous_fraction() {
    let mut p = Probe::default();
    p.set(C, 0.3);
    assert_eq!(p.c, 0.3);
}

#[test]
fn set_rounds_stepped_and_enum() {
    let mut p = Probe::default();
    p.set(S, 4.6);
    assert_eq!(p.s, 5.0);
    p.set(S, 4.4);
    assert_eq!(p.s, 4.0);
    p.set(E, 2.5);
    assert_eq!(p.e, 3);
    p.set(E, 7.0);
    assert_eq!(p.e, 3);
}

#[test]
fn nudge_moves_by_spec_step_and_clamps() {
    let mut p = Probe { c: 0.0, s: 5.0, e: 0 };
    p.nudge(C, 2);
    assert_eq!(p.c, 0.5);
    p.nudge(S, -1);
    assert_eq!(p.s, 4.0);
    p.nudge(E, 5);
    assert_eq!(p.e, 3);
}

#[test]
fn snap_follows_format_points() {
    // Bi points (normalized): 0, 20/127, 64/127, 107/127, 1. c = 0.0 is n = 0.5.
    let mut p = Probe::default();
    p.snap(C, 1);
    assert_eq!(p.c, -1.0 + (107.0 / 127.0) * 2.0);
    // Int points: 0, 1 → jumps to max / min.
    p.s = 5.0;
    p.snap(S, 1);
    assert_eq!(p.s, 10.0);
    p.snap(S, -1);
    assert_eq!(p.s, 0.0);
}

#[test]
fn normalized_uses_spec_range() {
    let p = Probe::default();
    assert_eq!(p.normalized(C), 0.5);
}

#[test]
fn unknown_id_is_inert() {
    let mut p = Probe::default();
    let x = ParamId(9);
    assert!(p.spec(x).is_none());
    assert_eq!(p.get(x), 0.0);
    p.set(x, 1.0);
    p.nudge(x, 1);
    p.snap(x, 1);
    apply_offset(&mut p, x, 1.0);
    assert_eq!((p.c, p.s, p.e), (0.0, 0.0, 0));
}

#[test]
fn apply_offset_is_the_old_formula_and_never_rounds() {
    let mut p = Probe { c: 0.0, s: 5.0, e: 0 };
    apply_offset(&mut p, S, 0.03);
    assert_eq!(p.s, (5.0f32 + 0.03f32 * (10.0 - 0.0)).clamp(0.0, 10.0));
    assert_ne!(p.s, 5.0); // fractional: Stepped modulation is not rounded
    apply_offset(&mut p, C, -0.1);
    assert_eq!(p.c, (0.0f32 + -0.1f32 * (1.0 - -1.0)).clamp(-1.0, 1.0));
    apply_offset(&mut p, C, 2.0);
    assert_eq!(p.c, 1.0);
}

// ── Per-block conformance ────────────────────────────────────────────

/// Every spec's `default` equals the values struct's `Default` (Global Constraints).
fn assert_defaults(name: &str, b: &dyn Block) {
    for s in b.specs() {
        assert_eq!(b.get(s.id), s.default, "{name}.{}: Default vs spec default", s.label);
    }
}

/// `get` returns what `set` stored at both ends; out-of-range input clamps.
fn assert_roundtrip(name: &str, b: &mut dyn Block) {
    for s in b.specs() {
        for (input, want) in [(s.max, s.max), (s.min, s.min), (s.max + 1000.0, s.max), (s.min - 1000.0, s.min)] {
            b.set(s.id, input);
            assert_eq!(b.get(s.id), want, "{name}.{}: set({input})", s.label);
        }
    }
}

fn conforms(name: &str, mut b: impl Block) {
    assert_defaults(name, &b);
    assert_roundtrip(name, &mut b);
}

#[test]
fn pizza_conforms() {
    conforms("pizza", PizzaParams::default());
}

#[test]
fn modal_conforms() {
    conforms("modal", chimera_core::dsp::modal::ModalParams::default());
}

#[test]
fn drive_conforms() {
    conforms("drive", chimera_core::params::DriveParams::default());
}

#[test]
fn filter_conforms() {
    conforms("filter", chimera_core::params::FilterParams::default());
}

/// Spec §1: defaults differ per instance — the snapshot's filter is fully open.
#[test]
fn snapshot_filter_starts_open() {
    assert_eq!(chimera_core::params::FilterParams::default().cutoff, 1000.0);
    assert_eq!(chimera_core::params::ParamSnapshot::default().filter.cutoff, 20000.0);
}

#[test]
fn folder_conforms() {
    conforms("folder", chimera_core::params::FolderParams::default());
}

#[test]
fn env_conforms() {
    conforms("env", chimera_core::params::EnvParams::default());
}
