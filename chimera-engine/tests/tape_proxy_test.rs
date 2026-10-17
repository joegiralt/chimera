//! TAPE's bank-1 proxy (ADR 0137): open, it forwards every call through the
//! door's `TapeDoor` and plays bit for bit as `TapeDeck`; closed, it is
//! `off`'s silent engine. The host builds its own door (`open_host`). One
//! test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use core::mem::MaybeUninit;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_engine::dsp::tape_engine::{
    Heads, PlayMode, TapeDeck, TapeEngine, TapeEngineParams, TapeModel,
};
use chimera_engine::hw::Cost;
use chimera_engine::modulation::ModState;
use chimera_engine::project::SourceId;
use chimera_engine::source_table::{AudioToken, SourceTable};
use chimera_engine::storage::twelve::pack12;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const RATE: u16 = 48_000;
const FRAMES: usize = 9_600;

/// A 48 kHz sine 100 frames long, as the arena holds it, alone in a table.
fn source() -> (AudioToken, SourceId) {
    let d = Descriptor {
        shape: Shape::Plain,
        count: 1,
        rate: RATE,
        head_frames: 0,
        heads: 0,
    };
    let s: Vec<i16> = (0..FRAMES)
        .map(|i| (libm::sinf(core::f32::consts::TAU * i as f32 / 100.0) * 2047.0) as i16)
        .collect();
    let mut held = d.to_bytes().to_vec();
    held.resize(DESC_LEN + 3 * s.len().div_ceil(2), 0);
    pack12(&s, &mut held[DESC_LEN..]);
    let table: &'static SourceTable = Box::leak(Box::new(SourceTable::new()));
    let bytes: &'static mut [u8] = Box::leak(held.into_boxed_slice());
    table.attach_for_test(bytes.as_mut_ptr());
    let id = SourceId::ALL[0];
    table.publish_for_test(id, 0, bytes.len() as u32, RATE);
    (table.reader().expect("its own table"), id)
}

fn bits(x: &[f32]) -> Vec<u32> {
    x.iter().map(|y| y.to_bits()).collect()
}

