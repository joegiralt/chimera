//! A project's TAPE Part loads as TAPE (ADR 0141): kept whole, saved back
//! whole, and while bank 2 isn't open, silent and reading NEEDS UPDATE.

mod common;
mod screen;

use chimera_core::bank2::{Bank2Status, NEEDS_UPDATE, NEEDS_UPDATE_CELL, Version, closed_text};
use chimera_core::block::Block;
use chimera_core::dsp::tape_engine::Way;
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

/// The door as `streamed`'s tests leave it, opened before anything is
/// built: they run in parallel in this process, and a NEW built closed
/// reads its Parts Edited once the door opens (maps decode only open).
fn door_as_streamed() {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
}

/// Part 2 TAPE, AGE turned: it loads as saved, not lossy, and a save
/// writes the file back byte for byte.
#[test]
fn a_tape_part_loads_as_tape_and_saves_back_whole() {
    door_as_streamed();
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
    door_as_streamed();
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
    door_as_streamed();
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::tape_engine::params::TapeEngineParams;
    let model = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL);
    assert_eq!(
        closed_text(model, 0.0, Some(Bank2Status::Empty)),
        Some(NEEDS_UPDATE)
    );
    assert_eq!(closed_text(model, 0.0, Some(open())), None);
    assert_eq!(closed_text(model, 0.0, None), None);
    let age = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::AGE);
    assert_eq!(closed_text(age, 0.0, Some(Bank2Status::Empty)), None);
    let shot = |s: Option<Bank2Status>| {
        let mut ui = UiState::new();
        if let Some(s) = s {
            ui.set_bank2(s);
        }
        tape_deck(&mut ui, chimera_core::dsp::tape_engine::Way::Loop);
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
    assert!(first_cell_reads(&ok.0, "WALK"));
}

// Streamed sources through pass 2 (slice 1 § 5.6).

mod streamed {
    use super::*;
    use chimera_core::dsp::tape_engine::params::TapeEngineParams;
    use chimera_core::dsp::tape_engine::read::Shape;

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
    use chimera_hal::store::{Seal, Store, synthetic_runs};
    use chimera_hal::testkit::MemStore;
    use chimera_hal::units::Bytes;

    fn source(seed: u8, frames: u32) -> (SourceHead, Vec<u8>) {
        // Under the firmware's features streamed loads go through bank 2's door.
        chimera_core::bank2::DOOR_CELL.open_host_if_linked();
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
        t.set_way(Way::One);
    }

    /// Part `n` on TAPE, WALK ONE, playing `r`.
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
            let runs =
                synthetic_runs(source_file(other.sref), Bytes::new(len), Seal::Sealed).unwrap();
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

    /// The unit proof's shape: four Parts on four beds the arena can't
    /// hold, each ref in the pool as a library load puts it, so the save
    /// writes its `PoolSource`. Both passes stream all four; none reads
    /// SOURCE MISSING. `just test` runs it under the firmware's features.
    #[test]
    fn four_streamed_parts_load_through_pass_two() {
        use chimera_core::ui::view::SourceState;
        let mut s = MemStore::new(1);
        let (mut p, t) = Project::boxed();
        let beds: Vec<_> = (0..4).map(|k| source(10 + k, 150_000)).collect();
        for (k, bed) in beds.iter().enumerate() {
            put(&mut s, bed);
            tape_part(&mut p, t, k, bed.0.sref);
            p.source_absent(bed.0.sref).unwrap();
        }
        let (q, note) = round_trip(&mut s, &mut p, false);
        assert_eq!(note, None, "nothing left out");
        for (k, bed) in beds.iter().enumerate() {
            let r = bed.0.sref;
            assert!(q.pool().streamed(r), "Part {} streams", k + 1);
            let st = SourceState::of(&q.part(PartId::ALL[k]).sound.params.tape, Some(r), q.pool());
            assert!(
                matches!(st, SourceState::Held { .. }),
                "Part {}: {st:?}",
                k + 1
            );
        }
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

    /// #765: a source pass 1 left out for room loads whole in pass 2 once
    /// another's read misses and leaves it the room: never left absent.
    #[test]
    fn a_source_that_now_fits_loads_whole() {
        let mut s = MemStore::new(1);
        let (a, b) = (source(40, 60_000), source(41, 60_000));
        put(&mut s, &a);
        put(&mut s, &b);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, a.0.sref);
        tape_part(&mut p, t, 1, b.0.sref);
        p.source_absent(a.0.sref).unwrap();
        p.source_absent(b.0.sref).unwrap();
        // A's file damaged after the save: pass 2's whole read misses it.
        let mut card = Card::new();
        assert!(matches!(
            save_at(&mut card, &mut s, &mut p, 1),
            ProjectNote::Saved(_)
        ));
        let mut file = file_bytes(&mut s, &a);
        let n = file.len();
        file[n - 64] ^= 0x01;
        s.plant(source_file(a.0.sref), &file);
        let (mut q, t) = Project::boxed();
        let vol = s.mount().unwrap();
        let id = ProjectId::new(1).unwrap();
        let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
        let out = load_project(&mut card, &mut s, &mut q, go, &LoadLink::new());
        assert!(out.swap.is_some(), "{:?}", out.note);
        assert_eq!(q.pool().held(a.0.sref), None, "A's read missed");
        assert!(q.pool().held(b.0.sref).is_some(), "B loads whole");
        assert!(!q.pool().streamed(b.0.sref), "whole, not streamed");
        assert!(
            !matches!(out.note, Some(ProjectNote::DidntFit { .. })),
            "nothing left out for room: {:?}",
            out.note
        );
    }

