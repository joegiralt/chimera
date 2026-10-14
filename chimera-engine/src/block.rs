//! Parameter description and access, implemented once per block type.
//!
//! Spec: docs/superpowers/specs/2026-09-23-engine-refactor-design.md §1.
//! Each values struct keeps real field types and implements `Block`; its
//! `ParamSpec` table is a `static` in the block's own module.

use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::clampf;

/// A switch's two names, as `ValFmt::Names` shows it.
pub const OFF_ON: [&str; 2] = ["OFF", "ON"];

/// Fine ticks to a step: a pushed encoder's turn (ADR 0140).
pub const FINE_DIV: f32 = 10.0;

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
    /// The 2040's KEYBD: a route amount that reads OFF, HALF or FULL at
    /// 0, 64 and 127 (filter-types spec § 3), else as `Route`.
    Keybd,
    /// A slider shown in its unit (spec § 1's laws).
    Law(crate::dsp::modulator::law::Law),
    /// ORBIT's tempo, whole BPM over `Bpm::MIN..=Bpm::MAX`.
    Bpm,
    /// A frequency of `base · 2^(v / per_octave)` Hz over a stored `v`
    /// (FRMT, BW). Snaps each octave. `fmt_val` takes the stored value.
    LogHz { base: u16, per_octave: u8 },
    /// VOWEL, 0–100 over U O A E I, 25 apart. Snaps each vowel.
    /// `fmt_val` takes the stored value.
    Vowel,
}

impl ValFmt {
    /// Coarse snap points in normalized 0..1 space.
    pub fn snap_points(self) -> &'static [f32] {
        match self {
            ValFmt::Uni | ValFmt::Law(_) => &[0.0, 100.0 / 127.0, 1.0],
            ValFmt::Bi | ValFmt::Pan | ValFmt::Route | ValFmt::Keybd => {
                &[0.0, 20.0 / 127.0, 64.0 / 127.0, 107.0 / 127.0, 1.0]
            }
            // Discrete: shift-encoder jumps to 0 or max
            ValFmt::Int(_) | ValFmt::OneBased(_) | ValFmt::Names(_) => &[0.0, 1.0],
            ValFmt::Signed(_) => &[0.0, 0.5, 1.0],
            // MIN, the default, MAX.
            ValFmt::Bpm => &[0.0, BPM_SNAP, 1.0],
            // `ParamSpec::snap_target` steps LogHz by octaves.
            ValFmt::LogHz { .. } => &[0.0, 1.0],
            ValFmt::Vowel => &[0.0, 0.25, 0.5, 0.75, 1.0],
        }
    }

    pub fn is_bipolar(self) -> bool {
        matches!(
            self,
            ValFmt::Bi | ValFmt::Pan | ValFmt::Route | ValFmt::Keybd | ValFmt::Signed(_)
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
            ValFmt::Uni
            | ValFmt::Bi
            | ValFmt::Pan
            | ValFmt::Route
            | ValFmt::Keybd
            | ValFmt::Law(_)
            | ValFmt::Bpm
            | ValFmt::LogHz { .. }
            | ValFmt::Vowel => 127,
        }
    }
}

/// `ValFmt::Bpm`'s middle snap point, a third of the way: 120 in 30–300.
const BPM_SNAP: f32 = 1.0 / 3.0;
const _: () = {
    use crate::orbit::clock::Bpm;
    assert!(3 * (Bpm::DEFAULT.get() - Bpm::MIN) == Bpm::MAX - Bpm::MIN);
};

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

/// The unit of a param's encoder tick (`ParamSpec::step`), and so how the
/// 0..1 display spans its range. A byte, so `ParamSpec` stays its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// The value's own unit; the display linear in the value.
    Linear,
    /// Octaves (a tick multiplies by `2^step`); the display linear in
    /// octaves: a frequency (CUTOFF, ADR 0107). The range's `min` is above 0.
    Octaves,
}

/// How a change of a parameter reaches the sound (ADR 0112). Every spec
/// declares one through its constructor, so no knob steps by omission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// No steps in the sound: `Smoothing` says what carries a change.
    Continuous(Smoothing),
    /// Whole steps, heard as they come: `Change` says how.
    Stepped(Change),
}

