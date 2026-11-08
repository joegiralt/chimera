//! Scala tunings (`.scl` steps, `.kbm` keyboard maps) as a table of
//! per-key pitch offsets from 12-TET. Pure, fixed-size, never panics: a
//! malformed file is refused, never half read. Runs where a file loads,
//! never on the audio thread. Linked into bank 2 (`bank2.x`).

/// The most steps a scale may have.
pub const MAX_STEPS: usize = 128;

/// Why a `.scl` was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SclError {
    /// No description and count lines.
    NoCount,
    /// The count isn't 1..=128.
    BadCount,
    /// Fewer pitch lines than the count.
    TooFew,
    /// A pitch that is neither cents (with a `.`) nor a positive ratio.
    BadPitch,
    /// The last step, the period, isn't above the root.
    BadPeriod,
}

/// Why a `.kbm` was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KbmError {
    /// Fewer values than its header and map size call for.
    Short,
    /// A value out of its range, or not a number.
    BadValue,
}

/// A scale's steps above its root, in cents: step `n` (1-based) is its
/// `n`th pitch line; the last is the period.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    steps: [f32; MAX_STEPS],
    len: u8,
}

/// The non-comment lines of a Scala text, trimmed: `!` starts a comment.
#[inline(always)]
fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|&b| b == b'\n')
        .map(|l| l.trim_ascii())
        .filter(|l| l.first() != Some(&b'!'))
}

/// A line's first word.
#[inline(always)]
fn word(line: &[u8]) -> &[u8] {
    line.split(|b| b.is_ascii_whitespace())
        .next()
        .unwrap_or_default()
}

/// A whole number of up to nine digits: no `FromStr`, whose float and
/// integer machinery would land in bank 1.
#[inline(always)]
fn uint(w: &[u8]) -> Option<u32> {
    if w.is_empty() || w.len() > 9 {
        return None;
    }
    let mut n = 0u32;
    for &b in w {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n * 10 + u32::from(b - b'0');
    }
    Some(n)
}

/// A decimal, `-12.5`, `440.`, `.5` or `440`: up to nine whole digits.
#[inline(always)]
fn decimal(w: &[u8]) -> Option<f32> {
    let (neg, w) = match w.split_first() {
        Some((b'-', r)) => (true, r),
        Some((b'+', r)) => (false, r),
        _ => (false, w),
    };
    let mut parts = w.splitn(2, |&b| b == b'.');
    let int = parts.next()?;
    let frac = parts.next().unwrap_or_default();
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    // Past nine places a fraction is below a float's reach: read, unused.
    if !frac.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let frac = &frac[..frac.len().min(9)];
    let i = if int.is_empty() { 0 } else { uint(int)? };
    let f = if frac.is_empty() { 0 } else { uint(frac)? };
    let mut scale = 1.0f32;
    for _ in 0..frac.len() {
        scale *= 10.0;
    }
    let v = i as f32 + f as f32 / scale;
    Some(if neg { -v } else { v })
}

/// A positive whole number of a ratio.
#[inline(always)]
fn whole(w: &[u8]) -> Option<f32> {
    uint(w).filter(|&n| n > 0).map(|n| n as f32)
}

/// A pitch line's cents: `701.955` (cents have a `.`), `3/2` or `2`.
#[inline(always)]
fn pitch(w: &[u8]) -> Option<f32> {
    let c = if w.contains(&b'.') {
        decimal(w)?
    } else {
        let mut it = w.splitn(2, |&b| b == b'/');
        let num = whole(it.next()?)?;
        let den = it.next().map_or(Some(1.0), whole)?;
        1200.0 * libm::log2f(num / den)
    };
    c.is_finite().then_some(c)
}

