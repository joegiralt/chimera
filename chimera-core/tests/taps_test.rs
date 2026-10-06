//! ALGO's taps as stored routes and matrix rows (filter-types spec § 6, § 7):
//! the `TapRoutes` record, the OP5 and OP6 rows, X cells, and the jacks'
//! tap arms.

mod common;

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::DiskCode;
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::modulation::{FixedCol, MAX_MOD_SOURCES, ModSource};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::storage::{FileError, RecordTag};
use chimera_core::taps::{Jack, TapOp, TapRoutes, to_tap_amount};
use chimera_core::ui::filter_panel::{JackSlot, jack_amount, repatch, set_jack_amount};
use chimera_core::ui::mod_grid::{FIXED_COLS, MatrixState, RowSource, X_OP5, X_OP6};
use common::codec_util::{decode, encode, record_offsets, sound_file};

fn code(a: ParamAddr) -> [u8; 2] {
    [a.block.disk_code().unwrap(), a.param.0]
}

fn entry(jack: u8, a: ParamAddr, amount: i8) -> [u8; 4] {
    let [b, id] = code(a);
    [jack, b, id, amount as u8]
}

/// A matrix showing `s`, as the UI loads it.
fn matrix(s: &Sound) -> MatrixState {
    let mut m = MatrixState::new();
    m.load_sound(s);
    m
}

/// A Sound with a registry column, so the matrix has X cells to try.
fn algo_with_registry() -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    let a = ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::LEVEL);
    s.dest_registry.add(a, *b"O1 LEVEL").unwrap();
    s.mod_state.push(a);
    s
}

#[test]
fn tap_routes_round_trip_and_bad_entries_drop() {
    for op in TapOp::ALL {
        for c in FixedCol::ALL {
            for a in [127, -127, 0, 50] {
                let mut s = Sound::init(EngineType::Algo);
                s.params.taps.set(op, c, a);
                let d = decode(&encode(&s)).unwrap();
                assert_eq!(d.params.taps.get(op, c), Some(a), "{op:?} {c:?} {a}");
                assert_eq!(d.params.taps, s.params.taps);
                assert!(d.bits_eq(&s));
            }
        }
    }
    // Every entry at once.
    let mut s = Sound::init(EngineType::Algo);
    for (i, (op, c)) in TapOp::ALL
        .into_iter()
        .flat_map(|op| FixedCol::ALL.map(|c| (op, c)))
        .enumerate()
    {
        s.params.taps.set(op, c, (i as i16 * 30 - 100) as i8);
    }
    assert_eq!(decode(&encode(&s)).unwrap().params.taps, s.params.taps);

    let engine = (RecordTag::Engine, vec![EngineType::Algo.disk_code()]);
    let op5_level = ParamAddr::new(BlockRef::AlgoOp(Op::E), AlgoOpParams::LEVEL);
    let flt_level = ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL);
    let q = FixedCol::Q.addr();
    let entries = [
        entry(8, q, 30),
        entry(3, q, 11),
        entry(10, q, 12),
        entry(8, op5_level, 13),
        entry(9, flt_level, 14),
        entry(9, FixedCol::Fold.addr(), -40),
    ];
    let payload: Vec<u8> = entries.concat();
    let d = decode(&sound_file(&[
        engine.clone(),
        (RecordTag::TapRoutes, payload.clone()),
    ]))
    .unwrap();
    let mut want = TapRoutes::default();
    want.set(TapOp::Op5, FixedCol::Q, 30);
    want.set(TapOp::Op6, FixedCol::Fold, -40);
    assert_eq!(d.params.taps, want);
    // A 3-byte tail is not an entry.
    let mut tail = payload;
    tail.extend_from_slice(&entry(8, q, 1)[..3]);
    assert_eq!(
        decode(&sound_file(&[engine, (RecordTag::TapRoutes, tail)])).err(),
        Some(FileError::Bounds)
    );
    // An empty `TapRoutes` writes no record.
    let s = Sound::init(EngineType::Algo);
    assert_eq!(encode(&s), encode(&decode(&encode(&s)).unwrap()));
    let f = encode(&s);
    let tags: Vec<u16> = record_offsets(&f)
        .iter()
        .filter(|&&o| o + 4 <= f.len() - 4)
        .map(|&o| u16::from_le_bytes([f[o], f[o + 1]]))
        .collect();
    assert!(tags.contains(&RecordTag::Routes.code()));
    assert!(!tags.contains(&RecordTag::TapRoutes.code()));
}

