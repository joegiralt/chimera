//! The prompt's answers and the yes they carry (ADR 0066): a SEQ tap on a
//! prompt's confirming pill is the only `Said`, the guard's witness
//! (`project::guard`). The UI draws the prompts
//! (`chimera_core::ui::settings::prompt`) and keeps the presses
//! (`chimera_core::ui::hold`).

use core::fmt::Debug;
use core::marker::PhantomData;

use chimera_hal::{Controls, EncoderId};

use crate::boot::RomDfu;
use crate::orbit::RINGS;
use crate::project::{
    DeleteTarget, OverwriteTarget, PartSource, ProjectSource, SceneSlot, Witnessed,
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

/// A prompt's options, as an enum.
pub trait Answers: Copy + Eq + Debug + 'static {
    /// In the pills' order.
    const ALL: Opts<Self>;
    fn label(self) -> &'static str;
}

macro_rules! answers {
    ($(#[$m:meta])* $name:ident { $($v:ident => $l:literal),+ $(,)? } $opts:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($v),+
        }

        impl Answers for $name {
            const ALL: Opts<Self> = Opts::$opts([$($name::$v),+]);
            fn label(self) -> &'static str {
                match self {
                    $($name::$v => $l),+
                }
            }
        }
    };
}

answers!(LoadAnswer { SaveThenLoad => "SAVE THEN LOAD", LoadAnyway => "LOAD ANYWAY", Cancel => "CANCEL" } Three);
answers!(ReplaceAnswer { SavePartFirst => "SAVE PART FIRST", Replace => "REPLACE", Cancel => "CANCEL" } Three);
answers!(ReloadAnswer { SavePartFirst => "SAVE PART FIRST", Reload => "RELOAD", Cancel => "CANCEL" } Three);
answers!(
    /// `Update`'s pill names the Parts: `UPDATE P4`, `UPDATE ALL`.
    AlsoUsesAnswer { Update => "UPDATE", Leave => "LEAVE" } Two
);
answers!(NameExistsAnswer { KeepBoth => "KEEP BOTH", Overwrite => "OVERWRITE THAT ONE" } Two);
answers!(DeleteAnswer { Delete => "DELETE", Cancel => "CANCEL" } Two);
answers!(ClearAnswer { Clear => "CLEAR", Cancel => "CANCEL" } Two);
answers!(SaveOverAnswer { SaveOver => "SAVE OVER", Cancel => "CANCEL" } Two);
answers!(SaveSceneOverAnswer { SaveOver => "SAVE OVER", Cancel => "CANCEL" } Two);
answers!(ClearSceneAnswer { Clear => "CLEAR", Cancel => "CANCEL" } Two);
answers!(ReloadSceneAnswer { Reload => "RELOAD", Cancel => "CANCEL" } Two);
answers!(CardChangedAnswer { SaveAs => "SAVE AS", Cancel => "CANCEL" } Two);
answers!(DfuAnswer { EnterDfu => "ENTER DFU", Cancel => "CANCEL" } Two);
answers!(PurgeAnswer { Purge => "PURGE", Cancel => "CANCEL" } Two);

/// A pick, or MENU (`None`). Only `Choice::input` makes one, so an answer
/// is always a key's:
///
/// ```compile_fail,E0423
/// use chimera_engine::confirm::{Answer, ReplaceAnswer};
/// let _ = Answer(Some(ReplaceAnswer::Replace));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer<A>(Option<A>);

impl<A: Copy> Answer<A> {
    /// The option SEQ confirmed; `None` for MENU.
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
    /// On the first option.
    pub fn new() -> Self {
        Choice {
            at: 0,
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

    /// MENU cancels; SEQ answers with the option shown before this frame's
    /// turn; A moves, clamped.
    pub fn input(&mut self, c: &impl Controls, p: &Presses) -> Option<Answer<A>> {
        if p.menu == Some(Press::Tap) {
            return Some(Answer(None));
        }
        if p.seq == Some(Press::Tap) {
            return Some(Answer(Some(self.picked())));
        }
        let last = A::ALL.as_slice().len() as i32 - 1;
        let to = self.at as i32 + c.encoder_delta(EncoderId::A) as i32;
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

impl Guarded for LoadAnswer {
    const SAVE_FIRST: Self = LoadAnswer::SaveThenLoad;
}

/// The yes in `a`, if SEQ took the confirming pill.
pub fn said<A: Commits>(a: Answer<A>) -> Option<Said<A::Confirms>> {
    (a.picked() == Some(A::CONFIRM)).then(Said::new)
}

/// A SEQ tap on a prompt's confirming pill, for a `T` target only.
pub struct Said<T>(PhantomData<T>);

impl<T> Said<T> {
    fn new() -> Self {
        Said(PhantomData)
    }
}
