//! TAPE's sources under the voice (TAPE Task 13, spec § 4): a Part's
//! source reaches the audio as an id, a voice latches it at note-on, a
//! note-on with nothing playable is refused before allocation, and the
//! `sounding` mask holds every source a voice reads or will.

mod common;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::tape_engine::params::TapeModel;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Shape};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{AUDIO_BUDGET_PERCENT, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::modulation::ModState;
use chimera_core::name::SoundName;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::save_at;
use chimera_core::project::{
    LoadLink, PartId, Project, ProjectNote, ProjectSource, ReplaceGuard, SourceId, SourceRef,
    load_project, project_file,
};
use chimera_core::scope::ScopeWriter;
use chimera_core::source_table::{AudioToken, SourceTable};
use chimera_core::storage::{
    Card, Event, Framer, ProjectId, ReadTag, RecordTag, Side, SourceHead, WholeShape, ZoneTune,
    source_crc, write_source,
};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use chimera_hal::store::{ReadSink, Store};
use chimera_hal::testkit::MemStore;
use common::tape::{held_plain, sine};
use common::{SR, scope_writer};
use core::ops::ControlFlow;

const RATE: u16 = 48_000;
const FRAMES: u32 = 9_600;

/// A Plain source at 48 kHz: a sine `period` frames long, as the arena
/// holds it, and a ref naming it.
fn plain(seed: u32, period: f32) -> (SourceRef, Vec<u8>) {
    let r = SourceRef {
        crc: 0x7A9E_0000 + seed,
        frames: FRAMES,
        rate: RATE,
    };
    (r, held_plain(&sine(RATE, period, FRAMES), RATE))
}

/// The audio side over `p`'s table: the instrument, and the table's one
/// reader. Each block begins a view, plays its events and the render
/// through it, and stores the instrument's mask.
struct Rig {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    shared: Box<AudioShared>,
    tok: AudioToken,
    table: &'static SourceTable,
}

impl Rig {
    fn of(p: &Project) -> Rig {
        Self::over(p.pool().table())
    }

    fn over(table: &'static SourceTable) -> Rig {
        Self::with_budget(table, SampleBudget::for_cpu(u32::MAX))
    }

    fn with_budget(table: &'static SourceTable, budget: SampleBudget) -> Rig {
        Rig {
            inst: Box::new(Instrument::new(SR, budget)),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: scope_writer(),
            shared: Box::default(),
            tok: table.reader().expect("one reader"),
            table,
        }
    }

    /// Part `part` plays INIT TAPE on `model`, with no route into VCA (its
    /// key holds a note), on `source`.
    fn tape(&mut self, part: usize, model: TapeModel, source: Option<SourceId>) {
        let a = &mut self.shared.parts[part];
        a.params = Sound::init(EngineType::Tape).params;
        a.params.tape.model = model;
        a.mod_state = ModState::new();
        a.source = source;
    }

    fn ev(&self, part: usize, note: u8, kind: NoteKind) -> NoteEvent {
        NoteEvent {
            channel: self.shared.parts[part].mix.channel,
            note: MidiNote::new(note).unwrap(),
            kind,
        }
    }

    fn on(&self, part: usize, note: u8) -> NoteEvent {
        self.ev(part, note, NoteKind::On(Velocity::DEFAULT))
    }

    fn off(&self, part: usize, note: u8) -> NoteEvent {
        self.ev(part, note, NoteKind::Off)
    }

    /// One block as the shells run one: Part 0's bus after it.
    fn block(&mut self, evs: &[NoteEvent]) -> [f32; BLOCK_SIZE] {
        {
            let v = self.tok.begin_block();
            for &e in evs {
                self.inst.handle_with(e, &self.shared, &v);
            }
            self.inst.render_with(
                &mut self.fx,
                &mut self.dac,
                &self.shared,
                &mut self.scope,
                &v,
            );
        }
        self.tok.store_sounding(self.inst.source_mask());
        *self.inst.part_bus(0)
    }

    fn refuses(&mut self, part: usize) -> bool {
        let v = self.tok.begin_block();
        self.inst.refuses(part, &self.shared, &v)
    }

    fn any_active(&self) -> bool {
        self.inst.active().iter().any(|&a| a)
    }