impl Scale {
    pub fn parse(text: &[u8]) -> Result<Scale, SclError> {
        // The description line may be blank: only comments are skipped
        // before it.
        let mut it = lines(text);
        it.next().ok_or(SclError::NoCount)?;
        let count = it.find(|l| !l.is_empty()).ok_or(SclError::NoCount)?;
        let len = uint(word(count))
            .map(|n| n as usize)
            .filter(|n| (1..=MAX_STEPS).contains(n))
            .ok_or(SclError::BadCount)?;
        let mut steps = [0.0; MAX_STEPS];
        let mut pitches = it.filter(|l| !l.is_empty());
        for s in &mut steps[..len] {
            let l = pitches.next().ok_or(SclError::TooFew)?;
            *s = pitch(word(l)).ok_or(SclError::BadPitch)?;
        }
        if steps[len - 1] <= 0.0 {
            return Err(SclError::BadPeriod);
        }
        Ok(Scale {
            steps,
            len: len as u8,
        })
    }

    /// Steps an octave (period).
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Degree `d`'s cents above the root, any degree: whole periods plus
    /// the step.
    pub fn cents(&self, d: i32) -> f32 {
        let n = i32::from(self.len.max(1));
        let period = self.steps[self.len() - 1];
        let step = match d.rem_euclid(n) {
            0 => 0.0,
            j => self.steps[j as usize - 1],
        };
        d.div_euclid(n) as f32 * period + step
    }
}

/// A Scala keyboard map: which degree each key plays, and the pitch one
/// key sounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keymap {
    /// Keys a repeat; 0 maps every key in turn to the next degree.
    size: u8,
    first: u8,
    last: u8,
    /// The key that plays degree 0.
    middle: u8,
    reference: u8,
    /// `reference`'s pitch as a MIDI note number (69 is 440 Hz).
    reference_st: f32,
    /// Degrees a repeat; 0 is the scale's own.
    octave: u16,
    /// Each key of a repeat's degree, `None` unmapped (`x`).
    map: [Option<u16>; MAX_STEPS],
}

impl Keymap {
    /// What a `.scl` alone plays: degree 0 on middle C, C at 12-TET's
    /// pitch.
    pub const LINEAR: Keymap = Keymap {
        size: 0,
        first: 0,
        last: 127,
        middle: 60,
        reference: 60,
        reference_st: 60.0,
        octave: 0,
        map: [None; MAX_STEPS],
    };

    pub fn parse(text: &[u8]) -> Result<Keymap, KbmError> {
        // No closure called twice: the machine outliner would share its
        // calls from bank 1 (ADR 0175).
        let mut words = lines(text).filter(|l| !l.is_empty()).map(word);
        // Map size, first, last, middle and reference keys.
        let mut head = [0u16; 5];
        for (i, h) in head.iter_mut().enumerate() {
            let max = if i == 0 { MAX_STEPS as u16 } else { 127 };
            let w = words.next().ok_or(KbmError::Short)?;
            *h = uint(w)
                .filter(|&k| k <= u32::from(max))
                .map(|k| k as u16)
                .ok_or(KbmError::BadValue)?;
        }
        let w = words.next().ok_or(KbmError::Short)?;
        let hz = decimal(w)
            .filter(|h| h.is_finite() && *h > 0.0)
            .ok_or(KbmError::BadValue)?;
        // The formal octave, then the map: one loop, one array (stack).
        let (mut octave, mut map) = (0, [None; MAX_STEPS]);
        for i in 0..=usize::from(head[0]) {
            let w = words.next().ok_or(KbmError::Short)?;
            let d = match w {
                b"x" | b"X" => None,
                w => Some(
                    uint(w)
                        .filter(|&d| d <= 1024)
                        .map(|d| d as u16)
                        .ok_or(KbmError::BadValue)?,
                ),
            };
            match i.checked_sub(1) {
                // `x` there is no octave.
                None => octave = d.ok_or(KbmError::BadValue)?,
                Some(j) => map[j] = d,
            }
        }
        let [size, first, last, middle, reference] = head.map(|v| v as u8);
        Ok(Keymap {
            size,
            first,
            last,
            middle,
            reference,
            reference_st: 69.0 + 12.0 * libm::log2f(hz / 440.0),
            octave,
            map,
        })
    }