    /// #765: heads with no room in the gap leave their source absent as
    /// didn't fit (POOL FULL), not SOURCE MISSING.
    #[test]
    fn heads_with_no_room_read_pool_full() {
        use chimera_core::ui::settings::prompt::POOL_FULL;
        use chimera_core::ui::view::SourceState;
        let mut s = MemStore::new(1);
        let (a, b) = (source(42, 102_800), source(43, 150_000));
        put(&mut s, &a);
        put(&mut s, &b);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, a.0.sref);
        tape_part(&mut p, t, 1, b.0.sref);
        p.source_absent(a.0.sref).unwrap();
        p.source_absent(b.0.sref).unwrap();
        let (q, note) = round_trip(&mut s, &mut p, false);
        assert!(q.pool().held(a.0.sref).is_some(), "A held");
        assert!(!q.pool().streamed(a.0.sref), "A whole");
        let id = q.pool().find_source(b.0.sref).expect("the ref kept");
        assert_eq!(q.pool().absence(id), Some(Absence::NoRoom));
        let st = SourceState::of(
            &q.part(PartId::ALL[1]).sound.params.tape,
            Some(b.0.sref),
            q.pool(),
        );
        let mut w = chimera_core::ui::fmt::FmtBuf::new();
        st.text(&mut w);
        assert_eq!(w.as_str(), POOL_FULL);
        assert_eq!(
            note,
            Some(ProjectNote::DidntFit {
                sounds: 0,
                sources: 1
            })
        );
    }

    /// `file`'s seal on `s`.
    fn seal_of(s: &mut MemStore, file: chimera_hal::store::FileName) -> Seal {
        let vol = s.mount().unwrap();
        s.runs(vol, file).unwrap().seal
    }

    /// An unsealed file (copied onto the card by hand, or a cut `put`'s
    /// remains) whose samples hash to its name streams as a sealed one,
    /// checked whole first, and is sealed after.
    #[test]
    fn an_unsealed_file_that_checks_out_streams_and_is_sealed() {
        let mut s = MemStore::new(1);
        let bed = source(7, 150_000);
        put(&mut s, &bed);
        let f = source_file(bed.0.sref);
        s.unseal(f);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, bed.0.sref);
        p.source_absent(bed.0.sref).unwrap();
        let (q, _) = round_trip(&mut s, &mut p, false);
        let r = bed.0.sref;
        let id = q.pool().find_source(r).expect("the ref kept");
        assert_eq!(q.pool().absence(id), None);
        assert!(q.pool().streams().unwrap().get(id).is_some(), "streamed");
        assert_eq!(seal_of(&mut s, f), Seal::Sealed);
    }

    /// ADR 0210: an unsealed source too big for one pass is checked a
    /// pass's budget at a time, unsealed until the last share passes,
    /// then sealed and streamed.
    #[test]
    fn a_large_unsealed_source_is_checked_a_share_a_pass() {
        let mut s = MemStore::new(1);
        let bed = source(9, 150_000);
        put(&mut s, &bed);
        let f = source_file(bed.0.sref);
        let len = file_bytes(&mut s, &bed).len();
        s.unseal(f);
        let r = bed.0.sref;
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, r);
        let heads = heads_for(r, &p.part(PartId::ALL[0]).sound.params.tape).unwrap();
        let (open, passes) = Card::new()
            .run(&mut s, |s, rd| {
                let mut l = StreamLoad::start(&mut p, bed.0, heads).unwrap();
                let (mut open, mut passes) = (0, 0);
                loop {
                    passes += 1;
                    match l.step(s, rd, &mut p, 4_096) {
                        StreamStep::More => {}
                        StreamStep::Done(_) => {
                            break Ok::<_, chimera_hal::store::StoreError>((open, passes));
                        }
                        StreamStep::Failed(e) => panic!("{e:?}"),
                    }
                    if s.runs(rd.volume(), f).unwrap().seal == Seal::Open {
                        open += 1;
                    }
                }
            })
            .unwrap()
            .result
            .unwrap();
        assert!(
            open >= len / 4_096,
            "{open} passes unsealed, of a {len} B file"
        );
        assert!(passes > open, "the heads after the check");
        assert_eq!(seal_of(&mut s, f), Seal::Sealed);
        assert!(p.pool().streamed(r));
    }

    /// m10 (#761): an unsealed file whose samples don't hash to its name
    /// doesn't stream, though its heads read clean: its ref stays,
    /// absent, with no stream row, and it stays unsealed.
    #[test]
    fn an_unsealed_damaged_file_doesnt_stream() {
        let mut s = MemStore::new(1);
        let bed = source(8, 150_000);
        put(&mut s, &bed);
        let f = source_file(bed.0.sref);
        let mut file = file_bytes(&mut s, &bed);
        let n = file.len();
        file[n - 64] ^= 0x01;
        s.plant(f, &file);
        s.unseal(f);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, bed.0.sref);
        p.source_absent(bed.0.sref).unwrap();
        let (q, _) = round_trip(&mut s, &mut p, false);
        let r = bed.0.sref;
        assert_eq!(q.pool().held(r), None, "nothing published");
        let id = q.pool().find_source(r).expect("the ref kept");
        assert_eq!(q.pool().absence(id), Some(Absence::ReadFailed));
        assert!(q.pool().streams().unwrap().get(id).is_none());
        assert_eq!(seal_of(&mut s, f), Seal::Open);
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

    /// M3: a card file under the project's stem whose head names other
    /// rate is another source: pass 2 leaves the Part's absent, and it
    /// takes no stream row under a new id.
    #[test]
    fn pass_two_refuses_a_file_naming_another_ref() {
        let mut s = MemStore::new(1);
        let bed = source(6, 150_000);
        put(&mut s, &bed);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, bed.0.sref);
        p.source_absent(bed.0.sref).unwrap();
        let mut other = source(6, 150_000);
        other.0.sref.rate = 48_000;
        assert_eq!(source_file(other.0.sref), source_file(bed.0.sref));
        let mut s2 = MemStore::new(1);
        put(&mut s2, &other);
        let bytes = file_bytes(&mut s2, &other);
        let mut card = Card::new();
        assert!(matches!(
            save_at(&mut card, &mut s, &mut p, 1),
            ProjectNote::Saved(_)
        ));
        s.plant(source_file(bed.0.sref), &bytes);
        let (mut q, t) = Project::boxed();
        let vol = s.mount().unwrap();
        let id = ProjectId::new(1).unwrap();
        let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
        let out = load_project(&mut card, &mut s, &mut q, go, &LoadLink::new());
        assert!(out.swap.is_some(), "{:?}", out.note);
        let id = q.pool().find_source(bed.0.sref).expect("the ref kept");
        assert_eq!(q.pool().absence(id), Some(Absence::Missing));
        assert!(
            q.pool().find_source(other.0.sref).is_none(),
            "no row for the impostor"
        );
        let streams = q.pool().streams().unwrap();
        assert!(
            SourceId::ALL.into_iter().all(|i| streams.get(i).is_none()),
            "no stream row"
        );
    }

    /// M4: the gap moved under a streamed load between its passes (another
    /// source went in below it): it stages again and loads, never POOL FULL.
    #[test]
    fn a_streamed_load_whose_gap_moves_stages_again() {
        use chimera_core::project::test_support::set_free_to;
        let mut s = MemStore::new(1);
        let bed = source(7, 150_000);
        put(&mut s, &bed);
        let r = bed.0.sref;
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, r);
        let heads = heads_for(r, &p.part(PartId::ALL[0]).sound.params.tape).unwrap();
        let mut card = Card::new();
        let got = card
            .run(&mut s, |s, rd| {
                let mut l = StreamLoad::start(&mut p, bed.0, heads).unwrap();
                let mut moved = false;
                let mut passes = 0;
                loop {
                    passes += 1;
                    if passes == 3 && !moved {
                        let free = p.pool().free().bytes() as usize;
                        set_free_to(&mut p, chimera_core::project::arena::Cost::of(free - 4_096));
                        moved = true;
                    }
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
        assert_eq!(p.pool().find_source(r), Some(got));
        assert!(p.pool().streamed(r));
        // The heads read again whole at the new place: the same bytes a
        // clean load stages.
        let (mut clean, t) = Project::boxed();
        tape_part(&mut clean, t, 0, r);
        let mut card = Card::new();
        let done = card
            .run(&mut s, |s, rd| {
                let mut l = StreamLoad::start(&mut clean, bed.0, heads).unwrap();
                while l.step(s, rd, &mut clean, 4_096) == StreamStep::More {}
                Ok::<_, chimera_hal::store::StoreError>(())
            })
            .unwrap();
        done.result.unwrap();
        assert_eq!(p.pool().held(r), clean.pool().held(r));
    }

    /// A Part that plays its too-big source a way that needs it whole:
    /// pass 2 keeps it absent and the cell says why.
    #[test]
    fn pass_two_says_what_the_part_needs_whole() {
        use chimera_core::dsp::tape_engine::NeedsRam;
        use chimera_core::project::Refusal;
        use chimera_core::ui::source_cell::PING_NEEDS_RAM;
        use chimera_core::ui::view::SourceState;
        let mut s = MemStore::new(1);
        let bed = source(5, 150_000);
        put(&mut s, &bed);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, bed.0.sref);
        p.edit_part(PartId::ALL[0])
            .sound
            .params
            .tape
            .set_way(Way::Ping);
        p.source_absent(bed.0.sref).unwrap();
        let (q, _) = round_trip(&mut s, &mut p, false);
        let id = q.pool().find_source(bed.0.sref).unwrap();
        assert_eq!(
            q.pool().absence(id),
            Some(Absence::NeedsRam(Refusal::NeedsRam(NeedsRam::Ping)))
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

    /// A transwave too big for the arena can't stream, whatever it plays:
    /// under WALK LOOP pass 2 keeps it absent for its shape, and the cell
    /// says so, not NEEDS RAM (H3).
    #[test]
    fn pass_two_says_a_big_transwave_wont_stream() {
        use chimera_core::project::arena::Cost;
        use chimera_core::project::{LoadKind, Refusal, load_kind};
        use chimera_core::ui::draw::text_width;
        use chimera_core::ui::source_cell::TRANSWAVE_NO_STREAM;
        use chimera_core::ui::theme::{CELL_COL_W, CONTENT_W, FONT_VALUE, LABEL_TRACKING};
        use chimera_core::ui::view::SourceState;
        let mut s = MemStore::new(1);
        let (mut head, data) = source(6, 150_000);
        head.shape = Shape::Transwave;
        head.count = 2;
        head.sref.crc = source_crc(WholeShape::Transwave, 2, &data);
        let wave = (head, data);
        put(&mut s, &wave);
        let (mut p, t) = Project::boxed();
        tape_part(&mut p, t, 0, wave.0.sref);
        p.edit_part(PartId::ALL[0])
            .sound
            .params
            .tape
            .set_way(Way::Loop);
        let tape = p.part(PartId::ALL[0]).sound.params.tape;
        let shape = Refusal::Transwave;
        assert_eq!(
            load_kind(&wave.0, Cost::of(0), &tape),
            LoadKind::Refused(shape)
        );
        p.source_absent(wave.0.sref).unwrap();
        let (q, _) = round_trip(&mut s, &mut p, false);
        let id = q.pool().find_source(wave.0.sref).unwrap();
        assert_eq!(q.pool().absence(id), Some(Absence::NeedsRam(shape)));
        let st = SourceState::of(
            &q.part(PartId::ALL[0]).sound.params.tape,
            Some(wave.0.sref),
            q.pool(),
        );
        let (mut band, mut cell) = (
            chimera_core::ui::fmt::FmtBuf::new(),
            chimera_core::ui::fmt::FmtBuf::new(),
        );
        st.text(&mut band);
        st.cell(&mut cell);
        assert_eq!(band.as_str(), TRANSWAVE_NO_STREAM);
        assert_eq!(cell.as_str(), "TOO BIG");
        let w = text_width(&FONT_VALUE, band.as_str(), LABEL_TRACKING);
        assert!(w <= CONTENT_W, "band {w}px over {CONTENT_W}");
        let w = text_width(&FONT_VALUE, cell.as_str(), 0);
        assert!(w <= CELL_COL_W, "cell {w}px over {CELL_COL_W}");
    }
}