#[test]
fn x_cells_refuse_every_edit() {
    let mut s = algo_with_registry();
    s.params.taps.set(TapOp::Op6, FixedCol::Vca, 20);
    let mut m = matrix(&s);
    assert!(m.num_cols() > FIXED_COLS);
    for col in FIXED_COLS..m.num_cols() {
        assert!(m.is_x(9, col) && m.is_x(8, col), "col {col}");
        let (taps, rev) = (m.taps, m.rev);
        let src = RowSource::Tap(TapOp::Op6).index();
        m.set(src, col, 40);
        (m.sel_row, m.sel_col) = (9, col);
        m.adjust_amount(10);
        m.adjust_amount(-10);
        m.delete_selected();
        assert_eq!((m.taps, m.rev), (taps, rev), "col {col}");
        assert_eq!(m.amount(src, col), 0);
        assert!(!m.is_present(src, col));
        assert_eq!(m.hint(), X_OP6);
        m.sel_row = 8;
        assert_eq!(m.hint(), X_OP5);
    }
    // The fixed columns are not X cells: they take an amount.
    for c in FixedCol::ALL {
        assert!(!m.is_x(9, c.index()));
    }
    (m.sel_row, m.sel_col) = (9, FixedCol::Freq.index());
    m.adjust_amount(10);
    assert_eq!(m.taps.get(TapOp::Op6, FixedCol::Freq), Some(10));
    let mut t = TapRoutes::default();
    m.sync_taps(&mut t);
    assert_eq!(t, m.taps);
}

#[test]
fn tap_rows_only_on_algo() {
    let m = matrix(&Sound::init(EngineType::Algo));
    assert_eq!(m.rows(), 10);
    assert_eq!(m.row_source(8), RowSource::Tap(TapOp::Op5));
    assert_eq!(m.row_source(9), RowSource::Tap(TapOp::Op6));
    assert_eq!(m.row_source(0), RowSource::Mod(ModSource::Env1.index()));
    let mut modal = Sound::init(EngineType::Modal);
    modal.params.taps.set(TapOp::Op5, FixedCol::Q, 50);
    let mut m = matrix(&modal);
    assert_eq!(m.rows(), 8);
    assert!(!m.is_x(8, FIXED_COLS));
    // A stored tap is inert off ALGO: no row reaches it.
    m.move_row(20);
    assert_eq!(m.sel_row, 7);
    m.set(RowSource::Tap(TapOp::Op5).index(), FixedCol::Q.index(), 9);
    assert_eq!(m.taps.get(TapOp::Op5, FixedCol::Q), Some(50));
}

