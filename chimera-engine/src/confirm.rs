//! The prompt's answers and the yes they carry (ADR 0066, 0149): an EDIT
//! press on a prompt's confirming pill is the only `Said`, the guard's witness
//! (`project::guard`). The UI draws the prompts
//! (`chimera_core::ui::settings::prompt`) and keeps the presses
//! (`chimera_core::ui::hold`).

use core::fmt::Debug;
use core::marker::PhantomData;

use crate::boot::RomDfu;
use crate::orbit::RINGS;
use crate::project::{
    DeleteTarget, Held, LibraryPart, OverwriteTarget, PartSource, ProjectSource, SceneSlot,
    Witnessed,
};

/// A key's press: a tap acts on release, a hold fires once while down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Tap,
    Hold,
}

/// A frame's presses of the keys that tap and hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presses {
    pub menu: Option<Press>,
    pub seq: Option<Press>,
    /// EDIT's press edge, alone: not in a chord with MIX or a B key.
    pub edit: bool,
    /// B1–B5, indexed by `RingIdx`: ORBIT's ring keys. B6 never holds.
    pub ring: [Option<Press>; RINGS],
}

/// Two or three, in order: nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opts<T> {
    Two([T; 2]),
    Three([T; 3]),
}

impl<T: Copy> Opts<T> {
    pub fn as_slice(&self) -> &[T] {
        match self {
            Opts::Two(o) => o,
            Opts::Three(o) => o,
        }
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Opts<U> {
        match self {
            Opts::Two(o) => Opts::Two(o.map(f)),
            Opts::Three(o) => Opts::Three(o.map(f)),
        }
    }
}

/// A prompt's options, as an enum. Their pills' words are the UI's
/// (`chimera_core::ui::settings::prompt::Pill`).
pub trait Answers: Copy + Eq + Debug + 'static {
    /// In the pills' order.
    const ALL: Opts<Self>;
    /// The pill a prompt opens on: the safe one, so a reflex EDIT loses nothing.
    const START: u8 = 0;
}

macro_rules! answers {
    ($(#[$m:meta])* $name:ident { $($v:ident),+ $(,)? } $opts:ident $(, start $start:literal)?) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($v),+
        }

        impl Answers for $name {
            const ALL: Opts<Self> = Opts::$opts([$($name::$v),+]);
            $(const START: u8 = $start;)?
        }
    };
}

answers!(LoadAnswer { SaveThenLoad, LoadAnyway, Cancel } Three);
answers!(ReplaceAnswer { SavePartFirst, Replace, Cancel } Three);
answers!(ReloadAnswer { SavePartFirst, Reload, Cancel } Three);
answers!(LibraryAnswer { SavePartFirst, Replace, Cancel } Three);
answers!(
    /// `Update`'s pill names the Parts: `UPDATE P4`, `UPDATE ALL`.
    AlsoUsesAnswer { Update, Leave } Two
);
answers!(NameExistsAnswer { KeepBoth, Overwrite } Two);
answers!(DeleteAnswer { Delete, Cancel } Two, start 1);
answers!(ClearAnswer { Clear, Cancel } Two, start 1);
answers!(SaveOverAnswer { SaveOver, Cancel } Two, start 1);
answers!(SaveSceneOverAnswer { SaveOver, Cancel } Two, start 1);
answers!(ClearSceneAnswer { Clear, Cancel } Two, start 1);
answers!(ReloadSceneAnswer { Reload, Cancel } Two, start 1);
answers!(CardChangedAnswer { SaveAs, Cancel } Two);
answers!(DfuAnswer { EnterDfu, Cancel } Two, start 1);
answers!(PurgeAnswer { Purge, Cancel } Two, start 1);

/// A pick, or MENU or SEQ (`None`). Only `Choice::input` makes one, so an answer
/// is always a key's:
///
/// ```compile_fail,E0423
/// use chimera_engine::confirm::{Answer, ReplaceAnswer};
/// let _ = Answer(Some(ReplaceAnswer::Replace));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer<A>(Option<A>);

impl<A: Copy> Answer<A> {
    /// The option EDIT confirmed; `None` for MENU or SEQ.
    pub fn picked(self) -> Option<A> {
        self.0
    }
}

/// The highlighted option of an open prompt: always one of `A::ALL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice<A> {
    at: u8,
    _a: PhantomData<A>,
}

impl<A: Answers> Default for Choice<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Answers> Choice<A> {
    /// On the safe option, `A::START`.
    pub fn new() -> Self {
        Choice {
            at: A::START,
            _a: PhantomData,
        }
    }

    pub fn picked(&self) -> A {
        A::ALL.as_slice()[self.at as usize]
    }

    /// The highlighted option's place in `A::ALL`.
    pub fn at(&self) -> u8 {
        self.at
    }

