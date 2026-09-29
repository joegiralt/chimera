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
    /// Discrete integer 0..N shown one-based, 1..N+1 (MIDI channel).
    OneBased(u8),
    /// Discrete integer −N..=N shown with a sign (`-3`, `0`, `+3`).
    Signed(u8),
    /// Discrete choice 0..len-1 shown by name.
    Names(&'static [&'static str]),
    /// Stereo position: bipolar like `Bi`, shown as `L64`..`C`..`R63`.
    Pan,
    /// A matrix route's amount (`amount_value`, 0.5 = 0), shown as a
    /// percentage of 127 (spec § 6).
    Route,
    /// A slider shown in its unit (spec § 1's laws).
    Law(crate::dsp::modulator::law::Law),
}

impl ValFmt {
    /// Coarse snap points in normalized 0..1 space.
    pub fn snap_points(self) -> &'static [f32] {
        match self {
            ValFmt::Uni | ValFmt::Law(_) => &[0.0, 100.0 / 127.0, 1.0],
            ValFmt::Bi | ValFmt::Pan | ValFmt::Route => {
                &[0.0, 20.0 / 127.0, 64.0 / 127.0, 107.0 / 127.0, 1.0]
            }
            // Discrete: shift-encoder jumps to 0 or max
            ValFmt::Int(_) | ValFmt::OneBased(_) | ValFmt::Names(_) => &[0.0, 1.0],
            ValFmt::Signed(_) => &[0.0, 0.5, 1.0],
        }
    }

    pub fn is_bipolar(self) -> bool {
        matches!(
            self,
            ValFmt::Bi | ValFmt::Pan | ValFmt::Route | ValFmt::Signed(_)
        )
    }

    /// A choice among a few values (channel, mode, output, type): shown as
    /// text with no value bar.
    pub fn is_discrete(self) -> bool {
        matches!(
            self,
            ValFmt::Int(_) | ValFmt::OneBased(_) | ValFmt::Names(_) | ValFmt::Signed(_)
        )
    }

    /// Max integer value (only meaningful for the discrete variants).
    pub fn max_int(self) -> u8 {
        match self {
            ValFmt::Int(n) | ValFmt::OneBased(n) => n,
            ValFmt::Signed(n) => n.saturating_mul(2),
            ValFmt::Names(names) => names.len().saturating_sub(1) as u8,
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
    /// Integer-valued. UI input rounds; a modulated copy stays fractional,
    /// and the DSP decides what to do with that — Algo LEVEL interpolates
    /// between steps rather than truncating.
    Stepped,
    /// Discrete choice `0..=max` (`min` is 0). Never modulatable.
    Enum,
}

/// How a matrix offset moves a value (spec § 3; ADR 0010 for `Linear`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OffsetLaw {
    /// `v + off·(max − min)`.
    Linear,
    /// `v · 2^(n·off)`: `n` octaves at a full offset (CUTOFF).
    Octaves(f32),
    /// `v + n·off`: `n` semitones at a full offset (PITCH, ADR 0042).
    Semitones(f32),
    /// `v + n·off`: `n` cents at a full offset (FINE, ADR 0042).
    Cents(f32),
}

