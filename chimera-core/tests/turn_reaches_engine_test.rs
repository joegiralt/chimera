//! A hand's turn reaches the sound on every Part engine, as the unit runs
//! it: a `UiState` fed encoder frames, published each pass to a real
//! `Instrument` (the board's `publish`), with bank 2's door open where the
//! build links it. FILTER's CUTOFF, AMP's LPG and AMP › SENDS' three
//! sends, each turned by hand, move what the voice plays and what the FX
//! bus returns, with the gate at 0 and above it. Bank 2's own: the gate,
//! VA's NOISE, SEM, the reverb and the delay's GRAIN, each turned on by
//! hand, play.

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, FilterParams, OutParams};
use chimera_core::part::DacPair;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::scope::ScopeWriter;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::{BlockDef, slot_addr};
use chimera_core::ui::block_registry::{AMP, DELAY, FILTER, SENDS, VA_HOME, VA_NOISE};
use chimera_core::ui::nav::PartChain;
use chimera_core::ui::view::SlotCtx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, ButtonId, EncoderId, SAMPLE_RATE};
use screen::{Input, feed};

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const LPG: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::LPG);
const ENCODERS: [EncoderId; 6] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];

struct Bench {
    ui: Box<UiState>,
    shared: Box<AudioShared>,
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    knobs: Box<KnobPublisher>,
}

impl Bench {
    /// Part 1 on `e`'s INIT, sustained, dry on pair 3: pair 1 carries the
    /// FX return alone.
    fn new(e: EngineType) -> Self {
        chimera_core::bank2::DOOR_CELL.open_host_if_linked();
        // Under the firmware's `boot-waves`, the boot's copy of the tables.
        #[cfg(feature = "boot-waves")]
        {
            static WAVES: std::sync::Once = std::sync::Once::new();
            WAVES.call_once(|| {
                chimera_core::dsp::algo::waves::render_into(Box::leak(Box::new_uninit()))
            });
        }
        let mut s = Sound::init(e);
        s.params.envelopes[0].sustain = 1.0;
        let mut ui = Box::new(screen::stack_ui());
        screen::init_part(&mut ui, P1);
        let was = PartChain::of(&ui.project().part(P1).sound);
        {
            let p = ui.project_mut().edit_part(P1);
            **p.sound = s;
            p.mix.output = DacPair::P3;
        }
        ui.replaced_for_test(was);
        let mut b = Self {
            ui,
            shared: Box::new(AudioShared::default()),
            inst: Box::new(Instrument::new(
                SAMPLE_RATE,
                SampleBudget::for_cpu(CPU_HZ_REV_V),
            )),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: common::scope_writer(),
            knobs: Box::new(KnobPublisher::new()),
        };
        b.publish();
        b
    }

    /// The board's `publish`: the project, then the UI's own part.
    fn publish(&mut self) {
        self.shared
            .update_from_project(self.ui.project(), 0, &mut self.knobs);
        self.ui.publish_ui(&mut self.shared);
    }

    /// Three audio blocks; pair 1's energy over them.
    fn render(&mut self, blocks: usize) -> f32 {
        let mut e = 0.0;
        for _ in 0..blocks {
            self.inst
                .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
            e += self.dac.out()[0].iter().map(|x| x * x).sum::<f32>();
        }
        e
    }

    /// A UI frame, a publish, a pass's blocks.
    fn frame(&mut self, i: Input) {
        feed(&mut self.ui, i);
        self.publish();
        self.render(3);
    }

    fn note_on(&mut self) {
        let ev = NoteEvent {
            channel: self.ui.project().part(P1).mix.channel,
            note: MidiNote::new(57).unwrap(),
            kind: NoteKind::On(Velocity::DEFAULT),
        };
        self.inst.handle(ev, &self.shared);
        self.render(20);
    }

    fn go_to(&mut self, def: &BlockDef) {
        for _ in 0..12 {
            if self.ui.page_def().id == def.id {
                return;
            }
            self.frame(Input::press(ButtonId::Plus));
        }
        panic!("page {} not reached", def.name);
    }

