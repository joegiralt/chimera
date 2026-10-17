//! A project's TAPE Part loads as TAPE (ADR 0141): kept whole, saved back
//! whole, and while bank 2 isn't open, silent and reading NEEDS UPDATE.

mod common;
mod screen;

use chimera_core::bank2::{Bank2Status, NEEDS_UPDATE, NEEDS_UPDATE_CELL, Version, closed_text};
use chimera_core::block::Block;
use chimera_core::dsp::tape_engine::params::TapeEngineParams;
use chimera_core::params::EngineType;
use chimera_core::project::{Origin, PartFrom, PartId, PartSource, Project};
use chimera_core::ui::UiState;
use chimera_core::ui::components::header_fits;
use common::codec_util::{decode_project_lossy, encode_project};
use common::project::load;
use screen::*;

fn open() -> Bank2Status {
    Bank2Status::Open {
        version: Version::new("0.2.0 02e440e2"),
        seal: [1; 8],
    }
}

/// Part 2 TAPE, AGE turned: it loads as saved, not lossy, and a save
/// writes the file back byte for byte.
#[test]
fn a_tape_part_loads_as_tape_and_saves_back_whole() {
    let part = PartId::ALL[1];
    let (mut p, t) = Project::boxed();
    let src = PartSource {
        part,
        from: PartFrom::Init(EngineType::Tape),
    };
    load(&mut p, t, src).unwrap();
    p.edit_part(part)
        .sound
        .params
        .tape
        .set(TapeEngineParams::AGE, 0.4);
    let file = encode_project(&p);
    let (mut q, _) = Project::boxed();
    let lossy = decode_project_lossy(&file, &mut q).unwrap();
    assert!(!lossy, "nothing of a TAPE Part is dropped");
    let x = q.part(part);
    assert_eq!(x.sound.engine(), EngineType::Tape);
    assert!(x.sound.bits_eq(&p.part(part).sound));
    assert_eq!(x.origin(), Origin::Init(EngineType::Tape));
    assert_eq!(encode_project(&q), file, "the save writes it back whole");
}

/// The header's warning on `engine`'s home page, Part 1, under `bank2`.
fn warn(engine: EngineType, bank2: Option<Bank2Status>) -> Option<&'static str> {
    let mut ui = UiState::new();
    if let Some(s) = bank2 {
        ui.set_bank2(s);
    }
    load_init(&mut ui, engine);
    let h = ui.page_header();
    assert!(header_fits(&h), "{:?} {:?}", h.name.as_str(), h.warn);
    h.warn
}

#[test]
fn a_tape_part_reads_needs_update_while_bank2_is_closed() {
    use Bank2Status::{Empty, Unsealed};
    use EngineType::{Tape, Va};
    assert_eq!(warn(Tape, Some(Empty)), Some(NEEDS_UPDATE_CELL));
    assert_eq!(
        warn(Tape, Some(Unsealed)),
        Some(NEEDS_UPDATE_CELL),
        "a bank-2 build that was never sealed"
    );
    assert_eq!(warn(Tape, Some(open())), None);
    assert_eq!(warn(Tape, None), None, "no bank 2: TAPE plays from bank 1");
    assert_eq!(warn(Va, Some(Empty)), None, "VA plays from bank 1");
}

/// DECK's MODEL reads NEEDS UPDATE in the focus band, its last word in the
/// cell; open, its model.
#[test]
fn deck_reads_needs_update_closed() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::tape_engine::params::TapeEngineParams;
    let model = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL);
    assert_eq!(
        closed_text(model, Some(Bank2Status::Empty)),
        Some(NEEDS_UPDATE)
    );
    assert_eq!(closed_text(model, Some(open())), None);
    assert_eq!(closed_text(model, None), None);
    let age = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::AGE);
    assert_eq!(closed_text(age, Some(Bank2Status::Empty)), None);
    let shot = |s: Option<Bank2Status>| {
        let mut ui = UiState::new();
        if let Some(s) = s {
            ui.set_bank2(s);
        }
        tape_deck(&mut ui, 0);
        settle(&mut ui);
        let h = ui.page_header();
        assert_eq!(h.name.as_str(), "DECK");
        (render_ui(&ui), h.warn)
    };
    let (none, empty, ok) = (
        shot(None),
        shot(Some(Bank2Status::Empty)),
        shot(Some(open())),
    );
    assert_eq!(
        none.0.hash(),
        ok.0.hash(),
        "open, DECK draws as it always did"
    );
    assert_eq!(empty.1, Some(NEEDS_UPDATE_CELL));
    assert!(first_cell_reads(&empty.0, NEEDS_UPDATE_CELL));
    assert!(first_cell_reads(&ok.0, "PLAY"));
}

// Streamed sources through pass 2 (slice 1 § 5.6).

