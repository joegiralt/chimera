//! The TAPE factory set (spec § 7, TAPE Task 19): its CRCs pinned, a Sound
//! on every source, six Parts in the arena beside NEW, and every factory
//! TAPE Sound at the factory median (ADR 0063).

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::tape_engine::TapeModel;
use chimera_core::dsp::tape_engine::Way;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::Project;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::{AudioToken, GateView};
use chimera_core::{MidiNote, Velocity};
use chimera_desktop::factory_tapes::{
    ROOT, Recipes, build_all, held, manifest_crcs, originals, read_recipes, tape_sound,
};
use chimera_desktop::import::tape::Built;
use chimera_desktop::listen::lufs;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::sync::OnceLock;

/// Built once for the file: every render is seconds of audio.
fn set() -> &'static (Recipes, Vec<Option<Built>>) {
    static SET: OnceLock<(Recipes, Vec<Option<Built>>)> = OnceLock::new();
    SET.get_or_init(|| {
        let r = read_recipes().unwrap();
        let b = build_all(&r, &originals()).unwrap();
        (r, b)
    })
}

fn pinned() -> Vec<(String, u32)> {
    let path = format!("{ROOT}/tools/factory-tapes/manifest-v1.json");
    manifest_crcs(&std::fs::read_to_string(path).unwrap())
}

/// A fresh build reproduces every CRC `manifest-v1.json` pins, by name,
/// and pins no other. A CC0 original not in `$CHIMERA_TAPES_SRC` is
/// skipped, said so.
#[test]
fn manifest_crcs_are_pinned() {
    let (r, built) = set();
    let pins = pinned();
    assert_eq!(pins.len(), r.sources.len(), "a pin a source");
    for (s, b) in r.sources.iter().zip(built) {
        let pin = pins
            .iter()
            .find(|p| p.0 == s.name.as_str())
            .unwrap_or_else(|| panic!("{} unpinned", s.name.as_str()));
        match b {
            Some(b) => assert_eq!(
                b.head.sref.crc,
                pin.1,
                "{}: {:08X}, pinned {:08X}",
                s.name.as_str(),
                b.head.sref.crc,
                pin.1
            ),
            None => eprintln!("{}: its CC0 original is absent, skipped", s.name.as_str()),
        }
    }
}

/// Every factory source's descriptor reads back to its own bytes.
#[test]
fn factory_descriptors_round_trip() {
    for b in set().1.iter().flatten() {
        let h = held(b);
        let d = Descriptor::from_bytes(&h).expect("a descriptor");
        assert_eq!(d.to_bytes(), h[..DESC_LEN], "{:?}", b.head.sref);
    }
}

/// Each source has at least one factory TAPE Sound on it.
#[test]
fn every_source_has_a_sound() {
    let (r, _) = set();
    for (name, _) in pinned() {
        assert!(
            r.sounds.iter().any(|s| s.source.as_str() == name),
            "{name}: no Sound"
        );
    }
}

/// A six-Part project of the six largest Plain sources fits beside NEW:
/// each is stored in NEW's own arena.
#[test]
fn six_parts_fit() {
    let (_, built) = set();
    let mut plain: Vec<&Built> = built
        .iter()
        .flatten()
        .filter(|b| b.head.shape == Shape::Plain)
        .collect();
    plain.sort_by_key(|b| std::cmp::Reverse(b.packed.len()));
    let (mut p, _) = Project::boxed();
    for b in &plain[..6] {
        p.source_store(b.head.sref, &held(b))
            .unwrap_or_else(|_| panic!("{} doesn't fit", b.head.name.as_str()));
    }
}

/// P1 through the instrument, the Part on `sound` over the source table
/// `tok` reads.
struct Rig {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    shared: Box<AudioShared>,
    tok: Option<AudioToken>,
}

impl Rig {
    fn new(
        sound: &Sound,
        tok: Option<AudioToken>,
        id: Option<chimera_core::project::SourceId>,
    ) -> Rig {
        let mut shared = Box::<AudioShared>::default();
        *shared.parts[0].params = sound.params.clone();
        *shared.parts[0].mod_state = sound.mod_state.clone();
        shared.parts[0].source = id;
        Rig {
            inst: Box::new(Instrument::new(
                SAMPLE_RATE,
                SampleBudget::for_cpu(u32::MAX),
            )),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0),
            shared,
            tok,
        }
    }

    /// `note` at velocity 100, `blocks` of it held: ADR 0063's LUFS on P1.
    fn lufs(&mut self, note: u8, blocks: usize) -> f32 {
        let ev = NoteEvent {
            channel: self.shared.parts[0].mix.channel,
            note: MidiNote::new(note).unwrap(),
            kind: NoteKind::On(Velocity::new(100).unwrap()),
        };
        let mut p1 = Vec::new();
        for b in 0..=blocks {
            {
                let empty = GateView::empty();
                let view = self.tok.as_mut().map(|t| t.begin_block());
                let v = view.as_ref().unwrap_or(&empty);
                if b == 0 {
                    self.inst.handle_with(ev, &self.shared, v);
                }
                self.inst.render_with(
                    &mut self.fx,
                    &mut self.dac,
                    &self.shared,
                    &mut self.scope,
                    v,
                );
            }
            if let Some(t) = &mut self.tok {
                t.store_sounding(self.inst.source_mask());
            }
            if b > 0 {
                // The loudness before the limiter's knee (ADR 0109): the
                // median is a level between Sounds, not a ceiling's.
                p1.extend_from_slice(&self.dac.input()[0]);
            }
        }
        lufs(&p1)
    }
}

