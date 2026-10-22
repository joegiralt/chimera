//! ORBIT through bank 2's door (ADR 0208): its event output, its view
//! and the whole engine's audio, step for step, hashed over several songs.
//! The hashes were taken from main before ORBIT moved, so a build without
//! bank 2 (bank 1's own) and a host with the door open (`--features
//! chimera-engine/bank2`, `open_host`) both play what ORBIT played. With
//! the door closed ORBIT sends nothing and its view says stopped.

mod common;

use chimera_core::block::Block;
use chimera_core::note_queue::RealTime;
use chimera_core::orbit::clock::{Bpm, SyncMode};
use chimera_core::orbit::ring::NoteMode;
use chimera_core::orbit::scene::{SceneIdx, SceneRing};
use common::codec_util::decode_project;
use common::orbit::OrbitRig;

const ORBIT_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v1/orbit.prj");

/// FNV-1a, 64 bits: stable across toolchains, as `DefaultHasher` isn't.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ u64::from(x)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// What a run hashed: ORBIT's events, its view, the summed audio.
#[derive(Debug, PartialEq, Eq)]
struct Hashes {
    events: u64,
    view: u64,
    audio: u64,
    ons: usize,
}

/// `blocks` blocks of `rig`, `each` called before each block.
fn run(rig: &mut OrbitRig, blocks: usize, mut each: impl FnMut(&mut OrbitRig, usize)) -> Hashes {
    let (mut ev, mut view, mut audio) = (Fnv::new(), Fnv::new(), Fnv::new());
    let mut ons = 0;
    let from = rig.sum.len();
    for b in 0..blocks {
        each(rig, b);
        rig.block();
        for e in &rig.sent {
            ev.bytes(format!("{b} {e:?}\n").as_bytes());
            ons += matches!(e.kind, chimera_core::orbit::engine::OrbitKind::On(_)) as usize;
        }
        let v = rig.view();
        view.bytes(
            format!(
                "{} {:?} {:?} {:?} {} {} {} {:?} {:?} {:?}\n",
                v.running,
                v.source,
                v.bpm,
                v.pos,
                v.beats,
                v.loops,
                v.seg,
                v.chord,
                v.cue,
                v.rings.map(|r| (r.step, r.fired, r.hits)),
            )
            .as_bytes(),
        );
    }
    for x in &rig.sum[from..] {
        audio.bytes(&x.to_bits().to_le_bytes());
    }
    Hashes {
        events: ev.0,
        view: view.0,
        audio: audio.0,
        ons,
    }
}

/// NEW's INIT scene from PLAY, two bars at 120 BPM.
fn init_scene() -> Hashes {
    let mut rig = OrbitRig::new();
    rig.play(true);
    run(&mut rig, 3000, |_, _| {})
}

/// The frozen ORBIT fixture: eight scenes, drift, a CUSTOM scale, a played
/// chord, 97 BPM; played, then slot 3 queued onto the loop, then STOP.
fn fixture() -> Hashes {
    let mut rig = OrbitRig::new();
    let f = std::fs::read(ORBIT_FIXTURE).expect("the fixture");
    decode_project(&f, &mut rig.project).expect("loads");
    rig.load();
    rig.run(2);
    rig.play(true);
    run(&mut rig, 4000, |rig, b| {
        if b == 600 {
            let s = SceneIdx::ALL[2];
            rig.ui.queue(s, rig.project.orbit_mut());
        }
        if b == 3600 {
            rig.play(false);
        }
    })
}

/// Every ring busy: MUTATE high, a STEP ring, 173 BPM, a live capture into
/// ring 2 and a BPM change mid-run.
fn dense() -> Hashes {
    let mut rig = OrbitRig::new();
    rig.song().edit(&mut |s| {
        for (i, r) in s.rings.iter_mut().enumerate() {
            r.set(SceneRing::MUTE, 0.0);
            r.set(SceneRing::MUTATE, 80.0 + i as f32 * 4.0);
            r.set(SceneRing::STEPS, 5.0 + i as f32 * 3.0);
        }
        s.rings[3].ring.mode = NoteMode::Step;
        s.seed = 0x0B17_A55E;
    });
    rig.song().bpm = Bpm::new(173).unwrap();
    rig.ui.flush(&mut rig.project);
    rig.play(true);
    run(&mut rig, 4000, |rig, b| match b {
        300 => {
            use chimera_core::orbit::RingIdx;
            rig.ui
                .arm(Some(chimera_core::orbit::channel::ArmTarget::Ring(
                    RingIdx::ALL[1],
                )));
        }
        320 => rig.key(0, 62, true),
        330 => rig.key(0, 65, true),
        340 => rig.key(0, 69, true),
        400 => {
            rig.key(0, 62, false);
            rig.key(0, 65, false);
            rig.key(0, 69, false);
        }
        2000 => rig.song().bpm = Bpm::new(88).unwrap(),
        _ => {}
    })
}