#[test]
fn mod_tap_mod_keeps_the_depth() {
    for a in -50..=50 {
        let mut s = Sound::init(EngineType::Algo);
        assert_eq!(
            JackSlot::Fm3.effective(&s),
            Jack::Mod(ModSource::Lfo1),
            "FM3 IN starts on LFO 1"
        );
        set_jack_amount(&mut s, JackSlot::Fm3, a);
        assert!(repatch(&mut s, JackSlot::Fm3, Jack::Tap(TapOp::Op6)));
        assert_eq!(
            s.params.taps.get(TapOp::Op6, FixedCol::Freq),
            Some(to_tap_amount(a)),
            "{a}"
        );
        assert_eq!(
            s.mod_state.fixed_present(FixedCol::Freq) & 1 << ModSource::Lfo1.index(),
            0
        );
        assert_eq!(jack_amount(&s, JackSlot::Fm3), to_tap_amount(a));
        assert!(repatch(&mut s, JackSlot::Fm3, Jack::Mod(ModSource::Lfo1)));
        let back = jack_amount(&s, JackSlot::Fm3);
        assert!((back as i16 - a as i16).abs() <= 1, "{a} came back {back}");
        assert_eq!(s.params.taps.get(TapOp::Op6, FixedCol::Freq), None);
    }
    // Tap → Tap moves the entry; an occupied target refuses.
    let mut s = Sound::init(EngineType::Algo);
    assert!(repatch(&mut s, JackSlot::Qm, Jack::Tap(TapOp::Op5)));
    set_jack_amount(&mut s, JackSlot::Qm, 70);
    assert!(repatch(&mut s, JackSlot::Qm, Jack::Tap(TapOp::Op6)));
    assert_eq!(s.params.taps.get(TapOp::Op5, FixedCol::Q), None);
    assert_eq!(s.params.taps.get(TapOp::Op6, FixedCol::Q), Some(70));
    s.params.taps.set(TapOp::Op5, FixedCol::Q, 5);
    assert!(!repatch(&mut s, JackSlot::Qm, Jack::Tap(TapOp::Op5)));
    s.mod_state
        .set_fixed(ModSource::Lfo2.index(), FixedCol::Q, 1);
    assert!(!repatch(&mut s, JackSlot::Qm, Jack::Mod(ModSource::Lfo2)));
    assert_eq!(s.params.filter.qm_src(), Jack::Tap(TapOp::Op6));
    // Off ALGO no jack takes a tap.
    let mut s = Sound::init(EngineType::Modal);
    assert!(!repatch(&mut s, JackSlot::Fm3, Jack::Tap(TapOp::Op5)));
}

#[test]
fn no_mod_source_caller_sees_a_tap() {
    assert_eq!(ModSource::ALL.len(), MAX_MOD_SOURCES);
    assert_eq!(MAX_MOD_SOURCES, 8);
    let mut s = Sound::init(EngineType::Algo);
    for op in TapOp::ALL {
        for c in FixedCol::ALL {
            s.params.taps.set(op, c, 64);
        }
    }
    let d = decode(&encode(&s)).unwrap();
    assert!(d.mod_state.num_sources() <= MAX_MOD_SOURCES);
    assert!(d.mod_state.bits_eq(&s.mod_state));
    assert_eq!(d.params.taps.wanted(), [true, true]);
}

#[test]
fn qm_on_a_tap_views_the_tap_cell() {
    let mut s = Sound::init(EngineType::Algo);
    assert!(repatch(&mut s, JackSlot::Qm, Jack::Tap(TapOp::Op5)));
    set_jack_amount(&mut s, JackSlot::Qm, 30);
    assert_eq!(s.params.taps.get(TapOp::Op5, FixedCol::Q), Some(30));
    let m = matrix(&s);
    assert_eq!(
        m.amount(RowSource::Tap(TapOp::Op5).index(), FixedCol::Q.index()),
        30
    );
    assert_eq!(m.amount(m.row_source(8).index(), FixedCol::Q.index()), 30);
    assert!(m.is_present(m.row_source(8).index(), FixedCol::Q.index()));
}

#[test]
fn tap_routes_moving_and_wanted() {
    let mut t = TapRoutes::default();
    assert!(t.is_empty() && !t.moving(FixedCol::Freq) && t.wanted() == [false, false]);
    t.set(TapOp::Op6, FixedCol::Fold, 0);
    assert!(!t.is_empty() && !t.moving(FixedCol::Fold) && t.wanted() == [false, false]);
    t.set(TapOp::Op6, FixedCol::Fold, -3);
    assert!(t.moving(FixedCol::Fold) && !t.moving(FixedCol::Q));
    assert_eq!(t.wanted(), [false, true]);
    t.clear(TapOp::Op6, FixedCol::Fold);
    assert!(t.is_empty());
    // The depth conversion: 10 octaves a route, 4 a tap, held to ±127.
    assert_eq!(to_tap_amount(40), 100);
    assert_eq!(to_tap_amount(127), 127);
    assert_eq!(to_tap_amount(-127), -127);
    assert_eq!(to_tap_amount(1), 3);
    assert_eq!(chimera_core::taps::from_tap_amount(100), 40);
}