    /// The encoder over `a` on the page, turned `n` detents.
    fn turn(&mut self, a: ParamAddr, n: i8) {
        let def = self.ui.page_def();
        let ctx = SlotCtx::read(&self.ui.project().read_part(P1), Op::A);
        let slot = (0..def.params.len())
            .position(|s| slot_addr(def, s, &ctx) == Some(a))
            .unwrap_or_else(|| panic!("{a:?} on {}", def.name));
        for _ in 0..8 {
            self.frame(Input::turn(ENCODERS[slot], n));
        }
        self.render(60);
    }

    /// EDIT down `def`'s chain block to its sub-page `sub`.
    fn go_down_to(&mut self, sub: &BlockDef) {
        for _ in 0..6 {
            if self.ui.page_def().id == sub.id {
                return;
            }
            self.frame(Input::press(ButtonId::Edit));
        }
        panic!("sub-page {} not reached", sub.name);
    }

    fn stored(&self, a: ParamAddr) -> f32 {
        let part = self.ui.project().read_part(P1);
        part.block(a.block).unwrap().get(a.param)
    }

    fn played(&self, a: ParamAddr) -> f32 {
        let busy = self.inst.allocator().slots().map(|s| !s.is_free());
        let v = busy.iter().position(|&b| b).expect("a voice sounds");
        self.inst.played(a)[v]
    }
}

const ENGINES: [EngineType; 3] = [EngineType::Algo, EngineType::Modal, EngineType::Va];

#[test]
fn a_turned_cutoff_reaches_the_voice() {
    for e in ENGINES {
        let mut b = Bench::new(e);
        b.note_on();
        b.go_to(&FILTER);
        let (stored, played) = (b.stored(CUTOFF), b.played(CUTOFF));
        b.turn(CUTOFF, -4);
        assert!(b.stored(CUTOFF) < stored, "{e:?}: CUTOFF stored");
        assert!(
            b.played(CUTOFF) < played,
            "{e:?}: CUTOFF played {} → {}",
            played,
            b.played(CUTOFF)
        );
    }
}

#[test]
fn a_turned_lpg_reaches_the_voice() {
    for e in ENGINES {
        let mut b = Bench::new(e);
        b.note_on();
        b.go_to(&AMP);
        b.turn(LPG, 4);
        assert!(b.stored(LPG) > 0.0, "{e:?}: LPG stored");
        assert!(b.played(LPG) > 0.0, "{e:?}: LPG played");
    }
}

/// Each send turned up on AMP › SENDS: the FX bus returns on pair 1,
/// where nothing returns with every send at 0. With LPG at 0 and up.
#[test]
fn turned_sends_reach_the_fx_bus() {
    for e in ENGINES {
        for lpg in [false, true] {
            for (k, send) in OutParams::SENDS.into_iter().enumerate() {
                let mut b = Bench::new(e);
                b.note_on();
                b.go_to(&AMP);
                if lpg {
                    b.turn(LPG, 4);
                }
                let quiet = b.render(200);
                assert!(
                    quiet < 1e-12,
                    "{e:?} lpg {lpg}: a return at sends 0: {quiet}"
                );
                // AMP › STEREO (ADR 0294), then AMP › SENDS.
                b.frame(Input::press(ButtonId::Edit));
                b.frame(Input::press(ButtonId::Edit));
                assert_eq!(b.ui.page_def().id, SENDS.id, "{e:?}: AMP › SENDS");
                let a = ParamAddr::new(BlockRef::Out, send);
                b.turn(a, 6);
                assert!(b.stored(a) > 0.0, "{e:?}: send {k} stored");
                let wet = b.render(400) / (400 * 2 * BLOCK_SIZE) as f32;
                assert!(
                    wet > 1e-8,
                    "{e:?} lpg {lpg}: send {k} returns nothing ({wet:e})"
                );
            }
        }
    }
}

/// VA NOISE's LEVEL, bank 2's pass, turned up on VA › NOISE.
#[test]
fn a_turned_noise_reaches_the_voice() {
    let noise = ParamAddr::new(BlockRef::Va, chimera_core::dsp::va::VaParams::NOISE);
    let mut b = Bench::new(EngineType::Va);
    b.note_on();
    b.go_to(&VA_HOME);
    b.go_down_to(&VA_NOISE);
    b.turn(noise, 6);
    assert!(b.stored(noise) > 0.0, "NOISE stored");
    assert!(b.played(noise) > 0.0, "NOISE played");
}