    /// The degree `key` plays, `None` if it is unmapped or out of range.
    fn degree(&self, key: u8, steps: usize) -> Option<i32> {
        if key < self.first || key > self.last {
            return None;
        }
        let i = i32::from(key) - i32::from(self.middle);
        if self.size == 0 {
            return Some(i);
        }
        let size = i32::from(self.size);
        let octave = match self.octave {
            0 => steps as i32,
            o => i32::from(o),
        };
        let m = self.map[i.rem_euclid(size) as usize]?;
        Some(i.div_euclid(size) * octave + i32::from(m))
    }
}

/// The degree `key` plays under `map`; `None` is `Keymap::LINEAR`, read
/// from its fields' values, so no image of it is kept (ADR 0251).
// `degree_in`'s `None` arm is `degree` on these.
const _: () =
    assert!(Keymap::LINEAR.size == 0 && Keymap::LINEAR.first == 0 && Keymap::LINEAR.last == 127);

#[inline(always)]
fn degree_in(map: Option<&Keymap>, key: u8, steps: usize) -> Option<i32> {
    match map {
        Some(k) => k.degree(key, steps),
        None => Some(i32::from(key) - i32::from(Keymap::LINEAR.middle)),
    }
}

/// Each key's pitch offset from 12-TET in semitones: what it adds to its
/// note number. A key `map` leaves unmapped or out of its range stays
/// 12-TET (0); every key stays on the MIDI range 0..=127. `None` is
/// `Keymap::LINEAR`.
pub fn offsets(s: &Scale, map: Option<&Keymap>) -> [f32; 128] {
    let mut out = [0.0; 128];
    offsets_into(s, map, &mut out);
    out
}

/// `offsets` into `out`: no table on the caller's stack.
pub fn offsets_into(s: &Scale, map: Option<&Keymap>, out: &mut [f32; 128]) {
    const L: Keymap = Keymap::LINEAR;
    let (reference, middle, reference_st) = match map {
        Some(k) => (k.reference, k.middle, k.reference_st),
        None => (L.reference, L.middle, L.reference_st),
    };
    let at_ref =
        degree_in(map, reference, s.len()).unwrap_or(i32::from(reference) - i32::from(middle));
    let ref_cents = s.cents(at_ref);
    for (key, o) in out.iter_mut().enumerate() {
        *o = match degree_in(map, key as u8, s.len()) {
            Some(d) => {
                let st = reference_st + (s.cents(d) - ref_cents) / 100.0;
                if st.is_finite() {
                    within(st, 0.0, 127.0) - key as f32
                } else {
                    0.0
                }
            }
            None => 0.0,
        };
    }
}

/// `x` within `lo..=hi` (`clamp` links float formatting).
const fn within(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// Named temperaments, rooted on C, as `.scl` text: what the TUNING cell
/// offers before the card's files (`JUST`, `PYTHAG`, `MEANTONE`, `WERCK`).
/// Named statics, so their bytes link with this module (bank 2).
pub static BUILT_IN: [&[u8]; 4] = [&JUST, &PYTHAG, &MEANTONE, &WERCK];

static JUST: [u8; 80] =
    *b"5-limit just intonation\n12\n16/15\n9/8\n6/5\n5/4\n4/3\n45/32\n3/2\n8/5\n5/3\n9/5\n15/8\n2/1\n";
static PYTHAG: [u8; 85] =
    *b"Pythagorean\n12\n256/243\n9/8\n32/27\n81/64\n4/3\n729/512\n3/2\n128/81\n27/16\n16/9\n243/128\n2/1\n";
static MEANTONE: [u8; 119] = *b"Quarter-comma meantone\n12\n76.049\n193.157\n310.265\n386.314\n503.422\n579.471\n696.578\n772.627\n889.735\n1006.843\n1082.892\n2/1\n";
static WERCK: [u8; 112] = *b"Werckmeister III\n12\n90.225\n192.180\n294.135\n390.225\n498.045\n588.270\n696.090\n792.180\n888.270\n996.090\n1092.180\n2/1\n";