/// A DAW's clock under SYNC EXT: Start, 24 clocks a beat at 125 BPM (one
/// every 7.5 blocks), Stop, Continue.
fn midi_clock() -> Hashes {
    let mut rig = OrbitRig::new();
    rig.sync = SyncMode::Ext;
    let mut next = 0.0f64;
    run(&mut rig, 3000, |rig, b| {
        if b == 10 {
            rig.rt(0, RealTime::Start);
        }
        if b == 1500 {
            rig.rt(0, RealTime::Stop);
        }
        if b == 1800 {
            rig.rt(0, RealTime::Continue);
        }
        while next <= b as f64 {
            rig.rt(0, RealTime::Clock);
            next += 7.5;
        }
    })
}

/// The songs' hashes, as main played them before ORBIT moved (5a2e50e7).
/// Dense's audio through the master tape stage (`master-tape`).
const DENSE_MASTER_TAPE_AUDIO: u64 = 10036277446996524101;

const WANT: [(&str, Hashes); 4] = [
    (
        "init",
        Hashes {
            events: 16712541784421469427,
            view: 5799786289991752721,
            audio: 6712420641175603369,
            ons: 14,
        },
    ),
    (
        "fixture",
        Hashes {
            events: 14629289380968475772,
            view: 1229182318731282432,
            audio: 17022195276077958477,
            ons: 32,
        },
    ),
    (
        "dense",
        Hashes {
            events: 14643758011103609088,
            view: 4960121372176126917,
            audio: 15069732824941459853,
            ons: 47,
        },
    ),
    (
        "midi_clock",
        Hashes {
            events: 7253012048407308496,
            view: 8363854142273624929,
            audio: 908574724158595525,
            ons: 27,
        },
    ),
];