mod streamed {
    use super::*;
    use chimera_core::dsp::tape_engine::params::TapeEngineParams;
    use chimera_core::dsp::tape_engine::read::Shape;
    use chimera_core::dsp::tape_engine::{PlayMode, TapeModel};
    use chimera_core::name::SoundName;
    use chimera_core::project::test_support::save_at;
    use chimera_core::project::{
        Absence, LoadLink, ProjectNote, ProjectSource, ReplaceGuard, SourceId, SourceRef,
        StreamLoad, StreamStep, load_project,
    };
    use chimera_core::storage::{
        Card, ProjectId, SourceHead, SourceLayout, WholeShape, source_crc, source_file,
        source_file_min_len, write_source,
    };
    use chimera_core::stream::layout::StreamFile;
    use chimera_core::stream::stage::heads_for;
    use chimera_hal::store::{Store, synthetic_runs};
    use chimera_hal::testkit::MemStore;
    use chimera_hal::units::Bytes;

    fn source(seed: u8, frames: u32) -> (SourceHead, Vec<u8>) {
        let data: Vec<u8> = (0..3 * frames.div_ceil(2) as usize)
            .map(|i| (i as u32).wrapping_mul(37).wrapping_add(seed as u32) as u8)
            .collect();
        let head = SourceHead {
            sref: SourceRef {
                crc: source_crc(WholeShape::Plain, 1, &data),
                frames,
                rate: 24_000,
            },
            shape: Shape::Plain,
            count: 1,
            root: 6_000,
            period_q16: 0,
            name: SoundName::new("BED").unwrap(),
        };
        (head, data)
    }

    fn put(s: &mut MemStore, src: &(SourceHead, Vec<u8>)) {
        let w = Card::new()
            .run(s, |s, r| write_source(s, r, &src.0, &src.1))
            .unwrap();
        w.result.unwrap();
    }

    fn one(t: &mut TapeEngineParams) {
        (t.model, t.mode) = (TapeModel::Play, PlayMode::One);
    }

    /// Part `n` on TAPE, PLAY ONE, playing `r`.
    fn tape_part(p: &mut Project, t: chimera_core::project::TemplateCrc, n: usize, r: SourceRef) {
        let part = PartId::ALL[n];
        let src = PartSource {
            part,
            from: PartFrom::Init(EngineType::Tape),
        };
        load(p, t, src).unwrap();
        let e = p.edit_part(part);
        one(&mut e.sound.params.tape);
        e.sound.source = Some(r.into());
    }

    /// `p` saved on `s`, loaded into a fresh project, its registry's rows
    /// 24..32 taken first when `full`.
    fn round_trip(
        s: &mut MemStore,
        p: &mut Project,
        full: bool,
    ) -> (Box<Project>, Option<ProjectNote>) {
        let mut card = Card::new();
        assert!(matches!(save_at(&mut card, s, p, 1), ProjectNote::Saved(_)));
        let (mut q, t) = Project::boxed();
        if full {
            let other = source(99, 150_000).0;
            let l = SourceLayout::whole(other).unwrap();
            let len = source_file_min_len(other.sref).unwrap();
            let runs = synthetic_runs(source_file(other.sref), Bytes::new(len)).unwrap();
            let f = StreamFile::from_layout(&l, runs).unwrap();
            for n in 24..32 {
                q.pool()
                    .streams()
                    .unwrap()
                    .insert(SourceId::new(n).unwrap(), f)
                    .unwrap();
            }
        }
        let vol = s.mount().unwrap();
        let id = ProjectId::new(1).unwrap();
        let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
        let out = load_project(&mut card, s, &mut q, go, &LoadLink::new());
        assert!(out.swap.is_some(), "{:?}", out.note);
        (q, out.note)
    }