    fn slots(&self) -> String {
        format!("{:?}", self.inst.allocator().slots())
    }
}

fn loud(b: &[f32]) -> bool {
    b.iter().any(|&x| x.abs() > 1e-4)
}

#[test]
fn missing_source_refused_before_allocation() {
    let (mut p, _) = Project::boxed();
    let (ra, ha) = plain(1, 100.0);
    let (rg, _) = plain(2, 100.0);
    let a = p.source_store(ra, &ha).unwrap();
    let gone = p.source_absent(rg).unwrap();
    let mut r = Rig::of(&p);
    for (id, model, what) in [
        (None, TapeModel::Loop, "no source"),
        (Some(gone), TapeModel::Loop, "an absent one"),
        (Some(a), TapeModel::Transwave, "TRANSWAVE on Plain"),
    ] {
        r.tape(0, model, id);
        assert!(r.refuses(0), "{what}");
        let (slots, refused) = (r.slots(), r.inst.allocator().refused());
        let out = r.block(&[r.on(0, 60)]);
        assert_eq!(r.slots(), slots, "{what}: allocator state unchanged");
        assert_eq!(r.inst.allocator().refused(), refused, "{what}");
        assert!(!r.any_active() && !loud(&out), "{what}: no voice");
    }
    // Another engine never is; a playable source isn't.
    r.shared.parts[0].params = Sound::init(EngineType::Algo).params;
    r.shared.parts[0].source = None;
    assert!(!r.refuses(0), "ALGO");
    r.tape(0, TapeModel::Loop, Some(a));
    assert!(!r.refuses(0));
    r.block(&[r.on(0, 60)]);
    assert!(r.any_active(), "allocated");
    assert!(loud(&r.block(&[])), "and plays");
}

#[test]
fn sounding_bit_follows_the_voice() {
    let (mut p, _) = Project::boxed();
    let (ra, ha) = plain(1, 100.0);
    let (rb, hb) = plain(2, 37.0);
    let a = p.source_store(ra, &ha).unwrap();
    let b = p.source_store(rb, &hb).unwrap();
    let mut r = Rig::of(&p);
    r.tape(0, TapeModel::Loop, Some(a));
    r.block(&[r.on(0, 60)]);
    assert_eq!(r.table.sounding(), a.bit());
    r.block(&[]);
    // Key up: the voice fades over two blocks. A note on the same key
    // mid-fade waits behind it, latched to the Part's source now, B.
    r.block(&[r.off(0, 60)]);
    assert!(r.any_active(), "fading");
    assert_eq!(
        r.table.sounding(),
        a.bit(),
        "a fading voice holds its source"
    );
    r.shared.parts[0].source = Some(b);
    r.block(&[r.on(0, 60)]);
    assert_eq!(
        r.table.sounding(),
        a.bit() | b.bit(),
        "the fade's source, and the queued note's"
    );
    r.block(&[]);
    assert_eq!(r.table.sounding(), b.bit(), "the fade over, B plays");
    r.block(&[r.off(0, 60)]);
    let mut n = 0;
    while r.any_active() {
        assert_eq!(r.table.sounding(), b.bit(), "set while it sounds");
        r.block(&[]);
        n += 1;
        assert!(n < 10, "ends");
    }
    assert_eq!(r.table.sounding(), 0, "clear on the block after it ends");
}

/// A note stolen from another Part waits for that Part's fade: the source
/// it latched is held while it waits.
#[test]
fn a_steal_holds_the_waiting_notes_source() {
    let (mut p, _) = Project::boxed();
    let (ra, ha) = plain(1, 100.0);
    let (rb, hb) = plain(2, 37.0);
    let a = p.source_store(ra, &ha).unwrap();
    let b = p.source_store(rb, &hb).unwrap();
    // Room for one TAPE voice beside the bus.
    let one = Voice::cost(&Sound::init(EngineType::Tape).params, &ModState::new());
    let target = u64::from(FxBus::COST.0 + one.0 * 3 / 2);
    let hz = target * 100 * u64::from(SR) / u64::from(AUDIO_BUDGET_PERCENT) + u64::from(SR);
    let mut r = Rig::with_budget(p.pool().table(), SampleBudget::for_cpu(hz as u32));
    r.tape(0, TapeModel::Loop, Some(a));
    r.tape(1, TapeModel::Loop, Some(b));
    r.block(&[r.on(0, 60)]);
    r.block(&[r.on(1, 64)]);
    assert_eq!(r.inst.allocator().refused(), 0, "stolen, not refused");
    assert_eq!(r.table.sounding(), a.bit() | b.bit());
    r.shared.parts[1].source = None;
    let mut n = 0;
    while r.table.sounding() != b.bit() {
        r.block(&[]);
        n += 1;
        assert!(n < 10);
    }
    r.block(&[]);
    assert!(loud(r.inst.part_bus(1)), "B plays what it latched");
}

#[test]
fn gate_holds_tape_note_on() {
    let run = |gated: bool| {
        let (mut p, _) = Project::boxed();
        let (ra, ha) = plain(1, 100.0);
        let a = p.source_store(ra, &ha).unwrap();
        let mut r = Rig::of(&p);
        r.tape(0, TapeModel::Loop, Some(a));
        r.block(&[]);
        if gated {
            r.table.close_gate_for_test(a.bit());
            assert!(!r.refuses(0), "a moving source is no missing one");
            for _ in 0..2 {
                let out = r.block(&[r.on(0, 60)]);
                assert!(!loud(&out), "waits");
                assert!(r.any_active(), "allocated and latched");
                assert_eq!(r.table.sounding(), a.bit(), "held at the gate");
            }
            r.table.open_gate_for_test();
            r.block(&[])
        } else {
            r.block(&[r.on(0, 60)])
        }
    };
    let (late, now) = (run(true), run(false));
    assert!(loud(&now));
    assert_eq!(
        late, now,
        "starts on the first block after open, as unmoved"
    );
}

#[test]
fn retire_never_cuts_a_tape_note() {
    let run = |purge: bool| {
        let (mut p, _) = Project::boxed();
        let (ra, ha) = plain(1, 100.0);
        let (rb, hb) = plain(2, 37.0);
        let (rc, hc) = plain(3, 51.0);
        let a = p.source_store(ra, &ha).unwrap();
        p.source_store(rb, &hb).unwrap();
        p.source_store(rc, &hc).unwrap();
        // C, above B, is Part 2's.
        p.edit_part(PartId::ALL[1]).sound.source = Some(rc.into());
        let mut r = Rig::of(&p);
        r.tape(0, TapeModel::Loop, Some(a));
        let mut out = r.block(&[r.on(0, 60)]).to_vec();
        if purge {
            // A, sounding, is retired; B goes, and C moves down under it.
            let gone = p.purge_unused_sources(r.table.sounding());
            assert_eq!(gone.retired, 1);
        }
        let mut moved = false;
        // Compaction passes between blocks: as many blocks either way.
        while out.len() < 40 * BLOCK_SIZE {
            let mut k = 0;
            moved |= p.sources_pass_with(|| {
                out.extend(r.block(&[]));
                k += 1;
                k < 3
            });
            out.extend(r.block(&[]));
        }
        out.truncate(40 * BLOCK_SIZE);
        assert_eq!(moved, purge, "C moved under the sounding note");
        out
    };
    let (cut, kept) = (run(true), run(false));
    assert!(loud(&kept[kept.len() - 2 * BLOCK_SIZE..]));
    assert_eq!(cut, kept, "bit-identical while it sounds");
}

#[test]
fn source_switch_leaves_the_ringing_note() {
    let run = |switch: bool| {
        let (mut p, _) = Project::boxed();
        let (ra, ha) = plain(1, 100.0);
        let (rb, hb) = plain(2, 37.0);
        let a = p.source_store(ra, &ha).unwrap();
        let b = p.source_store(rb, &hb).unwrap();
        let mut r = Rig::of(&p);
        r.tape(0, TapeModel::Loop, Some(a));
        let mut out = r.block(&[r.on(0, 60)]).to_vec();
        if switch {
            r.shared.parts[0].source = Some(b);
        }
        for _ in 0..30 {
            out.extend(r.block(&[]));
        }
        out
    };
    assert_eq!(run(true), run(false));
}

/// A guard (TAPE Task 12 review): a row re-stored under a voice's id with
/// fewer frames than it latched ends the note, never reads past them.
#[test]
fn a_shorter_row_under_the_id_ends_the_note() {
    let (_, held) = plain(1, 100.0);
    let table: &'static SourceTable = Box::leak(Box::new(SourceTable::new()));
    let bytes: &'static mut [u8] = Box::leak(held.clone().into_boxed_slice());
    table.attach_for_test(bytes.as_mut_ptr());
    let a = SourceId::ALL[0];
    table.publish_for_test(a, 0, held.len() as u32, RATE);
    let mut r = Rig::over(table);
    r.tape(0, TapeModel::Loop, Some(a));
    r.block(&[r.on(0, 60)]);
    assert!(loud(&r.block(&[])));
    // 64 frames of it, under the same id.
    table.publish_for_test(a, 0, (DESC_LEN + 3 * 32) as u32, RATE);
    r.block(&[]);
    assert!(!r.any_active(), "ended");
    assert_eq!(r.table.sounding(), 0);
}

/// The Sound's tuning, not the file's (spec § 1): picking copies the
/// head's labels; the file re-imported with another root label plays
/// bit-identically.
#[test]
fn tuning_lives_in_the_sound() {
    let data: Vec<u8> = held_plain(&sine(RATE, 100.0, FRAMES), RATE)[DESC_LEN..].to_vec();
    let head = |root: u16, period: u32| SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, &data),
            frames: FRAMES,
            rate: RATE,
        },
        shape: Shape::Plain,
        count: 1,
        root,
        period_q16: period,
        name: SoundName::new("SINE").unwrap(),
    };
    let first = head(6_100, 100 << 16);
    let mut s = Sound::init(EngineType::Tape);
    s.pick_source(&first, None);
    assert_eq!(s.source.map(|t| t.r), Some(first.sref));
    assert_eq!(s.params.tape.root, 6_100.0, "not INIT's 6 000");
    assert_eq!(s.params.tape.period, 100.0);
    let play = |label: &SourceHead| {
        let mut st = MemStore::new(1);
        put_source(&mut st, label, &data);
        let (mut p, _) = Project::boxed();
        p.source_absent(label.sref).unwrap();
        *p.edit_part(PartId::ALL[0]).sound = s.clone();
        let (q, note) = round_trip(&mut st, &mut p);
        assert_eq!(note, None);
        assert_eq!(q.part(PartId::ALL[0]).sound.params.tape.root, 6_100.0);
        let mut r = Rig::of(&q);
        r.shared.update_from_project(&q, 0);
        let mut out = r.block(&[r.on(0, 60)]).to_vec();
        for _ in 0..8 {
            out.extend(r.block(&[]));
        }
        out
    };
    let a = play(&first);
    assert!(loud(&a));
    assert_eq!(play(&head(6_700, 37 << 16)), a, "the label never plays");
}

