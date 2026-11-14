//! DRUM end to end (slice 1 Task 9), as the desktop runs it: a `UiState`
//! loads DRUM INIT on Part 1 from the browser, publishes each pass to a
//! real `Instrument` (project, UI, kits), and notes 0–11 on Part 1's
//! channel, one block apart, strike the twelve pads: each sounds at its
//! onset, KIT's band lights every pad a UI frame struck, and the last
//! struck, pad 12, ends selected. Then two pads struck together both
//! light, and each stays lit while it sounds and goes dark when it ends
//! (#950). Slice 2 (ADR 0269): FLT's turns set each pad's own filter,
//! and its hits play it.

mod common;
mod screen;

use chimera_core::dsp::drum::types::PadId;
use chimera_core::dsp::drum::view::KitWriter;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::project::PartId;
use chimera_core::scope::ScopeWriter;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::DRUM_KIT;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, ButtonId, EncoderId, SAMPLE_RATE};
use screen::{Input, feed};

const P1: PartId = PartId::ALL[0];
/// Two seconds of blocks.
const BLOCKS: usize = 2 * SAMPLE_RATE as usize / BLOCK_SIZE;

/// A block's energy at or under this is silence: −100 dBFS RMS.
/// SHAKER's grains scatter: its onset block holds a first grain at −90.
const SILENT: f32 = 1e-10 * (2 * BLOCK_SIZE) as f32;

struct Desk {
    ui: Box<UiState>,
    shared: Box<AudioShared>,
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
    knobs: Box<KnobPublisher>,
    kits: KitWriter,
}

impl Desk {
    fn new() -> Self {
        chimera_core::bank2::DOOR_CELL.open_host_if_linked();
        let mut ui = Box::new(screen::stack_ui());
        screen::load_init(&mut ui, EngineType::Drum);
        let mut d = Self {
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
            kits: KitWriter::take().expect("the kit writer, once"),
        };
        d.publish();
        d
    }

    /// The desktop's pass: the project, the UI's own part, the kits.
    fn publish(&mut self) {
        self.shared
            .update_from_project(self.ui.project(), 0, &mut self.knobs);
        self.ui.publish_ui(&mut self.shared);
        self.kits.publish_all(self.ui.project().kits());
    }

    fn note_on(&mut self, note: u8) {
        let ev = NoteEvent {
            channel: self.ui.project().part(P1).mix.channel,
            note: MidiNote::new(note).unwrap(),
            kind: NoteKind::On(Velocity::DEFAULT),
        };
        self.inst.handle(ev, &self.shared);
    }

    /// One block; every pair's energy in it.
    fn block(&mut self) -> f32 {
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
        self.dac.out().iter().flatten().map(|x| x * x).sum::<f32>()
    }

    /// A new Instrument, FX bus and DAC: nothing sounding.
    fn fresh_audio(&mut self) {
        *self.inst = Instrument::new(SAMPLE_RATE, SampleBudget::for_cpu(CPU_HZ_REV_V));
        *self.fx = FxBus::new();
        *self.dac = DacBlocks::new();
    }

    fn lit(&self) -> u16 {
        self.ui.project().pad_lights(P1).lit().bits()
    }
}

/// One test: the kit writer is the process's once, and `KIT_VIEW` is
/// shared by every Instrument in it.
#[test]
fn drum_end_to_end() {
    let mut d = Desk::new();
    drum_init_plays_its_twelve_pads_from_midi(&mut d);
    pads_stay_lit_while_they_sound(&mut d);
    pad_filter_from_the_ui(&mut d);
}