    /// A Part streams a source too big for the arena; the project saved
    /// and loaded, pass 2 streams it the same: its heads, its stream row,
    /// the Part on it, nothing left out.
    #[test]
    fn a_streamed_part_loads_through_pass_two() {
        let mut s = MemStore::new(1);
        let bed = source(1, 150_000);
        put(&mut s, &bed);
        let r = bed.0.sref;
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, r);
        let heads = heads_for(r, &p.part(PartId::ALL[0]).sound.params.tape).unwrap();
        let mut card = Card::new();
        let got = card
            .run(&mut s, |s, rd| {
                let mut l = StreamLoad::start(&mut p, bed.0, heads).unwrap();
                loop {
                    match l.step(s, rd, &mut p, 4_096) {
                        StreamStep::More => {}
                        StreamStep::Done(id) => break Ok::<_, chimera_hal::store::StoreError>(id),
                        StreamStep::Failed(f) => panic!("{f:?}"),
                    }
                }
            })
            .unwrap()
            .result
            .unwrap();
        let held = p.pool().held(r).unwrap().to_vec();
        let file = *p.pool().streams().unwrap().get(got).unwrap();
        let (q, note) = round_trip(&mut s, &mut p, false);
        assert_eq!(note, None, "nothing left out");
        assert_eq!(q.pool().held(r), Some(&held[..]), "the same heads");
        assert!(q.pool().streamed(r));
        let id = q.pool().find_source(r).unwrap();
        let f = q.pool().streams().unwrap().get(id).expect("its stream row");
        assert_eq!((f.file(), f.frames()), (file.file(), file.frames()));
        assert_eq!(q.part(PartId::ALL[0]).sound.source_ref(), Some(r));
    }

    /// Review Focus 3: a project load that finds every stream row taken
    /// keeps the streamed source's ref, absent and saying why, and the
    /// project's other Parts load.
    #[test]
    fn a_ninth_stream_in_a_project_stays_absent() {
        let mut s = MemStore::new(1);
        let (bed, small) = (source(2, 150_000), source(3, 6_000));
        put(&mut s, &bed);
        put(&mut s, &small);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, bed.0.sref);
        tape_part(&mut p, t, 1, small.0.sref);
        p.source_absent(bed.0.sref).unwrap();
        p.source_absent(small.0.sref).unwrap();
        let (q, note) = round_trip(&mut s, &mut p, true);
        let id = q.pool().find_source(bed.0.sref).expect("the ref kept");
        assert_eq!(q.pool().absence(id), Some(Absence::StreamsFull));
        assert!(
            q.pool().source_view(small.0.sref).is_some(),
            "Part 2's loads"
        );
        assert_eq!(
            note,
            Some(ProjectNote::DidntFit {
                sounds: 0,
                sources: 1
            })
        );
    }

    /// The file `write_source` made for `src` on `s`, byte for byte.
    fn file_bytes(s: &mut MemStore, src: &(SourceHead, Vec<u8>)) -> Vec<u8> {
        use core::ops::ControlFlow;
        struct Sink<'a>(&'a mut Vec<u8>);
        impl chimera_hal::store::ReadSink for Sink<'_> {
            fn begin(&mut self, _: u32) -> ControlFlow<()> {
                ControlFlow::Continue(())
            }
            fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
                self.0.extend_from_slice(b);
                ControlFlow::Continue(())
            }
        }
        let mut out = Vec::new();
        let f = source_file(src.0.sref);
        Card::new()
            .run(s, |s, r| s.read(r.volume(), f, &mut Sink(&mut out)))
            .unwrap()
            .result
            .unwrap();
        out
    }

    /// A record's tag broken inside the head pass 2 streams: nothing is
    /// published, and the cell reads CARD READ FAILED, not SOURCE MISSING.
    #[test]
    fn a_broken_head_in_pass_two_reads_failed() {
        use chimera_core::ui::source_cell::READ_FAILED;
        use chimera_core::ui::view::SourceState;
        let mut s = MemStore::new(1);
        let bed = source(4, 150_000);
        put(&mut s, &bed);
        let mut file = file_bytes(&mut s, &bed);
        file[28 + 4 + 34 + 508] ^= 0xFF;
        s.plant(source_file(bed.0.sref), &file);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, bed.0.sref);
        p.source_absent(bed.0.sref).unwrap();
        let (q, _) = round_trip(&mut s, &mut p, false);
        let r = bed.0.sref;
        assert_eq!(q.pool().held(r), None, "nothing published");
        let id = q.pool().find_source(r).unwrap();
        assert_eq!(q.pool().absence(id), Some(Absence::ReadFailed));
        assert!(
            q.pool().streams().unwrap().get(id).is_none(),
            "no stream row"
        );
        let st = SourceState::of(&q.part(PartId::ALL[0]).sound.params.tape, Some(r), q.pool());
        let mut b = chimera_core::ui::fmt::FmtBuf::new();
        st.text(&mut b);
        assert_eq!(b.as_str(), READ_FAILED);
    }

    /// A Part that plays its too-big source a way that needs it whole:
    /// pass 2 keeps it absent and the cell says why.
    #[test]
    fn pass_two_says_what_the_part_needs_whole() {
        use chimera_core::dsp::tape_engine::NeedsRam;
        use chimera_core::ui::source_cell::PING_NEEDS_RAM;
        use chimera_core::ui::view::SourceState;
        let mut s = MemStore::new(1);
        let bed = source(5, 150_000);
        put(&mut s, &bed);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, bed.0.sref);
        p.edit_part(PartId::ALL[0]).sound.params.tape.mode = PlayMode::Ping;
        p.source_absent(bed.0.sref).unwrap();
        let (q, _) = round_trip(&mut s, &mut p, false);
        let id = q.pool().find_source(bed.0.sref).unwrap();
        assert_eq!(
            q.pool().absence(id),
            Some(Absence::NeedsRam(NeedsRam::Ping))
        );
        let st = SourceState::of(
            &q.part(PartId::ALL[0]).sound.params.tape,
            Some(bed.0.sref),
            q.pool(),
        );
        let mut b = chimera_core::ui::fmt::FmtBuf::new();
        st.text(&mut b);
        assert_eq!(b.as_str(), PING_NEEDS_RAM);
    }
}
