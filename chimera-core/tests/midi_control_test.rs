//! MIDI CONFIG › CONTROL (ADR 0254): MIXER's and SEQUENCER's fixed maps,
//! heard only where no Part's own channel takes theirs, and the settings
//! page that sets them. The charts are `docs/guide/MIDI.md`'s.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::Block;
use chimera_core::midi_in::{CtrlEvent, Port};
use chimera_core::nrpn::NrpnAddr;
use chimera_core::orbit::chords::ChordRing;
use chimera_core::orbit::scene::SceneRing;
use chimera_core::params::OutParams;
use chimera_core::part::PartParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::hint;
use chimera_core::ui::midi::control::{SeqAct, mixer_target, seq_target};
use chimera_core::ui::midi_settings::{Control, MidiSettings};
use chimera_core::{MidiChannel, MidiNote};
use chimera_hal::{EncoderId, Ms};
use chimera_midi::ctl::{Ctl, DataValue, FreeCc, Sel};
use chimera_midi::value::U7;
use screen::{Input, feed};

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];
const LEVEL: ParamAddr = ParamAddr::new(BlockRef::Part, PartParams::LEVEL);
const PAN: ParamAddr = ParamAddr::new(BlockRef::Part, PartParams::PAN);
const DELAY_SEND: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::SEND2);

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

struct Rig {
    ui: Box<UiState>,
    now: u32,
}

impl Rig {
    /// MIXER on 14, SEQUENCER on 15, AUTO on 16.
    fn new() -> Self {
        let mut ui = Box::new(screen::stack_ui());
        let mut m = MidiSettings::DEFAULT;
        for (id, ch) in MidiSettings::CONTROL.into_iter().zip([14.0, 15.0, 16.0]) {
            chimera_core::block::Block::write(&mut m, id, ch);
        }
        ui.set_midi(m);
        Rig { ui, now: 1_000 }
    }

    fn pass(&mut self, ev: &[CtrlEvent]) {
        self.now += 4;
        self.ui.hear_ccs(ev.iter().copied(), Ms(self.now), true);
    }

    fn passes(&mut self, ev: &[CtrlEvent], n: usize) {
        self.pass(ev);
        for _ in 1..n {
            self.pass(&[]);
        }
    }

    fn stored(&self, p: PartId, a: ParamAddr) -> f32 {
        self.ui
            .project()
            .read_part(p)
            .block(a.block)
            .unwrap()
            .get(a.param)
    }

    fn ring(&self, r: usize, id: chimera_core::block::ParamId) -> f32 {
        self.ui.project().orbit().live().rings[r].get(id)
    }

    fn key(&self) -> f32 {
        self.ui.project().orbit().live().chords.get(ChordRing::KEY)
    }
}

fn cc(c: u8, n: u8, v: u8) -> CtrlEvent {
    CtrlEvent::Ctl(
        ch(c),
        Ctl::Cc {
            cc: FreeCc::new(n).unwrap(),
            value: U7::new(v).unwrap(),
        },
    )
}

#[test]
fn control_channels_default_off() {
    let m = MidiSettings::DEFAULT;
    for c in Control::ALL {
        assert_eq!(m.channel_of(c), None);
    }
    let mut r = Rig::new();
    r.ui.set_midi(MidiSettings::DEFAULT);
    let was = r.stored(P1, LEVEL);
    r.passes(&[cc(13, 20, 0)], 40);
    assert_eq!(r.stored(P1, LEVEL), was, "OFF: channel 14 is nobody's");
}

#[test]
fn mixer_moves_each_parts_level_pan_and_sends() {
    let mut r = Rig::new();
    r.passes(&[cc(13, 21, 0), cc(13, 27, 0), cc(13, 47, 127)], 60);
    assert_eq!(r.stored(P2, LEVEL), 0.0, "CC 21: Part 2's LEVEL");
    assert_eq!(r.stored(P2, PAN), -1.0, "CC 27: Part 2's PAN");
    assert_eq!(r.stored(P1, DELAY_SEND), 1.0, "CC 47: Part 1's DELAY send");
    assert_eq!(r.stored(P1, LEVEL), 0.8, "Part 1 untouched");
}

#[test]
fn mixer_glides_never_snaps() {
    let mut r = Rig::new();
    r.pass(&[cc(13, 20, 0)]);
    let first = r.stored(P1, LEVEL);
    assert!(first > 0.0, "one pass in, LEVEL is on its way: {first}");
}

#[test]
fn mixer_moves_the_fx_by_cc_and_nrpn() {
    let mut r = Rig::new();
    let mix = ParamAddr::new(
        BlockRef::Reverb,
        chimera_core::dsp::reverb::ReverbParams::MIX,
    );
    let damp = ParamAddr::new(BlockRef::Reverb, chimera_core::block::ParamId(2));
    let n = NrpnAddr::of(damp).unwrap().get();
    let nrpn = CtrlEvent::Ctl(
        ch(13),
        Ctl::Data {
            sel: Sel::Nrpn(n),
            value: DataValue::Coarse(U7::new(127).unwrap()),
        },
    );
    r.passes(&[cc(13, 87, 127), nrpn], 60);
    assert_eq!(r.stored(P1, mix), 1.0, "CC 87: REVERB MIX");
    assert_eq!(
        r.stored(P1, damp),
        damp.spec().unwrap().max,
        "NRPN: REVERB DAMP"
    );
}

