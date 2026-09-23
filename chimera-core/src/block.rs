//! Parameter description and access, implemented once per block type.
//!
//! Spec: docs/superpowers/specs/2026-09-23-engine-refactor-design.md §1.
//! Each values struct keeps real field types and implements `Block`; its
//! `ParamSpec` table is a `static` in the block's own module.

/// Display format and shift-snap behavior of a parameter.
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

/// Identifies a parameter within its block type. Stable; never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct ParamId(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// Any value in `min..=max`.
    Continuous,
    /// Integer-valued. UI input rounds; a modulated copy stays fractional and
    /// the DSP truncates as it does today (FM level `as u8`).
    Stepped,
    /// Discrete choice `0..=max` (`min` is 0). Never modulatable.
    Enum,
}

/// Description of one parameter. Lives in flash (`static` tables).
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub id: ParamId,
    pub label: &'static str,
    /// Display format, set explicitly to today's per-slot format.
    pub fmt: ValFmt,
    pub min: f32,
    pub max: f32,
    /// UI reset value only. Initial values come from the values struct's
    /// `Default` and from `Patch::init`.
    pub default: f32,
    /// Value change per encoder tick (today's per-page step).
    pub step: f32,
    pub kind: ParamKind,
    /// True only if `Voice` reads the param per block (see `ParamAddr::modulatable`).
    pub modulatable: bool,
}

impl ParamSpec {
    #[allow(clippy::too_many_arguments)]
    pub const fn continuous(
        id: u8,
        label: &'static str,
        fmt: ValFmt,
        min: f32,
        max: f32,
        default: f32,
        step: f32,
        modulatable: bool,
    ) -> Self {
        Self { id: ParamId(id), label, fmt, min, max, default, step, kind: ParamKind::Continuous, modulatable }
    }

    /// Integer-valued, one unit per encoder tick.
    pub const fn stepped(
        id: u8,
        label: &'static str,
        fmt: ValFmt,
        min: f32,
        max: f32,
        default: f32,
        modulatable: bool,
    ) -> Self {
        Self { id: ParamId(id), label, fmt, min, max, default, step: 1.0, kind: ParamKind::Stepped, modulatable }
    }

    /// Discrete choice `0..=max`, one choice per tick. Never modulatable.
    pub const fn choice(id: u8, label: &'static str, fmt: ValFmt, max: f32, default: f32) -> Self {
        Self {
            id: ParamId(id),
            label,
            fmt,
            min: 0.0,
            max,
            default,
            step: 1.0,
            kind: ParamKind::Enum,
            modulatable: false,
        }
    }

    /// `v` clamped to the range; Stepped/Enum rounded to nearest (UI input).
    pub fn quantize(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        match self.kind {
            ParamKind::Continuous => v,
            ParamKind::Stepped | ParamKind::Enum => libm::roundf(v),
        }
    }

    /// `v` mapped to 0..1 over the range.
    pub fn normalize(&self, v: f32) -> f32 {
        (v - self.min) / (self.max - self.min)
    }
}

/// Look up a spec by id in a block's table.
pub fn find_spec(specs: &'static [ParamSpec], id: ParamId) -> Option<&'static ParamSpec> {
    specs.iter().find(|s| s.id == id)
}

/// Values + description of one block. `get`/`write` convert between the
/// struct's real field types and `f32` at this boundary; DSP code reads the
/// fields directly. Unknown ids read 0.0 and ignore writes.
pub trait Block {
    fn specs(&self) -> &'static [ParamSpec];
    fn get(&self, id: ParamId) -> f32;
    /// Store `v` with no clamping or rounding (integer fields truncate with
    /// `as`). Only `set` and `apply_offset` call this.
    fn write(&mut self, id: ParamId, v: f32);

    fn spec(&self, id: ParamId) -> Option<&'static ParamSpec> {
        find_spec(self.specs(), id)
    }

    /// UI input: clamps to `min..=max`; Stepped/Enum round to nearest.
    fn set(&mut self, id: ParamId, v: f32) {
        if let Some(s) = self.spec(id) {
            self.write(id, s.quantize(v));
        }
    }

    /// 0..1 display value.
    fn normalized(&self, id: ParamId) -> f32 {
        self.spec(id).map_or(0.0, |s| s.normalize(self.get(id)))
    }

    /// One encoder turn: `delta` ticks of the spec's step.
    fn nudge(&mut self, id: ParamId, delta: i8) {
        if let Some(s) = self.spec(id) {
            self.set(id, self.get(id) + delta as f32 * s.step);
        }
    }

    /// Shift+encoder: jump to the next snap point of the spec's format.
    fn snap(&mut self, id: ParamId, delta: i8) {
        let Some(s) = self.spec(id) else { return };
        let n = s.normalize(self.get(id));
        let points = s.fmt.snap_points();
        let target = if delta > 0 {
            points.iter().copied().find(|&sp| sp > n + 0.005).unwrap_or(1.0)
        } else {
            points.iter().rev().copied().find(|&sp| sp < n - 0.005).unwrap_or(0.0)
        };
        self.set(id, s.min + target * (s.max - s.min));
    }
}

/// Apply a modulation offset (spec §4): `(v + off * (max - min)).clamp(min, max)`,
/// written raw. Bit-identical to the former `Param::apply_mod_offset`; never
/// rounds (a Stepped value stays fractional). Audio-thread safe: no allocation.
pub fn apply_offset(blk: &mut dyn Block, id: ParamId, off: f32) {
    if let Some(s) = blk.spec(id) {
        let v = (blk.get(id) + off * (s.max - s.min)).clamp(s.min, s.max);
        blk.write(id, v);
    }
}