fn drum_init_plays_its_twelve_pads_from_midi(d: &mut Desk) {
    assert_eq!(d.ui.page_def().id, DRUM_KIT.id, "the load lands on KIT");
    assert_eq!(d.ui.selected_pad(P1), PadId::ALL[0]);
    assert_eq!(d.lit(), 0, "nothing lit before a hit");

    // Silence before the first note.
    assert_eq!(d.block(), 0.0, "DRUM INIT is silent unstruck");

    // The limiter looks a block ahead: a hit is heard in the block after
    // the one it lands in, as the next note lands. The UI takes the hits
    // at its own frames, here after pads 1–6 and after 7–12: a frame's
    // hits all light, the last selected.
    let mut onsets = [0.0f32; 12];
    for (n, pad) in PadId::ALL.into_iter().enumerate() {
        d.note_on(n as u8);
        let e = d.block();
        if let Some(prev) = n.checked_sub(1) {
            onsets[prev] = e;
        }
        if n % 6 == 5 {
            d.ui.take_hits(&mut d.kits);
            let want = (1u16 << (n + 1)) - 1;
            assert_eq!(d.lit(), want, "after pad {}: {:012b}", n + 1, d.lit());
            assert_eq!(d.ui.selected_pad(P1), pad, "the frame's last hit");
            d.publish();
        }
    }
    onsets[11] = d.block();
    for (n, e) in onsets.iter().enumerate() {
        assert!(*e > SILENT, "pad {}'s onset block is silent: {e}", n + 1);
    }

    // The rest of 2 s, with the UI's frames: the lights go dark, pad 12
    // stays selected, the kit rings down.
    let mut tail = 0.0;
    for b in 13..BLOCKS {
        tail += d.block();
        if b % 37 == 0 {
            d.ui.update(chimera_core::ui::animation::UiTick::for_test());
            d.publish();
            d.ui.take_hits(&mut d.kits);
        }
    }
    assert!(tail > 0.0, "the kit rings on");
    assert_eq!(d.lit(), 0, "the lights go dark");
    assert_eq!(d.ui.selected_pad(P1), PadId::ALL[11], "pad 12 selected");

    // Each pad alone on a fresh Instrument, the same kits: its onset
    // sounds, so the run above is not one pad ringing under the rest.
    for n in 0..12u8 {
        d.fresh_audio();
        d.note_on(n);
        assert_eq!(d.block(), 0.0, "the lookahead's block");
        let e = d.block();
        assert!(
            e > SILENT,
            "pad {} alone is silent at its onset: {e}",
            n + 1
        );
    }
}

/// Pads 1 and 2 struck in one block both light; each stays lit, frame
/// after frame, while its voice sounds, and goes dark once it ends.
fn pads_stay_lit_while_they_sound(d: &mut Desk) {
    use chimera_core::ui::animation::{FRAME_MS, UiTick};
    let frame_blocks = (SAMPLE_RATE * FRAME_MS / 1000) as usize / BLOCK_SIZE;
    // The last run's hits taken, a frame on and a block of the fresh
    // Instrument's: nothing lit.
    d.fresh_audio();
    d.ui.take_hits(&mut d.kits);
    d.block();
    d.ui.update(UiTick::for_test());
    d.ui.take_hits(&mut d.kits);
    assert_eq!(d.lit(), 0, "dark before");
    d.note_on(0);
    d.note_on(1);
    d.block();
    d.ui.take_hits(&mut d.kits);
    assert_eq!(d.lit(), 0b11, "both struck, both lit");

    let mut lit_frames = [0u32; 2];
    for frame in 0..BLOCKS * 2 / frame_blocks {
        for _ in 0..frame_blocks {
            d.block();
        }
        d.ui.update(UiTick::for_test());
        d.ui.take_hits(&mut d.kits);
        let sounding = d.inst.active().iter().filter(|&&a| a).count();
        let lit = d.lit();
        assert_eq!(
            lit.count_ones() as usize,
            sounding,
            "frame {frame}: lit {lit:02b} with {sounding} voices"
        );
        for (n, f) in lit_frames.iter_mut().enumerate() {
            *f += u32::from(lit >> n & 1);
        }
    }
    let longest = lit_frames.into_iter().max().unwrap();
    assert!(longest >= 10, "a pad lit only {lit_frames:?} frames");
    assert_eq!(d.lit(), 0, "dark once they end");
}