    /// MENU and SEQ cancel; EDIT answers with the option shown before this
    /// frame's turn; `delta` (the picking encoder's turn) moves, clamped.
    pub fn input(&mut self, delta: i32, p: &Presses) -> Option<Answer<A>> {
        if p.menu == Some(Press::Tap) || p.seq == Some(Press::Tap) {
            return Some(Answer(None));
        }
        if p.edit {
            return Some(Answer(Some(self.picked())));
        }
        let last = A::ALL.as_slice().len() as i32 - 1;
        let to = self.at as i32 + delta;
        self.at = to.clamp(0, last) as u8;
        None
    }
}

/// A prompt with a pill that commits what it asked about, a `Confirms`.
pub trait Commits: Answers {
    type Confirms: Witnessed;
    const CONFIRM: Self;
}

/// A prompt that guards a replace: its confirming pill, and which saves first.
pub trait Guarded: Commits {
    const SAVE_FIRST: Self;
}

macro_rules! commits {
    ($($a:ident => $v:ident, $t:ty);+ $(;)?) => {
        $(impl Commits for $a {
            type Confirms = $t;
            const CONFIRM: Self = $a::$v;
        })+
    };
}

commits!(
    ReplaceAnswer => Replace, PartSource;
    ReloadAnswer => Reload, PartSource;
    LibraryAnswer => Replace, LibraryPart;
    LoadAnswer => LoadAnyway, ProjectSource;
    DeleteAnswer => Delete, DeleteTarget;
    ClearAnswer => Clear, OverwriteTarget;
    SaveOverAnswer => SaveOver, OverwriteTarget;
    NameExistsAnswer => Overwrite, OverwriteTarget;
    SaveSceneOverAnswer => SaveOver, SceneSlot;
    ClearSceneAnswer => Clear, SceneSlot;
    ReloadSceneAnswer => Reload, SceneSlot;
    DfuAnswer => EnterDfu, RomDfu;
);

impl Guarded for ReplaceAnswer {
    const SAVE_FIRST: Self = ReplaceAnswer::SavePartFirst;
}

impl Guarded for ReloadAnswer {
    const SAVE_FIRST: Self = ReloadAnswer::SavePartFirst;
}

impl Guarded for LibraryAnswer {
    const SAVE_FIRST: Self = LibraryAnswer::SavePartFirst;
}

impl Guarded for LoadAnswer {
    const SAVE_FIRST: Self = LoadAnswer::SaveThenLoad;
}

/// The yes in `a`, if EDIT took the confirming pill.
pub fn said<A: Commits>(a: Answer<A>) -> Option<Said<A::Confirms>> {
    (a.picked() == Some(A::CONFIRM)).then(Said::new)
}

/// An EDIT press on a prompt's confirming pill, for a `T` target only.
pub struct Said<T>(PhantomData<T>);

impl<T> Said<T> {
    fn new() -> Self {
        Said(PhantomData)
    }
}

/// Proof that the browser editor's own confirm said to drop a project's
/// unsaved edits (ADR 0289): the way to a `Said<ProjectSource>` that is
/// no key's. The engine can't name the token, which is the `ed` command
/// path's (`chimera_core::console::ed::project::Discard`, made only by
/// parsing a load's `discard` word, its field private to its module), so
/// the gate is this trait, and a test holds every naming of it in the
/// workspace to that one implementor
/// (`only_the_ed_path_proves_an_editors_yes`). A scene's yes needs no
/// trait: it takes the engine's own `Held`. Without a proof there is no
/// yes:
///
/// ```compile_fail,E0061
/// use chimera_engine::confirm::Said;
/// use chimera_engine::project::SceneSlot;
/// let _ = Said::<SceneSlot>::editor_confirm();
/// ```
///
/// ```compile_fail,E0061
/// use chimera_engine::confirm::Said;
/// use chimera_engine::project::ProjectSource;
/// let _ = Said::<ProjectSource>::editor_discard();
/// ```
pub trait EditorProof<T: Witnessed> {}

impl Said<SceneSlot> {
    /// The browser editor's yes to a scene's SAVE OVER, CLEAR, RELOAD or
    /// MOVE (ADR 0289): its own confirm dialog's, proven by the slot's
    /// rev as `since` said it, read off the slot by `Held::check`
    /// (`SceneAsk::seen` carries the same rev). Only a `Held` is a proof.
    #[inline(always)]
    pub fn editor_confirm(_: &Held) -> Self {
        Said::new()
    }
}

impl Said<ProjectSource> {
    /// The browser editor's `discard` (ADR 0289): its own confirm dialog's
    /// yes to a load over unsaved edits, as LOAD ANYWAY's pill is the
    /// box's. Only `ed project load … discard` holds a proof.
    #[inline(always)]
    pub fn editor_discard(_: &impl EditorProof<ProjectSource>) -> Self {
        Said::new()
    }
}
