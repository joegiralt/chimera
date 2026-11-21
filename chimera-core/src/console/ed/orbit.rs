//! ORBIT (ADR 0289, slice 5): its own cells' `act` lines, the song's lines
//! in `since`'s ORBIT section, and the commands that do what the box's
//! SCN, CHRD, CUSTOM and NOTES do. Every number on the wire counts from
//! 1, as the box shows it: scenes 1–8, rings 1–5, segments 1–8, degrees
//! 1–12 (I–XII), steps 1–32. Typed here, at the boundary; applied in the
//! UI pass through the box's own paths (`ui::ed_cue::orbit`).
//!
//! ```text
//! act "<label>" <CELL>                     (describe: an ORBIT cell no param)
//! song <playing|stopped> clock <INT|EXT> scene <n> queued <n|-> edited <yes|no>
//! slot <n> <saved|neutral> rev <r>
//! chord <seg> <degree>
//! scale <mask>                             (CUSTOM's set: bit i, i semitones up)
//! step <ring> <i> <note>
//! pool <ring> <note>…
//! ```

use core::fmt::{self, Write};

use super::lit::{QUOTE, QUOTE_SHUT, lit, uint};
use crate::orbit::clock::ClockSource;
use crate::orbit::scene::{OrbitSong, SCENES, Scene, SceneIdx, SlotRev};
use crate::orbit::walk::Pool;
use crate::orbit::{MAX_POOL, MAX_SEGS, MAX_STEPS, RINGS, RingIdx};
use crate::project::Held;
use crate::ui::UiState;
use crate::ui::orbit::OrbitCell;
use chimera_theory::{Degree, Interval, Note, Scale};

/// Steps a ring keeps notes for, as an index bound.
const STEPS: usize = MAX_STEPS as usize;

/// `act "<label>" <CELL>`: the cell's label as the box draws it, and its
/// token, so the editor can place the command that does its job.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.act_line"))]
pub(super) fn act_line(c: OrbitCell, w: &mut dyn Write) -> fmt::Result {
    w.write_str(ACT.get())?;
    w.write_str(QUOTE.get())?;
    w.write_str(c.label())?;
    w.write_str(QUOTE_SHUT.get())?;
    w.write_str(cell_token(c))?;
    w.write_char('\n')
}

/// An ORBIT cell's token: one each, an exhaustive match.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.orbit.cell_token")
)]
fn cell_token(c: OrbitCell) -> &'static str {
    match c {
        OrbitCell::Ring => RING.get(),
        OrbitCell::Degree => DEGREE.get(),
        OrbitCell::Cursor => CURSOR.get(),
        OrbitCell::Set => SET.get(),
        OrbitCell::Note => NOTE.get(),
        OrbitCell::Learn => LEARN.get(),
        OrbitCell::Clear => CLEAR.get(),
        OrbitCell::Play => PLAY.get(),
        OrbitCell::Scene => SCENE.get(),
        OrbitCell::Save => SAVE.get(),
        OrbitCell::Switch => SWITCH.get(),
    }
}

/// A slot and its rev as `since`'s `slot` line said them: the witness a
/// command that writes the slot carries. Its fields are this module's:
/// only the parse makes one, from the wire's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    slot: SceneIdx,
    rev: SlotRev,
}

impl Seen {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.seen_slot"))]
    pub const fn slot(self) -> SceneIdx {
        self.slot
    }
}

/// A segment of the progression, 1–8 on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seg(u8);

impl Seg {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.seg_index"))]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A STEP ring's step, 1–32 on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step(u8);

impl Step {
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.orbit.step_index")
    )]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The CUSTOM set as a mask, bit `i` the note `i` semitones above the
/// root: 1–4095 with bit 0, the root, set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScaleMask(Scale);

