//! The per-Part MIDI map's types (spec §§ 2-3): the whitelist, learn, the
//! place law and the reserved CCs.

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::lfo::LfoParams;
use chimera_core::dsp::modal::ModalParams;
use chimera_core::dsp::modulator::LfoSlot;
use chimera_core::dsp::va::VaParams;
use chimera_core::midi_in::Cc;
use chimera_core::midi_map::{MAP_LEN, MapEntry, MapFull, Mappable, MidiMap};
use chimera_core::orbit::RingIdx;
use chimera_core::params::{DriveParams, FilterParams, FolderParams, OutParams};
use chimera_core::part::PartParams;
use chimera_core::project::PartId;

fn cc(n: u8) -> Cc {
    Cc::new(n).unwrap()
}

fn mappable(b: BlockRef, p: chimera_core::block::ParamId) -> Mappable {
    Mappable::new(ParamAddr::new(b, p)).expect("mappable")
}

#[test]
fn mappable_whitelist() {
    for b in BlockRef::ALL {
        for s in b.specs() {
            let want = b.in_sound() && !s.patching && s.stored;
            let part_mix =
                b == BlockRef::Part && matches!(s.id, PartParams::LEVEL | PartParams::PAN);
            assert_eq!(
                Mappable::new(ParamAddr::new(b, s.id)).is_some(),
                want || part_mix,
                "{b:?} {}",
                s.label
            );
        }
    }
    let no = |b, p| assert!(Mappable::new(ParamAddr::new(b, p)).is_none(), "{b:?} {p:?}");
    for p in [PartParams::CHANNEL, PartParams::MODE, PartParams::BEND] {
        no(BlockRef::Part, p);
    }
    no(BlockRef::PartMix(PartId::ALL[0]), PartParams::LEVEL);
    no(
        BlockRef::Reverb,
        chimera_core::dsp::reverb::ReverbParams::SIZE,
    );
    no(
        BlockRef::OrbitRing(RingIdx::ALL[0]),
        chimera_core::block::ParamId(0),
    );
    no(BlockRef::Tempo, chimera_core::block::ParamId(0));
    no(BlockRef::Filter, FilterParams::FM3_IN);
    no(BlockRef::Theme, chimera_core::block::ParamId(0));

    // Section 9's default targets are all Mappable.
    let lfo1 = BlockRef::Lfo(LfoSlot::Lfo1);
    for (b, p) in [
        (BlockRef::Filter, FilterParams::CUTOFF),
        (BlockRef::Filter, FilterParams::RESONANCE),
        (BlockRef::Part, PartParams::LEVEL),
        (BlockRef::Part, PartParams::PAN),
        (BlockRef::Out, OutParams::SEND1),
        (BlockRef::Out, OutParams::SEND3),
        (lfo1, LfoParams::DEPTH),
        (BlockRef::Algo, AlgoParams::MORPH),
        (BlockRef::Algo, AlgoParams::VOWEL),
        (BlockRef::Drive, DriveParams::DRIVE),
        (BlockRef::Folder, FolderParams::FOLD),
        (BlockRef::Modal, ModalParams::STRUCTURE),
        (BlockRef::Modal, ModalParams::BRIGHT),
        (BlockRef::Modal, ModalParams::DAMP),
        (BlockRef::Modal, ModalParams::POS),
        (BlockRef::Va, VaParams::AMOUNT),
        (BlockRef::Va, VaParams::SHAPE1),
        (BlockRef::Va, VaParams::SHAPE2),
        (BlockRef::Va, VaParams::SUB),
    ] {
        let a = ParamAddr::new(b, p);
        assert_eq!(Mappable::new(a).map(Mappable::addr), Some(a), "{b:?} {p:?}");
    }
}

#[test]
fn learn_replaces_and_fills() {
    let cutoff = mappable(BlockRef::Filter, FilterParams::CUTOFF);
    let reso = mappable(BlockRef::Filter, FilterParams::RESONANCE);
    let mut m = MidiMap::EMPTY;
    m.learn(cc(74), cutoff).unwrap();
    m.learn(cc(20), cutoff).unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m.entries()[0].cc, cc(20));
    m.learn(cc(20), reso).unwrap();
    assert_eq!(m.len(), 2);
    assert_eq!(m.targets_of(cc(20)).count(), 2);
    assert_eq!(m.targets_of(cc(74)).count(), 0);

    // Fill to 16 distinct targets, then a 17th is refused.
    let mut m = MidiMap::EMPTY;
    let mut targets = BlockRef::ALL
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter_map(Mappable::new);
    for _ in 0..MAP_LEN {
        m.learn(cc(1), targets.next().unwrap()).unwrap();
    }
    assert_eq!(m.len(), MAP_LEN);
    assert_eq!(m.learn(cc(1), targets.next().unwrap()), Err(MapFull));
    // A full map still re-learns a target it holds.
    let held = m.entries()[3].target;
    assert_eq!(m.learn(cc(2), held), Ok(()));
    assert_eq!(m.entries()[3].cc, cc(2));
}

#[test]
fn remove_and_set() {
    let t = mappable(BlockRef::Filter, FilterParams::CUTOFF);
    let u = mappable(BlockRef::Filter, FilterParams::RESONANCE);
    let mut m = MidiMap::EMPTY;
    m.learn(cc(74), t).unwrap();
    m.learn(cc(71), u).unwrap();
    m.remove(0);
    assert_eq!((m.len(), m.entries()[0].target), (1, u));
    m.set(
        0,
        MapEntry {
            cc: cc(9),
            target: u,
            min: 5,
            max: 6,
        },
    );
    assert_eq!((m.entries()[0].cc, m.entries()[0].min), (cc(9), 5));
    m.remove(7);
    assert_eq!(m.len(), 1);
}

#[test]
fn place_inverts() {
    let t = mappable(BlockRef::Filter, FilterParams::CUTOFF);
    let e = |min, max| MapEntry {
        cc: cc(1),
        target: t,
        min,
        max,
    };
    assert_eq!(e(255, 0).place(0), 1.0);
    assert_eq!(e(0, 255).place(127), 1.0);
    assert_eq!(e(64, 128).place(0), 64.0 / 255.0);
}

#[test]
fn cc_reserved() {
    let reserved: Vec<u8> = (0..=127).filter(|&n| cc(n).is_reserved()).collect();
    let mut want = vec![0, 6, 32, 38, 64];
    want.extend(96..=101);
    want.extend(120..=127);
    assert_eq!(reserved, want);
}
