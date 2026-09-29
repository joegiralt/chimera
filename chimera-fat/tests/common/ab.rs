//! An A/B pair of Sounds, `/CHIMERA/SND.A` and `.B`, saved and loaded
//! through `chimera_core` on a `CutDisk`: each call on a fresh store, as
//! after a power cycle.
// Each test binary uses its own part of this module.
#![allow(dead_code)]

use crate::image::{Cut, CutDisk, RamDisk};
use crate::probe::{Probed, log, probed};
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::name::Name;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    AbFile, Card, CardFault, Generation, LoadError, Ready, RecordWriter, SaveError, Side,
    SoundCheck, SoundDecoder, check_file, encode_sound, load_ab, load_file, save_ab, write_target,
};
use chimera_hal::store::{Dir, Store, StoreError};
use core::cell::RefCell;
use std::rc::Rc;

pub type Slot = Rc<CutDisk>;

pub fn slot(disk: RamDisk) -> Slot {
    Rc::new(CutDisk::new(disk, Cut::Never))
}

pub fn copy(d: &RamDisk) -> RamDisk {
    RamDisk(RefCell::new(d.0.borrow().clone()))
}

pub fn file() -> AbFile {
    AbFile::new(Dir::Chimera, b"SND").unwrap()
}

pub fn name83(side: Side) -> [u8; 11] {
    match side {
        Side::A => *b"SND     A  ",
        Side::B => *b"SND     B  ",
    }
}

/// Save attempt `n`'s Sound, told apart by its name.
pub fn sound(n: u32) -> Sound {
    let mut s = factory_sound(n as usize % FACTORY_LEN).unwrap();
    s.name = Name::new(&format!("SAVE {n}")).unwrap();
    s
}

pub fn op<R, E: CardFault + From<StoreError>>(
    s: &mut Probed<CutDisk>,
    f: impl FnOnce(&mut Probed<CutDisk>, &Ready) -> Result<R, E>,
) -> Result<R, E> {
    Card::new().run(s, f).and_then(|o| o.result)
}

pub fn make_dir(slot: &Slot) {
    op(&mut probed(slot), |s, r| {
        s.make_dir(r.volume(), Dir::Chimera)
    })
    .unwrap();
}

/// Save `n`, `extra` records after the Sound's.
pub fn save_on(
    s: &mut Probed<CutDisk>,
    n: u32,
    extra: &dyn Fn(&mut RecordWriter<'_>) -> Result<(), StoreError>,
) -> Result<Generation, SaveError> {
    let snd = sound(n);
    op(s, |s, r| {
        save_ab(
            s,
            r,
            file(),
            &mut SoundCheck::new(),
            Some(snd.name),
            &mut |w| {
                encode_sound(&snd, w)?;
                extra(w)
            },
        )
    })
}

/// Save `n`, and the block its failed write was aimed at, if one failed.
pub fn save(slot: &Slot, n: u32) -> (Result<Generation, SaveError>, Option<u32>) {
    let mut s = probed(slot);
    let r = save_on(&mut s, n, &|_| Ok(()));
    let failed = r
        .is_err()
        .then(|| log(&s).writes.borrow().last().map(|&(b, _)| b))
        .flatten();
    (r, failed)
}

pub fn load(slot: &Slot) -> Result<Sound, LoadError> {
    let mut t = Sound::neutral(EngineType::Algo);
    op(&mut probed(slot), |s, r| {
        load_ab(s, r, file(), &mut SoundDecoder::new(&mut t))
    })?;
    Ok(t)
}

/// The side the next save writes.
pub fn target(slot: &Slot) -> Side {
    let f = file();
    let mut c = SoundCheck::new();
    op(&mut probed(slot), |s, r| {
        let a = check_file(s, r, f.side(Side::A), &mut c)?;
        let b = check_file(s, r, f.side(Side::B), &mut c)?;
        Ok::<_, StoreError>(write_target(a, b).unwrap().0)
    })
    .unwrap()
}

/// One side alone, through `load_file`: its generation and its Sound.
pub fn load_side(slot: &Slot, side: Side) -> Result<(Generation, Sound), LoadError> {
    let mut t = Sound::neutral(EngineType::Algo);
    let h = op(&mut probed(slot), |s, r| {
        load_file(s, r, file().side(side), &mut SoundDecoder::new(&mut t))
    })?;
    Ok((h.generation, t))
}

/// The side a cut save didn't target still loads alone, as it was.
pub fn assert_kept(what: &str, slot: &Slot, side: Side, generation: Generation, snd: &Sound) {
    let (g, got) = load_side(slot, side)
        .unwrap_or_else(|e| panic!("{what}: the kept side {side:?} alone, {e:?}"));
    assert_eq!(g, generation, "{what}: the kept side's generation");
    assert!(got.bits_eq(snd), "{what}: the kept side's Sound");
}