impl ScaleMask {
    /// The set's mask.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.mask_of"))]
    pub fn of(s: Scale) -> u16 {
        let mut m = 0;
        let mut i = 0;
        while let Some(&iv) = Interval::ALL.get(i) {
            if s.contains(iv) {
                m |= 1 << i;
            }
            i += 1;
        }
        m
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.mask_new"))]
    fn new(m: u32) -> Option<ScaleMask> {
        if m & 1 == 0 || m >= 1 << Interval::ALL.len() {
            return None;
        }
        let mut s = Scale::custom(&[]);
        let mut i = 1;
        while let Some(&iv) = Interval::ALL.get(i) {
            if m & 1 << i != 0 {
                s = s.toggle(iv);
            }
            i += 1;
        }
        Some(ScaleMask(s))
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.mask_get"))]
    pub const fn get(self) -> Scale {
        self.0
    }
}

/// `ed orbit play|stop`: the transport as asked, never a toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Play,
    Stop,
}

impl Transport {
    /// Whether the transport runs, so asked.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.runs"))]
    pub const fn runs(self) -> bool {
        match self {
            Transport::Play => true,
            Transport::Stop => false,
        }
    }
}

/// One ORBIT command, typed at the boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitCmd {
    /// `ed orbit play|stop`: B6, only when the transport differs.
    Transport(Transport),
    /// `ed scene queue <n>`: SCN's SCENE turned to `n`.
    Queue(SceneIdx),
    /// `ed scene save <n>`: SCN's SAVE into a slot that doesn't ask.
    Save(SceneIdx),
    /// `ed scene save <n> over <rev>`: SAVE OVER answered yes.
    SaveOver(Seen),
    /// `ed scene clear <n> <rev>`: SETTINGS › ORBIT's CLEAR, the playing
    /// scene.
    Clear(Seen),
    /// `ed scene reload <n> <rev>`: RELOAD FROM PROJ, the playing scene.
    Reload(Seen),
    /// `ed scene move <a> <b> <rev a> <rev b>`: the two slots swapped.
    Move(Seen, Seen),
    /// `ed chord <seg> <degree>`: CHRD's DEGREE on a segment.
    Chord(Seg, Degree),
    /// `ed scale <mask>`: CUSTOM's set, a note at a time as IN SCALE.
    Scale(ScaleMask),
    /// `ed step <ring> <i> <note>`: a STEP ring's step.
    Step(RingIdx, Step, Note),
    /// `ed pool <ring> <notes…>`: a POOL ring's notes.
    Pool(RingIdx, Pool),
}

/// The words that start an ORBIT command, each its own usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Orbit,
    Scene,
    Chord,
    Scale,
    Step,
    Pool,
}

impl Family {
    pub const ALL: [Family; 6] = [
        Family::Orbit,
        Family::Scene,
        Family::Chord,
        Family::Scale,
        Family::Step,
        Family::Pool,
    ];

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.word"))]
    fn word(self) -> &'static [u8] {
        match self {
            Family::Orbit => ORBIT_WORD.bytes(),
            Family::Scene => SCENE_WORD.bytes(),
            Family::Chord => CHORD_WORD.bytes(),
            Family::Scale => SCALE_WORD.bytes(),
            Family::Step => STEP_WORD.bytes(),
            Family::Pool => POOL_WORD.bytes(),
        }
    }

    /// The family `w` starts, if any.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.of"))]
    pub fn of(w: &[u8]) -> Option<Family> {
        let mut i = 0;
        while let Some(&f) = Family::ALL.get(i) {
            if w.eq_ignore_ascii_case(f.word()) {
                return Some(f);
            }
            i += 1;
        }
        None
    }

    /// `ERR ed <family> takes …`, whole.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.usage"))]
    pub fn usage(self, w: &mut dyn Write) -> fmt::Result {
        w.write_str(match self {
            Family::Orbit => ORBIT_USAGE.get(),
            Family::Scene => SCENE_USAGE.get(),
            Family::Chord => CHORD_USAGE.get(),
            Family::Scale => SCALE_USAGE.get(),
            Family::Step => STEP_USAGE.get(),
            Family::Pool => POOL_USAGE.get(),
        })
    }

    /// The rest of the line, after the family's word.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.parse"))]
    pub fn parse(self, mut words: crate::console::Words<'_>) -> Option<OrbitCmd> {
        let cmd = match self {
            Family::Orbit => {
                let w = words.next()?;
                if w.eq_ignore_ascii_case(PLAY_WORD.bytes()) {
                    OrbitCmd::Transport(Transport::Play)
                } else if w.eq_ignore_ascii_case(STOP_WORD.bytes()) {
                    OrbitCmd::Transport(Transport::Stop)
                } else {
                    return None;
                }
            }
            Family::Scene => return scene_cmd(words),
            Family::Chord => {
                let seg = one_based(words.next()?, MAX_SEGS)?;
                let d = one_based(words.next()?, Degree::ALL.len())?;
                OrbitCmd::Chord(Seg(seg), *Degree::ALL.get(usize::from(d))?)
            }
            Family::Scale => OrbitCmd::Scale(ScaleMask::new(number(words.next()?)?)?),
            Family::Step => {
                let r = ring(words.next()?)?;
                let i = one_based(words.next()?, STEPS)?;
                OrbitCmd::Step(r, Step(i), note(words.next()?)?)
            }
            Family::Pool => {
                let r = ring(words.next()?)?;
                let mut notes = [Note::MIN; MAX_POOL];
                let mut n = 0;
                for w in words.by_ref() {
                    *notes.get_mut(n)? = note(w)?;
                    n += 1;
                }
                return Some(OrbitCmd::Pool(r, Pool::new(notes.get(..n)?)?));
            }
        };
        words.next().is_none().then_some(cmd)
    }
}