/// Turns `e` a tick at a time while `at` comes nearer `want`: the cell's
/// nearest step.
fn turn_to(d: &mut Desk, e: EncoderId, want: f32, at: impl Fn(&UiState) -> f32) {
    let off = |ui: &UiState| (at(ui) / want).log2().abs();
    for dir in [1i8, -1] {
        for _ in 0..400 {
            let before = off(&d.ui);
            feed(&mut d.ui, Input::turn(e, dir));
            if off(&d.ui) >= before {
                feed(&mut d.ui, Input::turn(e, -dir));
                break;
            }
        }
    }
}

/// On FLT, pad 1 to LADDER at 300 Hz; MIX + PLUS, pad 2's CUTOFF to
/// 4000 Hz. Notes 0 and 1 play those, and the Sound's filter is left.
fn pad_filter_from_the_ui(d: &mut Desk) {
    use chimera_core::dsp::drum::sound::PadFilter;
    use chimera_core::dsp::filter::FilterKind;
    use chimera_core::ui::block_registry::DRUM_FLT;
    d.fresh_audio();
    d.ui.select_pad(P1, PadId::ALL[0]);
    feed(&mut d.ui, Input::press(ButtonId::Plus));
    feed(&mut d.ui, Input::press(ButtonId::Plus));
    assert_eq!(d.ui.page_def().id, DRUM_FLT.id, "on FLT");
    let sound = PadFilter::of(&d.ui.project().part(P1).sound.params.filter);
    let flt = |ui: &UiState, n: usize| *ui.project().kit(P1).pad(PadId::ALL[n]).sound().filter();

    for _ in 0..FilterKind::BUILT.len() {
        if flt(&d.ui, 0).kind() == FilterKind::Ladder {
            break;
        }
        feed(&mut d.ui, Input::turn(EncoderId::A, 1));
    }
    assert_eq!(flt(&d.ui, 0).kind(), FilterKind::Ladder);
    turn_to(d, EncoderId::F, 300.0, |ui| flt(ui, 0).cutoff().get());

    mix(&mut d.ui, ButtonId::Plus);
    assert_eq!(d.ui.selected_pad(P1), PadId::ALL[1]);
    turn_to(d, EncoderId::F, 4000.0, |ui| flt(ui, 1).cutoff().get());
    d.publish();

    let (p1, p2) = (flt(&d.ui, 0), flt(&d.ui, 1));
    // FREQ steps 1/24 octave: the nearest is within half a step.
    for (f, want) in [(p1, 300.0f32), (p2, 4000.0)] {
        let off = (f.cutoff().get() / want).log2().abs();
        assert!(off <= 1.0 / 48.0, "{} for {want}", f.cutoff().get());
    }
    assert_eq!(p2.kind(), FilterKind::Svf, "pad 2 keeps the Sound's SVF");

    d.note_on(0);
    d.note_on(1);
    d.block();
    for (n, want) in [(0, p1), (1, p2)] {
        let v = voice_of(d, PadId::ALL[n]);
        let f = d.inst.voice(v).played_filter();
        assert_eq!(f.kind(), want.kind(), "pad {}", n + 1);
        assert_eq!(f.cutoff, want.cutoff().get(), "pad {}", n + 1);
    }
    assert_eq!(
        PadFilter::of(&d.ui.project().part(P1).sound.params.filter),
        sound,
        "the Sound's filter is left"
    );
}

fn mix(ui: &mut UiState, b: ButtonId) {
    feed(ui, Input::chord(ButtonId::Mix, b));
    feed(ui, Input::release(b));
    feed(ui, Input::release(ButtonId::Mix));
}

/// The active voice sounding pad `p`'s hit.
fn voice_of(d: &Desk, p: PadId) -> usize {
    use chimera_core::dsp::engines::EngineSlot;
    (0..chimera_core::hw::MAX_VOICES)
        .find(|&v| {
            let voice = d.inst.voice(v);
            voice.is_active()
                && matches!(voice.slot(), EngineSlot::Drum(dr) if dr.hit().is_some_and(|h| h.pad() == p))
        })
        .expect("a voice sounds the pad")
}