#[test]
fn project_round_trip_carries_no_audio() {
    let data: Vec<u8> = held_plain(&sine(RATE, 100.0, FRAMES), RATE)[DESC_LEN..].to_vec();
    let h = SourceHead {
        sref: SourceRef {
            crc: source_crc(WholeShape::Plain, 1, &data),
            frames: FRAMES,
            rate: RATE,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("SINE").unwrap(),
    };
    let mut s = Sound::init(EngineType::Tape);
    s.pick_source(&h, None);
    let mut st = MemStore::new(1);
    put_source(&mut st, &h, &data);
    let (mut p, _) = Project::boxed();
    p.source_absent(h.sref).unwrap();
    *p.edit_part(PartId::ALL[0]).sound = s.clone();
    let (q, _) = round_trip(&mut st, &mut p);
    assert_eq!(
        q.part(PartId::ALL[0]).sound.source.map(|t| t.r),
        Some(h.sref),
        "the ref kept"
    );
    let file = project_bytes(&mut st);
    let tags: Vec<ReadTag> = records(&file);
    assert!(tags.contains(&ReadTag::Known(RecordTag::SourceRef)));
    assert!(
        !tags.contains(&ReadTag::Known(RecordTag::SourceData)),
        "no audio"
    );
    assert!(file.len() < data.len(), "{} B", file.len());

    // On a card without the file: it loads, silent, and the Part reads
    // absent.
    let mut bare = MemStore::new(1);
    let (mut p, _) = Project::boxed();
    p.source_absent(h.sref).unwrap();
    *p.edit_part(PartId::ALL[0]).sound = s;
    let (q, note) = round_trip(&mut bare, &mut p);
    assert_eq!(note, Some(ProjectNote::SourcesMissing(1)));
    let mut r = Rig::of(&q);
    r.shared.update_from_project(&q, 0);
    let id = q.pool().find_source(h.sref).expect("the ref kept");
    assert_eq!(r.shared.parts[0].source, Some(id));
    assert!(r.refuses(0), "absent");
    assert!(!loud(&r.block(&[r.on(0, 60)])) && !r.any_active());
}

#[test]
fn zone_tune_clamps_to_the_sets_zones() {
    let t = ZoneTune::new(&[(6_000, 1), (6_100, 2)]).unwrap();
    assert_eq!(t.zone(0), (6_000, 1));
    assert_eq!(t.zone(1), (6_100, 2));
    assert_eq!(t.zone(5), (6_100, 2), "past the tune: the last tune's");
}

/// `p` saved as P0000001 on `s`, then loaded into a fresh project.
fn round_trip(s: &mut MemStore, p: &mut Project) -> (Box<Project>, Option<ProjectNote>) {
    let mut card = Card::new();
    assert!(matches!(save_at(&mut card, s, p, 1), ProjectNote::Saved(_)));
    let (mut q, t) = Project::boxed();
    let vol = s.mount().unwrap();
    let id = ProjectId::new(1).unwrap();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
    let out = load_project(&mut card, s, &mut q, go, &LoadLink::new());
    assert!(out.swap.is_some(), "{:?}", out.note);
    (q, out.note)
}

fn put_source(s: &mut MemStore, head: &SourceHead, data: &[u8]) {
    let mut card = Card::new();
    card.run(s, |s, r| write_source(s, r, head, data))
        .unwrap()
        .result
        .unwrap();
}

struct Bytes(Vec<u8>);

impl ReadSink for Bytes {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        self.0.extend_from_slice(b);
        ControlFlow::Continue(())
    }
}