/// `queue <n>`, `save <n> [over <rev>]`, `clear <n> <rev>`, `reload <n>
/// <rev>`, `move <a> <b> <rev a> <rev b>`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.scene_cmd"))]
fn scene_cmd(mut words: crate::console::Words<'_>) -> Option<OrbitCmd> {
    let sub = words.next()?;
    let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
    let cmd = if is(QUEUE_WORD.bytes()) {
        OrbitCmd::Queue(slot(words.next()?)?)
    } else if is(SAVE_WORD.bytes()) {
        let n = slot(words.next()?)?;
        match words.next() {
            None => return Some(OrbitCmd::Save(n)),
            Some(w) if w.eq_ignore_ascii_case(OVER_WORD.bytes()) => OrbitCmd::SaveOver(Seen {
                slot: n,
                rev: rev(words.next()?)?,
            }),
            Some(_) => return None,
        }
    } else if is(CLEAR_WORD.bytes()) {
        OrbitCmd::Clear(seen(words.next()?, words.next()?)?)
    } else if is(RELOAD_WORD.bytes()) {
        OrbitCmd::Reload(seen(words.next()?, words.next()?)?)
    } else if is(MOVE_WORD.bytes()) {
        let (a, b) = (slot(words.next()?)?, slot(words.next()?)?);
        let (ra, rb) = (rev(words.next()?)?, rev(words.next()?)?);
        if a == b {
            return None;
        }
        OrbitCmd::Move(Seen { slot: a, rev: ra }, Seen { slot: b, rev: rb })
    } else {
        return None;
    };
    words.next().is_none().then_some(cmd)
}

/// A decimal: `chimera_theory`'s parse, called directly (an `and_then`
/// instance's name would match the tuning group's glob, S0-S3's).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.number"))]
fn number(w: &[u8]) -> Option<u32> {
    chimera_theory::tuning::uint(w)
}