/// Description of one parameter. Lives in flash (`static` tables).
#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub id: ParamId,
    pub label: &'static str,
    /// Matrix column header when `label` is too wide for it (`CUT`).
    pub short: Option<&'static str>,
    /// Display format, set explicitly to today's per-slot format.
    pub fmt: ValFmt,
    pub min: f32,
    pub max: f32,
    /// UI reset value only. Initial values come from the values struct's
    /// `Default` and from `Sound::init`.
    pub default: f32,
    /// Value change per encoder tick (today's per-page step).
    pub step: f32,
    pub kind: ParamKind,
    /// True only if `Voice` reads the param per block (see `ParamAddr::modulatable`).
    pub modulatable: bool,
    /// How a matrix offset applies.
    pub law: OffsetLaw,
    /// Written to the card. False for a live view of other stored params
    /// (ENV's FORM reads the current MODE's slot), so nothing is stored twice.
    pub stored: bool,
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
        Self {
            id: ParamId(id),
            label,
            short: None,
            fmt,
            min,
            max,
            default,
            step,
            kind: ParamKind::Continuous,
            modulatable,
            law: OffsetLaw::Linear,
            stored: true,
        }
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
        Self {
            id: ParamId(id),
            label,
            short: None,
            fmt,
            min,
            max,
            default,
            step: 1.0,
            kind: ParamKind::Stepped,
            modulatable,
            law: OffsetLaw::Linear,
            stored: true,
        }
    }

    /// Discrete choice `0..=max`, one choice per tick. Never modulatable.
    pub const fn choice(id: u8, label: &'static str, fmt: ValFmt, max: f32, default: f32) -> Self {
        Self {
            id: ParamId(id),
            label,
            short: None,
            fmt,
            min: 0.0,
            max,
            default,
            step: 1.0,
            kind: ParamKind::Enum,
            modulatable: false,
            law: OffsetLaw::Linear,
            stored: true,
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
        if self.max == self.min {
            return 0.0;
        }
        (v - self.min) / (self.max - self.min)
    }

    /// This spec as a live view of stored params: never written to the card.
    pub const fn live(self) -> Self {
        Self {
            stored: false,
            ..self
        }
    }

    /// This spec with a short column header.
    pub const fn short(self, short: &'static str) -> Self {
        Self {
            short: Some(short),
            ..self
        }
    }

    /// This spec with the octave law.
    pub const fn octaves(self, n: f32) -> Self {
        Self {
            law: OffsetLaw::Octaves(n),
            ..self
        }
    }

    /// This spec with the semitone law.
    pub const fn semitones(self, n: f32) -> Self {
        Self {
            law: OffsetLaw::Semitones(n),
            ..self
        }
    }

    /// This spec with the cent law.
    pub const fn cents(self, n: f32) -> Self {
        Self {
            law: OffsetLaw::Cents(n),
            ..self
        }
    }

    /// `v` moved by a matrix offset `off`, clamped to the range.
    pub fn offset(&self, v: f32, off: f32) -> f32 {
        match self.law {
            OffsetLaw::Linear => (v + off * (self.max - self.min)).clamp(self.min, self.max),
            OffsetLaw::Octaves(n) => (v * crate::dsp::fast_exp2(n * off)).clamp(self.min, self.max),
            OffsetLaw::Semitones(n) | OffsetLaw::Cents(n) => {
                (v + off * n).clamp(self.min, self.max)
            }
        }
    }

    /// `offset` on a 0..1 display value (the UI's mod bars).
    pub fn offset_normalized(&self, n: f32, off: f32) -> f32 {
        match self.law {
            OffsetLaw::Linear => (n + off).clamp(0.0, 1.0),
            OffsetLaw::Octaves(_) | OffsetLaw::Semitones(_) | OffsetLaw::Cents(_) => {
                self.normalize(self.offset(self.min + n * (self.max - self.min), off))
            }
        }
    }
}

/// A stored enum's permanent disk code (ADR 0045). The code is chosen here,
/// by an exhaustive match, never read off the Rust discriminant or the
/// variant order, so reordering or inserting a variant can't renumber a file.
/// Codes are frozen by `tests/fixtures/disk_codes_v1.txt` and never reused.
pub trait DiskCode: Sized + Copy {
    fn disk_code(self) -> u8;
    /// `None`: this firmware has no variant with code `c`.
    fn from_disk_code(c: u8) -> Option<Self>;
}

/// Runs `f` on a decoded code and says whether there was one: the body of a
/// `set_enum_code` arm.
pub fn apply_code<T>(v: Option<T>, f: impl FnOnce(T)) -> bool {
    v.map(f).is_some()
}

/// `c` if it is a value of Enum `id` in `specs`, for the params whose code is
/// the stored value itself (LFO SHAPE, chorus MODE, a wave index).
pub fn identity_code(specs: &'static [ParamSpec], id: ParamId, c: u8) -> Option<u8> {
    find_spec(specs, id).and_then(|s| (f32::from(c) <= s.max).then_some(c))
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

    /// The disk code of Enum `id`'s current value; `None` for any other param.
    fn enum_code(&self, _id: ParamId) -> Option<u8> {
        None
    }

    /// Store the value with disk code `code` in Enum `id`. `false`: this
    /// firmware doesn't know the code, or the value isn't allowed now (a MODE
    /// its KIND lacks); nothing is written.
    fn set_enum_code(&mut self, _id: ParamId, _code: u8) -> bool {
        false
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
            points
                .iter()
                .copied()
                .find(|&sp| sp > n + 0.005)
                .unwrap_or(1.0)
        } else {
            points
                .iter()
                .rev()
                .copied()
                .find(|&sp| sp < n - 0.005)
                .unwrap_or(0.0)
        };
        self.set(id, s.min + target * (s.max - s.min));
    }
}

/// Apply a modulation offset (spec §4) by the spec's law, written raw.
/// Bit-identical to the former `Param::apply_mod_offset`; never rounds (a
/// Stepped value stays fractional). Audio-thread safe: no allocation.
pub fn apply_offset(blk: &mut dyn Block, id: ParamId, off: f32) {
    if let Some(s) = blk.spec(id) {
        blk.write(id, s.offset(blk.get(id), off));
    }
}