/// A song: its name and its run.
type Song = (&'static str, fn() -> Hashes);

fn songs() -> [Song; 4] {
    [
        ("init", init_scene),
        ("fixture", fixture),
        ("dense", dense),
        ("midi_clock", midi_clock),
    ]
}

/// The door, open on the host's own table while held; without bank 2
/// there is none to open. The door is a process-wide static: the tests
/// take turns.
mod door {
    use std::sync::{Mutex, MutexGuard};

    static TURN: Mutex<()> = Mutex::new(());

    pub struct Open(#[allow(dead_code)] MutexGuard<'static, ()>);

    pub fn turn() -> MutexGuard<'static, ()> {
        TURN.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn open() -> Open {
        let g = turn();
        #[cfg(feature = "bank2")]
        chimera_core::bank2::DOOR_CELL.open_host();
        Open(g)
    }

    impl Drop for Open {
        fn drop(&mut self) {
            #[cfg(feature = "bank2")]
            chimera_core::bank2::DOOR_CELL.close();
        }
    }
}

#[test]
fn orbit_plays_as_it_did_before_the_move() {
    let _open = door::open();
    let got: Vec<_> = songs().into_iter().map(|(name, f)| (name, f())).collect();
    for (name, h) in &got {
        println!("{name}: {h:?}");
    }
    for ((name, got), (want_name, want)) in got.into_iter().zip(WANT) {
        assert_eq!(name, want_name);
        assert!(got.ons > 0, "{name}: ORBIT played nothing");
        // The master tape stage (ADR 0055) colours a sum loud enough to
        // reach it: only dense's.
        let want = if cfg!(feature = "master-tape") && name == "dense" {
            Hashes {
                audio: DENSE_MASTER_TAPE_AUDIO,
                ..want
            }
        } else {
            want
        };
        assert_eq!(got, want, "{name}");
    }
}

/// Closed, ORBIT is stopped: no song sends a note, B6 or a DAW's Start
/// notwithstanding, and the view never says it runs.
#[cfg(feature = "bank2")]
#[test]
fn orbit_with_the_door_closed_plays_nothing() {
    let _g = door::turn();
    assert!(!chimera_core::bank2::DOOR_CELL.is_open());
    for (name, f) in songs() {
        let got = f();
        assert_eq!(got.ons, 0, "{name}");
    }
    let mut rig = OrbitRig::new();
    rig.play(true);
    rig.run(500);
    assert!(!rig.view().running);
    assert!(rig.sent.is_empty());
}

/// Closed, a project's ORBIT loads, plays nothing and saves as it loaded:
/// no scene, drift or seed lost.
#[cfg(feature = "bank2")]
#[test]
fn orbit_with_the_door_closed_keeps_the_project() {
    let _g = door::turn();
    let mut rig = OrbitRig::new();
    let f = std::fs::read(ORBIT_FIXTURE).expect("the fixture");
    decode_project(&f, &mut rig.project).expect("loads");
    rig.load();
    let loaded = common::codec_util::encode_project(&rig.project);
    rig.play(true);
    rig.run(3000);
    assert_eq!(common::codec_util::encode_project(&rig.project), loaded);
}

/// Counts the pixels drawn.
#[cfg(feature = "bank2")]
struct Count(usize);

#[cfg(feature = "bank2")]
impl embedded_graphics::geometry::OriginDimensions for Count {
    fn size(&self) -> embedded_graphics::geometry::Size {
        embedded_graphics::geometry::Size::new(320, 240)
    }
}

#[cfg(feature = "bank2")]
impl embedded_graphics::draw_target::DrawTarget for Count {
    type Color = embedded_graphics::pixelcolor::Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>>(
        &mut self,
        p: I,
    ) -> Result<(), Self::Error> {
        self.0 += p.into_iter().count();
        Ok(())
    }
}

/// Closed, ORBIT's pages say so: the header warns UPDATE, its params read
/// NEEDS UPDATE, its own cells UPDATE, and the bands and their key are
/// empty. Open, the same bands draw.
#[cfg(feature = "bank2")]
#[test]
fn orbit_pages_with_the_door_closed_say_so() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::bank2::{
        Bank2Status, DOOR_CELL, NEEDS_UPDATE, NEEDS_UPDATE_CELL, closed_orbit, closed_text,
    };
    use chimera_core::orbit::RingIdx;
    use chimera_core::ui::orbit::door as pages;
    use chimera_core::ui::orbit::draw::OrbitFrame;
    use chimera_core::ui::orbit::{OrbitCell, OrbitUi};

    let _g = door::turn();
    let ui = Box::new(chimera_core::ui::UiState::new());
    let orbit = OrbitUi::default();
    let f = OrbitFrame::new(ui.project().orbit(), &orbit);
    let r = RingIdx::ALL[0];
    type Band = fn(&mut Count, &OrbitFrame, RingIdx);
    let pixels = |band: Band| {
        let mut c = Count(0);
        band(&mut c, &f, r);
        c.0
    };
    let bands: [Band; 4] = [pages::rhythm, pages::voice, pages::notes, pages::orb];

    DOOR_CELL.open_host();
    assert!(
        bands.iter().all(|&b| pixels(b) > 0),
        "open, every band draws"
    );
    assert_ne!(
        pages::cell_text(OrbitCell::Ring, &f, r).as_str(),
        NEEDS_UPDATE_CELL
    );
    DOOR_CELL.close();

    let closed = Some(Bank2Status::Empty);
    assert_eq!(closed_orbit(closed), Some(NEEDS_UPDATE_CELL));
    let bpm = ParamAddr::new(BlockRef::Tempo, chimera_core::block::ParamId(0));
    assert_eq!(closed_text(bpm, 0.5, closed), Some(NEEDS_UPDATE));
    assert_eq!(
        pages::cell_text(OrbitCell::Ring, &f, r).as_str(),
        NEEDS_UPDATE_CELL
    );
    assert!(
        bands.iter().all(|&b| pixels(b) == 0),
        "closed, the bands are blank"
    );
    let mut c = Count(0);
    pages::chords(&mut c, &f);
    pages::scenes(&mut c, &f);
    pages::circle(&mut c, &f, 0.0);
    assert_eq!(c.0, 0);
}