/// `1..=len` on the wire, as an index from 0.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.one_based"))]
fn one_based(w: &[u8], len: usize) -> Option<u8> {
    let n = number(w)?;
    let i = usize::try_from(n).ok()?.checked_sub(1)?;
    (i < len).then_some(i as u8)
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.slot"))]
fn slot(w: &[u8]) -> Option<SceneIdx> {
    SceneIdx::ALL
        .get(usize::from(one_based(w, SCENES)?))
        .copied()
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.ring"))]
fn ring(w: &[u8]) -> Option<RingIdx> {
    RingIdx::ALL.get(usize::from(one_based(w, RINGS)?)).copied()
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.rev"))]
fn rev(w: &[u8]) -> Option<SlotRev> {
    Some(SlotRev::new(number(w)?))
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.seen"))]
fn seen(n: &[u8], r: &[u8]) -> Option<Seen> {
    Some(Seen {
        slot: slot(n)?,
        rev: rev(r)?,
    })
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.note"))]
fn note(w: &[u8]) -> Option<Note> {
    Note::new(u8::try_from(number(w)?).ok()?)
}

/// Why the box wouldn't take an ORBIT command now, said at once (the UI
/// pass checks again, and a stale witness writes nothing there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotNow {
    /// The build has no ORBIT.
    Unbuilt,
    /// A switch waits to land: what lands stays what was queued.
    SwitchPending,
    /// The slot was written since the editor's `since` said its rev.
    SlotChanged,
    /// SAVE into a saved scene other than the playing one asks first.
    Saved,
    /// CLEAR and RELOAD are the playing scene's.
    NotPlaying,
    /// RELOAD with no drift or edit to drop: dimmed on the box.
    NothingToReload,
}

impl NotNow {
    /// `ERR …`, whole.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.not_now"))]
    pub fn write(self, w: &mut dyn Write) -> fmt::Result {
        w.write_str(match self {
            NotNow::Unbuilt => UNBUILT.get(),
            NotNow::SwitchPending => SWITCH_PENDING.get(),
            NotNow::SlotChanged => SLOT_CHANGED.get(),
            NotNow::Saved => SAVED_ASKS.get(),
            NotNow::NotPlaying => NOT_PLAYING.get(),
            NotNow::NothingToReload => NOTHING_TO_RELOAD.get(),
        })
    }
}

/// `s` still holds, proven by the engine's `Held`: its slot unwritten
/// since the editor read it.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.holds"))]
pub fn holds(song: &OrbitSong, s: Seen) -> Result<Held, NotNow> {
    match Held::check(song, s.slot, s.rev) {
        Some(held) => Ok(held),
        None => Err(NotNow::SlotChanged),
    }
}

/// `holds`, the proof dropped: the answer's check.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.unmoved"))]
fn unmoved(song: &OrbitSong, s: Seen) -> Result<(), NotNow> {
    match holds(song, s) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

/// What `c` would do on `ui` now, or why not: the box's own refusals,
/// in the order it checks them. Checked again in the UI pass.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.check"))]
pub fn check(c: OrbitCmd, ui: &UiState) -> Result<(), NotNow> {
    if !cfg!(feature = "orbit") {
        return Err(NotNow::Unbuilt);
    }
    let song = ui.project().orbit();
    let switching = |r: Result<(), NotNow>| match song.queued() {
        Some(_) => Err(NotNow::SwitchPending),
        None => r,
    };
    match c {
        OrbitCmd::Save(n) => switching(match n != song.current() && song.saved().contains(n) {
            true => Err(NotNow::Saved),
            false => Ok(()),
        }),
        OrbitCmd::SaveOver(s) => switching(unmoved(song, s)),
        OrbitCmd::Clear(s) => switching(playing(song, s)),
        OrbitCmd::Reload(s) => switching(playing(song, s).and_then(|()| match song.edited() {
            true => Ok(()),
            false => Err(NotNow::NothingToReload),
        })),
        OrbitCmd::Move(a, b) => switching(unmoved(song, a).and_then(|()| unmoved(song, b))),
        // B6 is never refused, whoever drives the transport (ADR 0080:
        // under SYNC EXT a press still flips it, the last word wins).
        OrbitCmd::Transport(_)
        | OrbitCmd::Queue(_)
        | OrbitCmd::Chord(..)
        | OrbitCmd::Scale(_)
        | OrbitCmd::Step(..)
        | OrbitCmd::Pool(..) => Ok(()),
    }
}

/// `s` is the playing slot, unwritten since.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.playing"))]
fn playing(song: &OrbitSong, s: Seen) -> Result<(), NotNow> {
    match s.slot == song.current() {
        true => unmoved(song, s),
        false => Err(NotNow::NotPlaying),
    }
}

/// `song <playing|stopped> clock <INT|EXT> scene <n> queued <n|-> edited
/// <yes|no>`: the transport and its clock as the runner last said, the
/// playing and queued slots, and whether `live` has moved from its slot.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.song_line"))]
pub(super) fn song_line(ui: &UiState, w: &mut dyn Write) -> fmt::Result {
    let st = &ui.orbit().status;
    let song = ui.project().orbit();
    w.write_str(SONG.get())?;
    w.write_str(match st.running {
        true => PLAYING.get(),
        false => STOPPED.get(),
    })?;
    w.write_str(CLOCK.get())?;
    w.write_str(match st.source {
        ClockSource::Internal => INT.get(),
        ClockSource::Midi => EXT.get(),
    })?;
    w.write_str(SCENE_AT.get())?;
    uint(w, u32::from(song.current().get()) + 1)?;
    w.write_str(QUEUED.get())?;
    match song.queued() {
        Some(q) => uint(w, u32::from(q.get()) + 1)?,
        None => w.write_char('-')?,
    }
    w.write_str(EDITED.get())?;
    w.write_str(match song.edited() {
        true => YES.get(),
        false => NO.get(),
    })?;
    w.write_char('\n')
}

/// Slot `n` as a `slot` line says it: saved or neutral, and its rev.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct SlotLine {
    saved: bool,
    rev: SlotRev,
}

impl SlotLine {
    pub(super) const NONE: SlotLine = SlotLine {
        saved: false,
        rev: SlotRev::new(0),
    };

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.slot_now"))]
    pub(super) fn now(song: &OrbitSong, n: SceneIdx) -> SlotLine {
        SlotLine {
            saved: song.saved().contains(n),
            rev: song.slot_rev(n),
        }
    }

    /// `slot <n> <saved|neutral> rev <r>`.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.orbit.slot_write")
    )]
    pub(super) fn write(self, n: SceneIdx, w: &mut dyn Write) -> fmt::Result {
        w.write_str(SLOT.get())?;
        uint(w, u32::from(n.get()) + 1)?;
        w.write_str(match self.saved {
            true => SAVED.get(),
            false => NEUTRAL.get(),
        })?;
        w.write_str(REV.get())?;
        uint(w, self.rev.get())?;
        w.write_char('\n')
    }
}

/// What `since` keeps of the scene's notes and chords: CHRD's degrees,
/// CUSTOM's set, each ring's steps and pool.
#[derive(Clone, Copy)]
pub(super) struct Notes {
    degrees: [Degree; MAX_SEGS],
    scale: Scale,
    steps: [[Note; STEPS]; RINGS],
    pools: [Pool; RINGS],
}

impl Notes {
    /// What `s` holds now.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.notes_of"))]
    pub(super) fn of(s: &Scene) -> Notes {
        let mut steps = [[Note::MIN; STEPS]; RINGS];
        let mut pools = [Pool::one(Note::MIN); RINGS];
        let mut r = 0;
        while r < RINGS {
            steps[r] = s.rings[r].ring.notes;
            pools[r] = s.rings[r].ring.pool;
            r += 1;
        }
        Notes {
            degrees: s.chords.degrees,
            scale: s.chords.custom,
            steps,
            pools,
        }
    }

    /// The `chord`, `scale`, `step` and `pool` lines of what differs from
    /// `was`, all of them with none.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.orbit.notes_write")
    )]
    pub(super) fn write(&self, was: Option<&Notes>, w: &mut dyn Write) -> fmt::Result {
        let mut i = 0;
        while i < MAX_SEGS {
            let d = self.degrees[i];
            if was.is_none_or(|o| o.degrees[i] != d) {
                w.write_str(CHORD_LINE.get())?;
                uint(w, i as u32 + 1)?;
                w.write_char(' ')?;
                uint(w, d.index() as u32 + 1)?;
                w.write_char('\n')?;
            }
            i += 1;
        }
        if was.is_none_or(|o| o.scale != self.scale) {
            w.write_str(SCALE_LINE.get())?;
            uint(w, u32::from(ScaleMask::of(self.scale)))?;
            w.write_char('\n')?;
        }
        let mut r = 0;
        while r < RINGS {
            let mut i = 0;
            while i < STEPS {
                let n = self.steps[r][i];
                if was.is_none_or(|o| o.steps[r][i] != n) {
                    w.write_str(STEP_LINE.get())?;
                    uint(w, r as u32 + 1)?;
                    w.write_char(' ')?;
                    uint(w, i as u32 + 1)?;
                    w.write_char(' ')?;
                    uint(w, u32::from(n.get()))?;
                    w.write_char('\n')?;
                }
                i += 1;
            }
            r += 1;
        }
        let mut r = 0;
        while r < RINGS {
            let p = &self.pools[r];
            if was.is_none_or(|o| !same_pool(&o.pools[r], p)) {
                w.write_str(POOL_LINE.get())?;
                uint(w, r as u32 + 1)?;
                let mut i = 0;
                while i < p.len() {
                    w.write_char(' ')?;
                    uint(w, u32::from(p.get(i).get()))?;
                    i += 1;
                }
                w.write_char('\n')?;
            }
            r += 1;
        }
        Ok(())
    }
}