fn project_bytes(s: &mut MemStore) -> Vec<u8> {
    let vol = s.mount().unwrap();
    let mut c = Bytes(Vec::new());
    s.read(
        vol,
        project_file(ProjectId::new(1).unwrap()).side(Side::A),
        &mut c,
    )
    .unwrap();
    c.0
}

fn records(bytes: &[u8]) -> Vec<ReadTag> {
    let mut f = Framer::new(bytes.len() as u32).unwrap();
    let mut v = Vec::new();
    f.push(bytes, &mut |e| {
        if let Event::Record(t, _) = e {
            v.push(t);
        }
        Ok(())
    })
    .unwrap();
    f.finish().unwrap();
    v
}

/// A glide steal on a note waiting at the gate moves the note it starts
/// on: it starts there, as a fresh note-on would.
#[test]
fn a_glide_on_a_gated_note_moves_its_start() {
    use chimera_core::dsp::tape_engine::{TapeEngine, params::TapeEngineParams};
    use common::tape::OneSource;
    let fresh = || {
        let mut slot = core::mem::MaybeUninit::uninit();
        *TapeEngine::init_in_place(&mut slot)
    };
    let (_, held) = plain(1, 100.0);
    let p = TapeEngineParams::default();
    let mut src = OneSource::new(&held);
    let mut e = fresh();
    e.latch(src.id);
    src.close();
    e.start(60, &p, &src.view(), SR);
    assert!(e.is_active());
    e.glide_to(64, (SR, 0.01));
    src.open();
    let mut late = [0.0; BLOCK_SIZE];
    e.play(&mut late, &p, &src.view(), SR);
    let mut f = fresh();
    f.latch(src.id);
    let mut now = [0.0; BLOCK_SIZE];
    f.start(64, &p, &src.view(), SR);
    f.play(&mut now, &p, &src.view(), SR);
    assert!(loud(&now));
    assert_eq!(late, now);

    // A model that can't play it, found only once the gate opens: idle.
    let wave = TapeEngineParams {
        model: TapeModel::Transwave,
        ..p
    };
    let mut g = fresh();
    g.latch(src.id);
    src.close();
    g.start(60, &wave, &src.view(), SR);
    src.open();
    let mut out = [1.0; BLOCK_SIZE];
    g.play(&mut out, &wave, &src.view(), SR);
    assert!(!g.is_active() && !loud(&out));
}