#[test]
fn the_proxy_plays_as_the_deck_open_and_nothing_closed() {
    let (mut tok, id) = source();
    let p = TapeEngineParams {
        model: TapeModel::Play,
        mode: PlayMode::Loop,
        ..TapeEngineParams::default()
    };
    let mods = ModState::new();

    // Closed: `off`'s engine.
    assert!(!DOOR_CELL.is_open());
    let mut slot = MaybeUninit::uninit();
    let e = TapeEngine::init_in_place(&mut slot);
    e.seed(3);
    e.latch(id);
    assert!(TapeEngine::refuses(p.model, Some(id), &tok.begin_block()));
    e.start(60, &p, &tok.begin_block(), SR, None);
    let mut out = [1.0; BLOCK_SIZE];
    e.play(&mut out, &p, &tok.begin_block(), SR, None);
    assert_eq!(out, [0.0; BLOCK_SIZE]);
    assert!(!e.is_active() && !e.sustains());
    assert_eq!(e.source(), None);
    assert_eq!(e.heads(), Heads::NONE);
    assert_eq!(e.playing_cost(), None);
    assert_eq!(TapeEngine::cost(&p, &mods), Cost(0));
    assert_eq!(TapeEngine::model_cost(p.model, p.mode), Cost(0));

    // Open: the deck's every sample.
    DOOR_CELL.open_host();
    let mut dslot = MaybeUninit::uninit();
    let d = TapeDeck::init_in_place(&mut dslot);
    d.seed(3);
    let mut pslot = MaybeUninit::uninit();
    let e = TapeEngine::init_in_place(&mut pslot);
    e.seed(3);
    for (k, note) in [(0, 60u8), (40, 67)] {
        let v = tok.begin_block();
        assert!(!TapeEngine::refuses(p.model, Some(id), &v));
        assert_eq!(
            TapeEngine::refuses(p.model, Some(id), &v),
            TapeDeck::refuses(p.model, Some(id), &v)
        );
        d.latch(id);
        e.latch(id);
        if k > 0 {
            d.glide_to(note, (SR, 0.01));
            e.glide_to(note, (SR, 0.01));
        } else {
            d.hold();
            e.hold();
            d.set_pitch(7.0);
            e.set_pitch(7.0);
            d.start(note, &p, &v, SR, None);
            e.start(note, &p, &v, SR, None);
        }
        for _ in 0..40 {
            let v = tok.begin_block();
            let (mut a, mut b) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
            d.play(&mut a, &p, &v, SR, None);
            e.play(&mut b, &p, &v, SR, None);
            assert_eq!(bits(&a), bits(&b));
            assert_eq!(d.heads(), e.heads());
            assert_eq!(d.playing_cost(), e.playing_cost());
        }
    }
    assert!(e.is_active() && d.is_active());
    assert_eq!(e.source(), Some(id));
    assert_eq!(e.sustains(), d.sustains());
    assert_eq!(TapeEngine::cost(&p, &mods), TapeDeck::cost(&p, &mods));
    assert_eq!(
        TapeEngine::model_cost(p.model, p.mode),
        TapeDeck::model_cost(p.model, p.mode)
    );
    // A strip moved to a rebuilt slot carries on as the deck's.
    let (ds, es) = (d.strip(), e.strip());
    let mut dslot2 = MaybeUninit::uninit();
    let d2 = TapeDeck::init_in_place(&mut dslot2);
    d2.restore(ds);
    let mut pslot2 = MaybeUninit::uninit();
    let e2 = TapeEngine::init_in_place(&mut pslot2);
    e2.restore(es);
    d2.latch(id);
    e2.latch(id);
    let v = tok.begin_block();
    d2.start(64, &p, &v, SR, None);
    e2.start(64, &p, &v, SR, None);
    d.note_off();
    e.note_off();
    for _ in 0..20 {
        let v = tok.begin_block();
        let (mut a, mut b) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        d2.play(&mut a, &p, &v, SR, None);
        e2.play(&mut b, &p, &v, SR, None);
        assert_eq!(bits(&a), bits(&b));
        assert!(a.iter().any(|&y| y != 0.0));
    }

    // Closed again mid-note: silent, and nothing reads the deck.
    DOOR_CELL.close();
    let mut out = [1.0; BLOCK_SIZE];
    e2.play(&mut out, &p, &tok.begin_block(), SR, None);
    assert_eq!(out, [0.0; BLOCK_SIZE]);
    assert!(!e2.is_active());
    assert_eq!(e2.heads(), Heads::NONE);
    assert!(TapeEngine::refuses(p.model, Some(id), &tok.begin_block()));

    // Open again: the deck plays on from where the close left it.
    DOOR_CELL.open_host();
    assert!(e2.is_active());
    for _ in 0..5 {
        let v = tok.begin_block();
        let (mut a, mut b) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        d2.play(&mut a, &p, &v, SR, None);
        e2.play(&mut b, &p, &v, SR, None);
        assert_eq!(bits(&a), bits(&b));
        assert!(a.iter().any(|&y| y != 0.0));
    }

    // A strip restored while closed: the deck's strip is dropped and the
    // voice's seed kept, so the deck built next is a fresh one on seed 3.
    // TAPE up, so a strip that ran on (`d4`) sounds different.
    let worn = TapeEngineParams { tape: 0.8, ..p };
    let s = e2.strip();
    let ran = d2.strip();
    DOOR_CELL.close();
    let mut pslot3 = MaybeUninit::uninit();
    let e3 = TapeEngine::init_in_place(&mut pslot3);
    e3.restore(s);
    DOOR_CELL.open_host();
    let mut dslot3 = MaybeUninit::uninit();
    let d3 = TapeDeck::init_in_place(&mut dslot3);
    d3.seed(3);
    let mut dslot4 = MaybeUninit::uninit();
    let d4 = TapeDeck::init_in_place(&mut dslot4);
    d4.restore(ran);
    let v = tok.begin_block();
    for k in [&mut *d3, &mut *d4] {
        k.latch(id);
        k.start(60, &worn, &v, SR, None);
    }
    e3.latch(id);
    e3.start(60, &worn, &v, SR, None);
    let mut differs = false;
    for _ in 0..40 {
        let v = tok.begin_block();
        let (mut a, mut b, mut c) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        d3.play(&mut a, &worn, &v, SR, None);
        e3.play(&mut b, &worn, &v, SR, None);
        d4.play(&mut c, &worn, &v, SR, None);
        assert_eq!(bits(&a), bits(&b));
        differs |= bits(&a) != bits(&c);
    }
    assert!(differs, "the strip's wear must be audible here");
    DOOR_CELL.close();
}