/// Two pools of the same notes: a compare by hand (`Pool`'s `==` is a
/// slice compare, a generic bank 1 would hold).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.orbit.same_pool"))]
fn same_pool(a: &Pool, b: &Pool) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a.get(i) != b.get(i) {
            return false;
        }
        i += 1;
    }
    true
}

lit!(
    /// An ORBIT cell's line's head.
    ACT = "act"
);
lit!(RING = "RING");
lit!(DEGREE = "DEGREE");
lit!(CURSOR = "CURSOR");
lit!(SET = "SET");
lit!(NOTE = "NOTE");
lit!(LEARN = "LEARN");
lit!(CLEAR = "CLEAR");
lit!(PLAY = "PLAY");
lit!(SCENE = "SCENE");
lit!(SAVE = "SAVE");
lit!(SWITCH = "SWITCH");
lit!(ORBIT_WORD = "orbit");
lit!(SCENE_WORD = "scene");
lit!(CHORD_WORD = "chord");
lit!(SCALE_WORD = "scale");
lit!(STEP_WORD = "step");
lit!(POOL_WORD = "pool");
lit!(PLAY_WORD = "play");
lit!(STOP_WORD = "stop");
lit!(QUEUE_WORD = "queue");
lit!(SAVE_WORD = "save");
lit!(OVER_WORD = "over");
lit!(CLEAR_WORD = "clear");
lit!(RELOAD_WORD = "reload");
lit!(MOVE_WORD = "move");
lit!(ORBIT_USAGE = "ERR ed orbit takes play or stop\n");
lit!(
    SCENE_USAGE = "ERR ed scene takes queue <n>, save <n> [over <rev>], clear <n> <rev>, reload <n> <rev> or move <a> <b> <rev a> <rev b>\n"
);
lit!(CHORD_USAGE = "ERR ed chord takes <seg 1-8> <degree 1-12>\n");
lit!(SCALE_USAGE = "ERR ed scale takes <mask 1-4095, bit 0 set>\n");
lit!(STEP_USAGE = "ERR ed step takes <ring 1-5> <step 1-32> <note 0-127>\n");
lit!(POOL_USAGE = "ERR ed pool takes <ring 1-5> <note 0-127>, 1-8 of them\n");
lit!(UNBUILT = "ERR ed orbit is not in this build\n");
lit!(SWITCH_PENDING = "ERR ed scene: a switch is pending\n");
lit!(SLOT_CHANGED = "ERR ed scene: the slot changed since: ed since\n");
lit!(SAVED_ASKS = "ERR ed scene save: a saved scene: over <rev>\n");
lit!(NOT_PLAYING = "ERR ed scene: not the playing scene\n");
lit!(NOTHING_TO_RELOAD = "ERR ed scene reload: nothing to reload\n");
lit!(
    /// The song line's head.
    SONG = "song "
);
lit!(PLAYING = "playing");
lit!(STOPPED = "stopped");
lit!(CLOCK = " clock ");
lit!(INT = "INT");
lit!(EXT = "EXT");
lit!(SCENE_AT = " scene ");
lit!(QUEUED = " queued ");
lit!(EDITED = " edited ");
lit!(YES = "yes");
lit!(NO = "no");
lit!(SLOT = "slot ");
lit!(SAVED = " saved");
lit!(NEUTRAL = " neutral");
lit!(REV = " rev ");
lit!(CHORD_LINE = "chord ");
lit!(SCALE_LINE = "scale ");
lit!(STEP_LINE = "step ");
lit!(POOL_LINE = "pool ");