/// FILTER's TYPE turned to SEM, bank 2's kernel: the voice plays it.
#[test]
fn a_turned_type_plays_sem() {
    use chimera_core::dsp::filter::FilterKind;
    let kind = ParamAddr::new(BlockRef::Filter, FilterParams::KIND);
    for e in ENGINES {
        let mut b = Bench::new(e);
        b.note_on();
        b.go_to(&FILTER);
        for _ in 0..8 {
            if b.ui.project().part(P1).sound.params.filter.kind() == FilterKind::Sem {
                break;
            }
            b.turn(kind, 1);
        }
        assert_eq!(
            b.ui.project().part(P1).sound.params.filter.kind(),
            FilterKind::Sem,
            "{e:?}"
        );
        let busy = b.inst.allocator().slots().map(|s| !s.is_free());
        let v = busy.iter().position(|&x| x).expect("a voice sounds");
        assert_eq!(
            b.inst.played_filters()[v].kind(),
            FilterKind::Sem,
            "{e:?}: played"
        );
    }
}

/// The delay's TYPE turned to GRAIN on the mixer, bank 2's kernel, and
/// the DELAY send up: the bus returns it.
#[test]
fn a_grain_delay_turned_on_returns() {
    use chimera_core::dsp::delay::{DelayParams, DelayType};
    let ty = ParamAddr::new(BlockRef::Delay, DelayParams::TYPE);
    let send = ParamAddr::new(BlockRef::Out, OutParams::SEND2);
    let mut b = Bench::new(EngineType::Algo);
    b.note_on();
    b.frame(Input::chord(ButtonId::Mix, ButtonId::B1));
    b.go_to(&SENDS);
    b.turn(send, 6);
    b.go_to(&DELAY);
    b.turn(ty, 1);
    assert_eq!(b.ui.project().perf().fx.delay.kind(), DelayType::Grain);
    let wet = b.render(400) / (400 * 2 * BLOCK_SIZE) as f32;
    assert!(wet > 1e-8, "GRAIN returns nothing ({wet:e})");
}

/// The delay's SYNC turned on by hand on DLY › CHAR, then TIME, now its
/// note, turned down a detent on DELAY (ADR 0302): the engine's head
/// moves to 1/8D at the project's 120 BPM, then to 1/4T, and the free
/// TIME stays the ms it was.
#[test]
fn a_synced_delay_turned_by_hand_plays_its_note() {
    use chimera_core::dsp::delay::{DelayNote, DelayParams, DelaySync};
    use chimera_core::ui::block_registry::DELAY_CHAR;
    let at = |id| ParamAddr::new(BlockRef::Delay, id);
    let mut b = Bench::new(EngineType::Algo);
    b.frame(Input::chord(ButtonId::Mix, ButtonId::B1));
    b.go_to(&DELAY);
    b.turn(at(DelayParams::TIME_MS), -4);
    let free = b.ui.project().perf().fx.delay.time_ms;
    assert!(free < 375.0);
    assert_eq!(b.fx.delay().head(), (free * 48.0, 0));
    b.go_down_to(&DELAY_CHAR);
    b.turn(at(DelayParams::SYNC), 1);
    assert_eq!(b.ui.project().perf().fx.delay.sync(), DelaySync::Sync);
    assert_eq!(b.fx.delay().head(), (18_000.0, 0), "1/8D at 120 BPM");
    // Back on DELAY the TIME cell is the note: one detent, one note.
    // EDIT wraps past DLY › GRAIN.
    b.go_down_to(&DELAY);
    let slot = 1;
    let ctx = b.ui.slot_ctx_for_test();
    assert_eq!(slot_addr(&DELAY, slot, &ctx), Some(at(DelayParams::NOTE)));
    b.frame(Input::turn(ENCODERS[slot], -1));
    b.render(60);
    let d = b.ui.project().perf().fx.delay;
    assert_eq!(d.note().index(), DelayNote::DEFAULT.index() - 1);
    assert_eq!(d.time_ms, free, "the free TIME is kept");
    assert_eq!(b.fx.delay().head(), (16_000.0, 0), "1/4T at 120 BPM");
}