impl Motion {
    /// The Part's knobs glide it (`Smoothing::Knobs`), by this τ.
    pub const fn knob(self) -> Option<GlideMs> {
        match self {
            Motion::Continuous(Smoothing::Knobs(t)) => Some(t),
            _ => None,
        }
    }

    /// Continuous in the sound, whatever eases it: a knob that turns
    /// smoothly, not one that clicks between values.
    pub const fn is_continuous(self) -> bool {
        matches!(self, Motion::Continuous(_))
    }
}

/// What carries a continuous value from one setting to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Smoothing {
    /// The Part's knobs (`dsp::knobs`): once a block, a one-pole of this τ,
    /// linear in the spec's `Scale` (CUTOFF in octaves), before the routes'
    /// offsets. Only a Sound's params: the knobs are a Part's.
    Knobs(GlideMs),
    /// Its own DSP: an effect's or the mixer's ease, a crossfade of two
    /// read heads, or an envelope or oscillator travelling to it (a time or
    /// a rate bends a path, it never jumps a value).
    Own,
    /// Read when a note starts (or a ring steps, or a loop wraps): heard
    /// on the next one, never under a held one.
    NoteOn,
    /// Not heard: a setting, a route's hidden base, a value nothing reads.
    Unheard,
}

impl Smoothing {
    /// A knob's glide (ADR 0107): 30 ms.
    pub const KNOB: Self = Smoothing::Knobs(GlideMs::KNOB);
    /// A stage's: drive, fold, the filter's LEVEL and OUT LEVEL, 20 ms as
    /// a bus setting (`ease::EASE_S`).
    pub const STAGE: Self = Smoothing::Knobs(GlideMs::STAGE);
}

/// How a step is heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// At once: a semitone, a ratio, a channel. The phase runs on, so
    /// nothing clicks; a glide would be a portamento on every turn.
    Snap,
    /// Through a crossfade the DSP runs (a filter TYPE, an ALG, a WAVE).
    Crossfade,
    /// On the next note.
    NoteOn,
}

/// A glide's time constant, ms: 63 % of the way in this long, 95 % in three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlideMs(u8);

impl GlideMs {
    pub const KNOB: Self = Self(30);
    pub const STAGE: Self = Self(20);

    pub const fn ms(self) -> u8 {
        self.0
    }

    /// In seconds.
    pub fn secs(self) -> f32 {
        f32::from(self.0) / 1000.0
    }
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
    /// Change per encoder tick, in `scale`'s unit (today's per-page step).
    pub step: f32,
    pub scale: Scale,
    pub kind: ParamKind,
    /// True only if `Voice` reads the param per block (see `ParamAddr::modulatable`).
    pub modulatable: bool,
    /// How a matrix offset applies.
    pub law: OffsetLaw,
    /// The param's frozen identity: one token, an explicit literal, never the
    /// label. The fixture pins `(id, ident)`, so swapping two ids is caught.
    pub ident: &'static str,
    /// Written to the card. False for a live view of other stored params
    /// (ENV's FORM reads the current MODE's slot), so nothing is stored twice.
    pub stored: bool,
    /// The focus band's gauge, hand-assigned; ARC unless set.
    pub glyph: crate::glyph::FocusGlyph,
    /// Patching, not sound (a filter jack): out of the parameter sweep and
    /// of MIDI learn.
    pub patching: bool,
    /// How a change reaches the sound (ADR 0112). Private: set only by the
    /// constructors, so a choice can never be made to glide (`motion()`):
    ///
    /// ```compile_fail
    /// use chimera_engine::block::{Change, Motion, ParamSpec, Smoothing};
    /// let mut s = ParamSpec::choice(0, "X", chimera_engine::block::ValFmt::Uni, 1.0, 0.0, Change::Snap);
    /// s.motion = Motion::Continuous(Smoothing::KNOB);
    /// ```
    motion: Motion,
}