#[test]
fn a_parts_own_channel_wins() {
    let mut r = Rig::new();
    r.ui.project_mut().edit_part(P2).mix.channel = ch(13);
    let was = r.stored(P1, LEVEL);
    r.passes(&[cc(13, 20, 0)], 40);
    assert_eq!(r.stored(P1, LEVEL), was, "channel 14 is Part 2's");
}

#[test]
fn sequencer_drives_orbit() {
    let mut r = Rig::new();
    let muted = |r: &Rig| r.ring(0, SceneRing::MUTE) != 0.0;
    let was = muted(&r);
    r.pass(&[cc(14, 21, 127)]);
    assert_ne!(muted(&r), was, "CC 21 up: ring 1 toggles");
    r.pass(&[cc(14, 21, 100)]);
    assert_ne!(muted(&r), was, "still up: no second toggle");
    r.pass(&[cc(14, 21, 0), cc(14, 21, 127)]);
    assert_eq!(muted(&r), was, "down and up: back");
    r.pass(&[cc(14, 26, 0), cc(14, 102, 127)]);
    assert_eq!(r.ring(0, SceneRing::CHANCE), 0.0, "CC 26: ring 1's CHANCE");
    assert_eq!(
        r.ring(0, SceneRing::STEPS),
        31.0,
        "CC 102: ring 1's STEPS, 32"
    );
    r.pass(&[cc(14, 107, 127)]);
    assert_eq!(r.key(), 11.0, "CC 107: KEY, B");
    let d = MidiNote::new(62).unwrap();
    r.pass(&[CtrlEvent::Key(d)]);
    assert_eq!(r.key(), 2.0, "a note: KEY is its pitch class");
}

#[test]
fn sequencer_play_stop_is_b6() {
    use chimera_core::orbit::channel::{Cmd, receive_each};
    let mut r = Rig::new();
    r.pass(&[cc(14, 20, 127), cc(14, 20, 127)]);
    feed(&mut r.ui, Input::turn(EncoderId::A, 0));
    let mut n = 0;
    receive_each(r.ui.orbit().log(), &mut Default::default(), |c| {
        n += usize::from(c == Cmd::TogglePlay)
    });
    assert_eq!(n, 1, "one press, one toggle");
}

#[test]
fn a_key_off_the_sequencer_is_dropped() {
    let mut r = Rig::new();
    r.ui.set_midi(MidiSettings::DEFAULT);
    let was = r.key();
    r.pass(&[CtrlEvent::Key(MidiNote::new(62).unwrap())]);
    assert_eq!(r.key(), was);
    let _ = Port::Din;
}

/// Each chart's rows: `| CC | what |`, MIXER's naming the Part.
fn rows() -> (Vec<String>, Vec<String>) {
    let name = |a: ParamAddr| {
        format!(
            "{} {}",
            a.block.disk_ident().unwrap(),
            a.spec().unwrap().label
        )
    };
    let mut mixer = Vec::new();
    let mut seq = Vec::new();
    for cc in 0..128u8 {
        if let Some((p, n)) = mixer_target(cc) {
            let a = NrpnAddr::new(n).addr().unwrap();
            let who = match a.block {
                BlockRef::Part | BlockRef::Out => format!("Part {}", p.index() + 1),
                _ => "FX".to_string(),
            };
            mixer.push(format!("| {cc} | {who} | {} |", name(a)));
        }
        if let Some(a) = seq_target(cc) {
            let what = match a {
                SeqAct::PlayStop => "PLAY/STOP".to_string(),
                SeqAct::Ring(i) => format!("RING{} ON/OFF", i.get() + 1),
                SeqAct::Set(a) => name(a),
            };
            seq.push(format!("| {cc} | {what} |"));
        }
    }
    (mixer, seq)
}

/// The charts in `docs/guide/MIDI.md` are these.
#[test]
fn the_guide_publishes_the_charts() {
    let guide = include_str!("../../docs/guide/MIDI.md");
    let (mixer, seq) = rows();
    if std::env::var("PRINT_CHARTS").is_ok() {
        println!("{}\n\n{}", mixer.join("\n"), seq.join("\n"));
    }
    for row in mixer.iter().chain(&seq) {
        assert!(guide.contains(row.as_str()), "the guide lacks `{row}`");
    }
}