/// How far a factory TAPE Sound may sit from the median, dB.
const MEDIAN_DB: f32 = 1.0;

/// One second, in blocks: ADR 0063's hold.
const BPS: usize = SAMPLE_RATE as usize / BLOCK_SIZE;

/// Every factory TAPE Sound, its source's root (C4 for SLICE NOTE's first
/// slice) at velocity 100 on P1, sits within `MEDIAN_DB` of ALGO INIT's C4
/// (ADR 0063), measured over a second, or a ONESHOT's own length.
#[test]
fn factory_sounds_sit_at_the_median() {
    let (r, built) = set();
    let median = Rig::new(&Sound::init(EngineType::Algo), None, None).lufs(60, BPS);
    let mut bad = Vec::new();
    for s in &r.sounds {
        let Some(b) = r
            .sources
            .iter()
            .zip(built)
            .find(|(x, _)| x.name == s.source)
            .and_then(|(_, b)| b.as_ref())
        else {
            continue;
        };
        let snd = tape_sound(s, &b.head);
        let (mut p, _) = Project::boxed();
        let id = p.source_store(b.head.sref, &held(b)).unwrap();
        let tok = p.pool().table().reader().unwrap();
        let mut rig = Rig::new(&snd, Some(tok), Some(id));
        let t = snd.params.tape;
        let note = if t.slice == chimera_core::dsp::tape_engine::SliceCode::NOTE {
            60
        } else {
            ((b.head.root + 50) / 100) as u8
        };
        let secs = b.head.sref.frames as f32 / f32::from(b.head.sref.rate);
        let blocks = match (t.way(), t.slice.code()) {
            (Way::One, 1) => (secs / 16.0 * BPS as f32) as usize,
            (Way::One, _) => ((secs * BPS as f32) as usize).min(BPS),
            _ => BPS,
        };
        let d = rig.lufs(note, blocks.max(1)) - median;
        println!("{:<16} {:+6.2} dB", s.name.as_str(), d);
        if d.abs() > MEDIAN_DB {
            bad.push(format!("{}: {d:+.2} dB", s.name.as_str()));
        }
    }
    assert!(bad.is_empty(), "off the median:\n{}", bad.join("\n"));
}