impl ParamSpec {
    /// How a change reaches the sound (ADR 0112).
    pub const fn motion(&self) -> Motion {
        self.motion
    }

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
        smoothing: Smoothing,
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
            scale: Scale::Linear,
            kind: ParamKind::Continuous,
            modulatable,
            law: OffsetLaw::Linear,
            stored: true,
            ident: "",
            glyph: crate::glyph::FocusGlyph::Arc,
            patching: false,
            motion: Motion::Continuous(smoothing),
        }
    }

    /// Integer-valued, one unit per encoder tick. Its `motion` may still be
    /// continuous: a LEVEL stored in whole steps glides between them.
    #[allow(clippy::too_many_arguments)]
    pub const fn stepped(
        id: u8,
        label: &'static str,
        fmt: ValFmt,
        min: f32,
        max: f32,
        default: f32,
        modulatable: bool,
        motion: Motion,
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
            scale: Scale::Linear,
            kind: ParamKind::Stepped,
            modulatable,
            law: OffsetLaw::Linear,
            stored: true,
            ident: "",
            glyph: crate::glyph::FocusGlyph::Arc,
            patching: false,
            motion,
        }
    }

    /// Discrete choice `0..=max`, one choice per tick. Never modulatable,
    /// never glides: a choice steps, as `change` says.
    pub const fn choice(
        id: u8,
        label: &'static str,
        fmt: ValFmt,
        max: f32,
        default: f32,
        change: Change,
    ) -> Self {
        Self {
            id: ParamId(id),
            label,
            short: None,
            fmt,
            min: 0.0,
            max,
            default,
            step: 1.0,
            scale: Scale::Linear,
            kind: ParamKind::Enum,
            modulatable: false,
            law: OffsetLaw::Linear,
            stored: true,
            ident: "",
            glyph: crate::glyph::FocusGlyph::Arc,
            patching: false,
            motion: Motion::Stepped(change),
        }
    }

    /// `v` clamped to the range; Stepped/Enum rounded to nearest (UI input).
    pub fn quantize(&self, v: f32) -> f32 {
        let v = clampf(v, self.min, self.max);
        match self.kind {
            ParamKind::Continuous => v,
            ParamKind::Stepped | ParamKind::Enum => libm::roundf(v),
        }
    }

    /// `quantize` out of line, for every `Block::set` (UI input): inlined,
    /// each impl's `set`, and each `snap` with it, carried a copy.
    #[inline(never)]
    pub fn quantized(&self, v: f32) -> f32 {
        self.quantize(v)
    }

    /// `v` mapped to 0..1 over the range, by the tick's law.
    #[inline(never)]
    pub fn normalize(&self, v: f32) -> f32 {
        if self.max == self.min {
            return 0.0;
        }
        match self.scale {
            Scale::Linear => (v - self.min) / (self.max - self.min),
            Scale::Octaves => log2(v / self.min) / log2(self.max / self.min),
        }
    }

    /// `normalize`'s inverse: the value at 0..1 `n`.
    #[inline(never)]
    pub fn denormalize(&self, n: f32) -> f32 {
        match self.scale {
            Scale::Linear => self.min + n * (self.max - self.min),
            Scale::Octaves => self.min * exp2(n * log2(self.max / self.min)),
        }
    }

    /// `v` moved `delta` ticks, clamped (and rounded as `quantize` does).
    #[inline(never)]
    pub fn nudged(&self, v: f32, delta: i8) -> f32 {
        let d = f32::from(delta);
        self.quantize(match self.scale {
            Scale::Linear => v + d * self.step,
            Scale::Octaves => v * exp2(d * self.step),
        })
    }

    /// `v` moved `delta` fine ticks, each `1 / FINE_DIV` of a step: an
    /// encoder pushed while it turns (ADR 0140).
    #[inline(never)]
    pub fn nudged_fine(&self, v: f32, delta: i8) -> f32 {
        let d = f32::from(delta) / FINE_DIV;
        self.quantize(match self.scale {
            Scale::Linear => v + d * self.step,
            Scale::Octaves => v * exp2(d * self.step),
        })
    }

    /// Where shift+encoder takes `v`: the format's next snap point up
    /// (`delta > 0`) or down, as a value. Out of line: one copy, not one
    /// per block.
    #[inline(never)]
    pub fn snap_target(&self, v: f32, delta: i8) -> f32 {
        if let ValFmt::LogHz { per_octave, .. } = self.fmt {
            let o = f32::from(per_octave);
            let k = v / o;
            let k = if delta > 0 {
                libm::floorf(k + 0.005) + 1.0
            } else {
                libm::ceilf(k - 0.005) - 1.0
            };
            return clampf(k * o, self.min, self.max);
        }
        let n = self.normalize(v);
        let points = self.fmt.snap_points();
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
        self.denormalize(target)
    }

    /// This spec ticking `1/n` octave a tick (`Scale::Octaves`).
    pub const fn ticks_per_octave(self, n: f32) -> Self {
        Self {
            step: 1.0 / n,
            scale: Scale::Octaves,
            ..self
        }
    }

    /// This spec with its frozen identity.
    pub const fn ident(self, ident: &'static str) -> Self {
        Self { ident, ..self }
    }

    /// This spec as patching, not sound (`patching`).
    pub const fn patching(self) -> Self {
        Self {
            patching: true,
            ..self
        }
    }

    /// This spec as a live view of stored params: never written to the card.
    pub const fn live(self) -> Self {
        Self {
            stored: false,
            ..self
        }
    }

    /// This spec with its focus glyph.
    pub const fn glyph(self, glyph: crate::glyph::FocusGlyph) -> Self {
        Self { glyph, ..self }
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
            OffsetLaw::Linear => clampf(v + off * (self.max - self.min), self.min, self.max),
            OffsetLaw::Octaves(n) => clampf(v * crate::dsp::fast_exp2(n * off), self.min, self.max),
            OffsetLaw::Semitones(n) | OffsetLaw::Cents(n) => {
                clampf(v + off * n, self.min, self.max)
            }
        }
    }

    /// `offset` on a 0..1 display value (the UI's mod bars).
    pub fn offset_normalized(&self, n: f32, off: f32) -> f32 {
        match self.law {
            OffsetLaw::Linear => clampf(n + off, 0.0, 1.0),
            OffsetLaw::Octaves(_) | OffsetLaw::Semitones(_) | OffsetLaw::Cents(_) => {
                self.normalize(self.offset(self.denormalize(n), off))
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
    /// What the value is, as an explicit literal per variant (`"LP24"`): one
    /// token, never from `Debug` or a label, so a cosmetic rename can't move
    /// it. The fixture pins `(code, ident)`, so swapping two codes is caught.
    fn disk_ident(self) -> &'static str;
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
pub const fn find_spec(specs: &'static [ParamSpec], id: ParamId) -> Option<&'static ParamSpec> {
    let mut i = 0;
    while i < specs.len() {
        if specs[i].id.0 == id.0 {
            return Some(&specs[i]);
        }
        i += 1;
    }
    None
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

    /// Out of line, as `get` and `write` are in every impl: inlined, each
    /// block's search and conversions are copied into each default method
    /// below (the UI's), not shared.
    #[inline(never)]
    fn spec(&self, id: ParamId) -> Option<&'static ParamSpec> {
        find_spec(self.specs(), id)
    }

    /// The disk code of Enum `id`'s current value; `None` for any other param.
    fn enum_code(&self, _id: ParamId) -> Option<u8> {
        None
    }

    /// The frozen ident of Enum `id`'s current value (see `DiskCode`); `None`
    /// for any other param.
    fn enum_ident(&self, _id: ParamId) -> Option<&'static str> {
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
            self.write(id, s.quantized(v));
        }
    }

    /// 0..1 display value.
    fn normalized(&self, id: ParamId) -> f32 {
        self.spec(id).map_or(0.0, |s| s.normalize(self.get(id)))
    }

    /// One encoder turn: `delta` ticks of the spec's step.
    fn nudge(&mut self, id: ParamId, delta: i8) {
        if let Some(s) = self.spec(id) {
            self.write(id, s.nudged(self.get(id), delta));
        }
    }

    /// A pushed encoder's turn: a continuous param moves a tenth of its
    /// step a tick; any other moves one plain tick, through `nudge`, so a
    /// block's own stepping (WAVE's budget, ROTATE's wrap) still holds.
    fn nudge_fine(&mut self, id: ParamId, delta: i8) {
        match self.spec(id) {
            Some(s) if s.kind == ParamKind::Continuous => {
                self.write(id, s.nudged_fine(self.get(id), delta))
            }
            _ => self.nudge(id, delta.signum()),
        }
    }

    /// Shift+encoder: jump to the next snap point of the spec's format.
    fn snap(&mut self, id: ParamId, delta: i8) {
        if let Some(s) = self.spec(id) {
            self.set(id, s.snap_target(self.get(id), delta));
        }
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