/// MIDI CONFIG › CONTROL CHANNELS sets each; one a Part's own channel
/// takes reads dimmed, its band says why. Shots: `target/screens/`.
#[test]
fn the_control_page_sets_them_and_marks_a_clash() {
    use chimera_core::ui::settings::leaves::CONTROL;
    let mut ui = Box::new(screen::stack_ui());
    screen::to_leaf(&mut ui, &["MIDI CONFIG", "CONTROL CHANNELS"]);
    assert_eq!(ui.page_def().id, CONTROL.id);
    let shot = screen::render_ui(&ui);
    shot.dump("midi_control_off");
    for col in 0..3 {
        assert!(screen::cell_reads(&shot, col, "OFF"), "cell {col} OFF");
    }
    feed(&mut ui, Input::turn(EncoderId::A, 14));
    feed(&mut ui, Input::turn(EncoderId::B, 15));
    feed(&mut ui, Input::turn(EncoderId::C, 16));
    assert_eq!([0, 1, 2].map(|i| ui.midi().raw(i)), [14, 15, 16]);
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    shot.dump("midi_control_set");
    assert!(screen::cell_reads(&shot, 0, "14"));
    assert!(screen::cell_reads(&shot, 2, "16"));
    assert!(
        !lit(&shot, hint::FOCUS_Y - 10, hint::FOCUS_Y),
        "no clash, no words"
    );
    // AUTO turned down to 1, Part 1's: dimmed, and the band says why.
    feed(&mut ui, Input::turn(EncoderId::C, -15));
    assert_eq!(ui.midi().raw(2), 1);
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    shot.dump("midi_control_clash");
    assert!(screen::cell_reads(&shot, 2, "1"));
    assert!(
        brightest(&shot, 2) < brightest(&shot, 0),
        "AUTO's cell dims"
    );
    assert!(
        lit(&shot, hint::FOCUS_Y - 10, hint::FOCUS_Y),
        "the band says why"
    );
    let note = |ui: &UiState, id| {
        use chimera_core::ui::midi::control::{clash_note, clashes};
        let a = ParamAddr::new(BlockRef::Midi, id);
        clash_note(a, clashes(&ui.midi(), ui.project()))
    };
    assert_eq!(
        note(&ui, MidiSettings::AUTO),
        Some("A PART HAS THIS CHANNEL")
    );
    assert_eq!(note(&ui, MidiSettings::MIXER), None);
    // SEQUENCER on MIXER's 14: MIXER has it, and the band names MIXER.
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert_eq!(
        note(&ui, MidiSettings::SEQUENCER),
        Some("MIXER HAS THIS CHANNEL")
    );
    // AUTO on SEQUENCER's 15 once SEQUENCER holds it: SEQUENCER has it.
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    feed(&mut ui, Input::turn(EncoderId::C, 14));
    assert_eq!(ui.midi().raw(2), 15);
    assert_eq!(
        note(&ui, MidiSettings::AUTO),
        Some("SEQUENCER HAS THIS CHANNEL")
    );
}

/// The brightest red of cell `col`'s value.
fn brightest(shot: &screen::Fb, col: i32) -> u16 {
    use chimera_core::ui::theme::{CELL_COL_W, CELL_LABEL_Y, CELL_VALUE_DY, MARGIN_X};
    let (x, y) = (MARGIN_X + col * CELL_COL_W, CELL_LABEL_Y + CELL_VALUE_DY);
    (y - 16..y)
        .flat_map(|y| (x..x + CELL_COL_W - 4).map(move |x| (x, y)))
        .map(|(x, y)| shot.px[y as usize * 240 + x as usize] >> 11)
        .max()
        .unwrap()
}

/// Any pixel lit between rows `top` and `bottom`.
fn lit(shot: &screen::Fb, top: i32, bottom: i32) -> bool {
    shot.px[top as usize * 240..bottom as usize * 240]
        .iter()
        .any(|&p| p >> 5 & 63 > 20)
}

/// Every chart's CC is one MIDI in passes on: never data entry, its
/// increment and decrement, an RPN or NRPN select, bank select or the pedal.
#[test]
fn every_chart_cc_is_a_free_cc() {
    for cc in 0..128u8 {
        if mixer_target(cc).is_some() || seq_target(cc).is_some() {
            assert!(FreeCc::new(cc).is_some(), "CC {cc} never reaches the UI");
        }
    }
}

/// MIXER's COMP MIX, sent as bytes through DIN's pipe into the UI.
#[test]
fn a_mixer_cc_moves_the_comp_end_to_end() {
    use chimera_core::midi_in::{DinPipe, Sinks};
    use chimera_core::note_queue::InEvent;
    struct Ctrl(Vec<CtrlEvent>);
    impl Sinks for Ctrl {
        fn note(&mut self, _: InEvent) -> bool {
            true
        }
        fn ctrl(&mut self, e: CtrlEvent) -> bool {
            self.0.push(e);
            true
        }
    }
    let mix = ParamAddr::new(BlockRef::Comp, chimera_core::block::ParamId(5));
    let n = NrpnAddr::of(mix).unwrap().get();
    let cc = (0..128u8)
        .find(|&c| mixer_target(c).is_some_and(|(_, t)| t == n))
        .unwrap();
    let mut r = Rig::new();
    let mut out = Ctrl(Vec::new());
    DinPipe::new().feed(&[0xBD, cc, 0], &mut out);
    assert_eq!(out.0.len(), 1, "CC {cc} reached the UI");
    r.passes(&out.0, 60);
    assert_eq!(r.stored(P1, mix), 0.0, "CC {cc}: COMP MIX");
}