/// Each factory Sound file the set writes loads back, on a card, as the
/// Sound its recipe builds: the same params by bits and the same source.
#[test]
fn sound_files_load_as_built() {
    use chimera_core::storage::{Card, LoadError, SoundDecoder, load_file};
    use chimera_desktop::factory_tapes::{sound_stem, write_set};
    use chimera_desktop::store::DirStore;
    use chimera_hal::store::{Dir, FileName};
    let (r, built) = set();
    if built.iter().any(Option::is_none) {
        eprintln!("a CC0 original is absent: skipped");
        return;
    }
    let tmp = std::env::temp_dir().join(format!("factory-tapes-{}", std::process::id()));
    let out = tmp.join("set");
    write_set(r, built, &originals(), &out).unwrap();
    let sounds = tmp.join("card/CHIMERA/SOUNDS");
    std::fs::create_dir_all(&sounds).unwrap();
    for (i, s) in r.sounds.iter().enumerate() {
        let stem = sound_stem(i);
        std::fs::copy(
            out.join(format!("{stem}.A")),
            sounds.join(format!("{stem}.A")),
        )
        .unwrap();
        let b = r
            .sources
            .iter()
            .zip(built)
            .find(|(x, _)| x.name == s.source)
            .and_then(|(_, b)| b.as_ref())
            .unwrap();
        let want = tape_sound(s, &b.head);
        let mut got = Sound::init(EngineType::Algo);
        let mut store = DirStore::new(tmp.join("card"));
        let file = FileName::new(Dir::Sounds, stem.as_bytes(), b"A").unwrap();
        Card::new()
            .run(&mut store, |st, rd| {
                load_file(st, rd, file, &mut SoundDecoder::new(&mut got)).map(|_| ())
            })
            .and_then(|o| o.result)
            .unwrap_or_else(|e: LoadError| panic!("{stem}: {e:?}"));
        assert!(got.bits_eq(&want), "{}", s.name.as_str());
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// SCRUB at its machine defaults on note 60, MOTION 0.6 → −0.6 over 3 s
/// with a fresh take at 1.5 s, where MOTION is about 0 (#383): the take
/// starts forward and turns, and adds no click the source played straight
/// hasn't got (the QA's rule).
#[test]
fn scrub_take_that_turns_is_click_free() {
    use chimera_core::block::Block as _;
    use chimera_core::dsp::tape_engine::machine::defaults;
    use chimera_core::dsp::tape_engine::{StretchType, TapeEngineParams as P};
    use chimera_desktop::factory_tapes::pitch;
    use chimera_desktop::listen::{CLICK_RATIO, Script, Way, qa, render, root_note};
    static RAMP: [(f32, chimera_core::block::ParamId, f32); 2] =
        [(0.0, P::MOTION, 0.6), (3.0, P::MOTION, -0.6)];
    let (r, built) = set();
    for name in ["SINE C4", "RAIN", "PAD CHORD"] {
        let b = r
            .sources
            .iter()
            .zip(built)
            .find(|(s, _)| s.name.as_str() == name)
            .and_then(|(_, b)| b.as_ref())
            .unwrap();
        let (h, p) = (held(b), pitch(&b.head));
        let straight = render(
            Way::One.base(TapeModel::Walk, StretchType::Mono),
            &h,
            p,
            root_note(p),
            &Way::One.script(TapeModel::Walk),
        );
        let own = qa(&straight[..straight.len().min(48_000 * 5 / 2)]).click_ratio;
        let mut base = Way::Plain.base(TapeModel::Scrub, StretchType::Mono);
        for (id, v) in defaults(
            chimera_core::dsp::tape_engine::TapeMachine::Scrub,
            chimera_core::dsp::tape_engine::Direction::Loop,
        ) {
            base.write(id, v);
        }
        let y = render(base, &h, p, 60, &Script(&RAMP, &[1.5], &[]));
        let q = qa(&y);
        assert!(
            q.click_ratio <= CLICK_RATIO.max(own * 1.05),
            "{name}: click {:.1}x at {:.3} s (straight {own:.2}x)",
            q.click_ratio,
            q.click_at as f32 / 48_000.0
        );
    }
}

#[path = "../../chimera-core/tests/common/codec_util.rs"]
mod codec_util;

/// Every factory TAPE Sound loads and saves to its file's bytes (ADR
/// 0189), and those bytes are pinned. An absent CC0 original's Sounds
/// stand on another source, and the pin waits for the whole set.
#[test]
fn factory_sounds_save_as_loaded() {
    use chimera_core::storage::Crc32;
    use codec_util::{decode, encode};
    let (r, built) = set();
    let stand_in = &built.iter().flatten().next().unwrap().head;
    let mut all = Crc32::new();
    for s in &r.sounds {
        let head = r
            .sources
            .iter()
            .zip(built)
            .find(|(x, _)| x.name == s.source)
            .and_then(|(_, b)| b.as_ref())
            .map_or(stand_in, |b| &b.head);
        let file = encode(&tape_sound(s, head));
        let got = decode(&file).unwrap();
        assert_eq!(encode(&got), file, "{}", s.name.as_str());
        // Not the file's own CRC trailer: a CRC run over a message and
        // its CRC is a constant of the length, blind to the content.
        all.update(&file[..file.len() - 4]);
    }
    if built.iter().all(Option::is_some) {
        // ADR 0205: SAW SCRUB and RAIN SCRUB take SCRUB's new defaults
        // (LENGTH 70 ms, KEY POSITION) and their re-trims. ADR 0203: MODE
        // ONE's Sounds save FORWARD (3) and LENGTH 1, TRON BOWED WALK with
        // TRON on; each renders as before (`tape_legacy_test`). ADR 0204: the
        // STRETCH Sounds turn to GRAIN, with ONE's ENV 1, re-trimmed.
        // Re-pinned on the integration's merged tree.
        assert_eq!(all.finish(), 0x8DC4_E96D, "{} Sounds", r.sounds.len());
    } else {
        let absent = r.sources.iter().zip(built).filter(|(_, b)| b.is_none());
        let names: Vec<_> = absent.map(|(x, _)| x.name.as_str()).collect();
        eprintln!(
            "CRC pin skipped, CC0 originals absent: {}",
            names.join(", ")
        );
    }
}
