//! MIDI learn (MIDI map spec § 4): MIX+SEQ arms on the focused cell, a
//! turn moves the focus only, and the first CC on the active Part's
//! channel maps to it, its value never applied. Every refusal says why in
//! the band; a nav key cancels.

#![cfg(feature = "midi-map")]

mod screen;

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::midi_in::{Cc, CcEvent};
use chimera_core::midi_map::{MAP_LEN, Mappable};
use chimera_core::params::FilterParams;
use chimera_core::project::{PartId, PartStatus, part_status};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::{FILTER, FILTER_JACKS};
use chimera_core::ui::learn::{Learn, LearnNote};
use chimera_core::ui::nav::PartChain;
use chimera_hal::ButtonId::{B2, Edit, Menu, Mix, Plus, Seq};
use chimera_hal::EncoderId::{C, F};
use chimera_hal::Ms;
use screen::{Input, feed, tap};

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const RESO: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);

/// Part 1 on INIT Algo with the 2040 (its FREQ reads CUTOFF), on FLT,
/// CUTOFF focused.
fn on_cutoff() -> Box<UiState> {
    let mut ui = Box::new(crate::screen::stack_ui());
    let was = PartChain::of(&ui.project().part(P1).sound);
    screen::init_part(&mut ui, P1);
    let sound = ui.project_mut().edit_part(P1).sound;
    sound.params.filter.set_kind(FilterKind::K2040);
    // Learning starts from an empty map, not INIT's default one.
    sound.map = chimera_core::midi_map::MidiMap::EMPTY;
    ui.replaced_for_test(was);
    for _ in 0..12 {
        if ui.page_def().id == FILTER.id {
            break;
        }
        feed(&mut ui, Input::press(Plus));
    }
    assert_eq!(ui.page_def().id, FILTER.id);
    feed(&mut ui, Input::turn(F, 1));
    assert_eq!(ui.focused_slot(), 5);
    ui
}

/// MIX + SEQ pressed and let go, SEQ first.
fn mix_seq(ui: &mut UiState) {
    feed(ui, Input::chord(Mix, Seq).at(10_000));
    feed(ui, Input::release(Seq).and_held(Mix).at(10_100));
    feed(ui, Input::release(Mix).at(10_133));
}

fn band(ui: &UiState) -> String {
    ui.status_text()
        .map_or_else(String::new, |b| b.as_str().into())
}

fn ch(n: u8) -> MidiChannel {
    MidiChannel::new(n).unwrap()
}

fn cc(ui: &mut UiState, channel: MidiChannel, n: u8, value: u8, at: u32) {
    let ev = CcEvent {
        channel,
        cc: Cc::new(n).unwrap(),
        value,
    };
    ui.hear_ccs(core::iter::once(ev), Ms(at), true);
}

fn stored(ui: &UiState, a: ParamAddr) -> f32 {
    ui.project()
        .read_part(P1)
        .block(a.block)
        .unwrap()
        .get(a.param)
}

fn armed(ui: &UiState) -> bool {
    matches!(ui.learn(), Learn::Armed { .. })
}

#[test]
fn mix_seq_arms_on_focus() {
    let mut ui = on_cutoff();
    let at = ui.location();
    // Held past a hold: no SEQ hold either (that's ORBIT).
    feed(&mut ui, Input::chord(Mix, Seq).at(10_000));
    feed(&mut ui, Input::held(Seq).and_held(Mix).at(10_600));
    assert_eq!(band(&ui), "MOVE A KNOB · CUTOFF");
    feed(&mut ui, Input::release(Seq).and_held(Mix).at(10_633));
    feed(&mut ui, Input::release(Mix).at(10_666));
    assert_eq!(ui.location(), at, "no SEQ tap or hold");
    assert_eq!(
        ui.learn(),
        Learn::Armed {
            part: P1,
            note: None
        }
    );
    assert_eq!(band(&ui), "MOVE A KNOB · CUTOFF");
}

#[test]
fn armed_turn_moves_focus_only() {
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    let before = format!("{:?}", ui.project().part(P1).sound.params);
    feed(&mut ui, Input::turn(C, 5));
    assert_eq!(ui.focused_slot(), 2);
    assert_eq!(
        format!("{:?}", ui.project().part(P1).sound.params),
        before,
        "no value moved"
    );
    assert!(armed(&ui));
    assert_eq!(band(&ui), "MOVE A KNOB · RES");
}

#[test]
fn learn_maps_first_cc() {
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    let was = stored(&ui, CUTOFF);
    let p1 = ui.project().part(P1).mix.channel;
    cc(&mut ui, p1, 74, 100, 20_000);
    let map = ui.project().part(P1).sound.map;
    assert_eq!(map.len(), 1);
    assert_eq!(
        map.entries()[0].input,
        chimera_core::midi_map::Input::cc(74).unwrap()
    );
    assert_eq!(map.entries()[0].target, Mappable::new(CUTOFF).unwrap());
    let (top, words, _) = ui.learn_text().unwrap();
    assert_eq!(top.as_str(), "CC 74 → CUTOFF");
    assert_eq!(words.as_str(), "TURN IT RIGHT, THEN LEFT");
    assert_eq!(band(&ui), "TURN IT RIGHT, THEN LEFT");
    // Learn stays open, sensing the control's kind (phase 1 § 6.3).
    assert_eq!(ui.learn(), Learn::Sensing { part: P1, entry: 0 });
    // FLT is BigViz: the words ride the viz band's top line.
    let fb = screen::render_ui(&ui);
    let top = chimera_core::ui::theme::BIGVIZ_STATUS_Y;
    let ink = (0..240)
        .flat_map(|x| (top - 8..=top).map(move |y| (x, y)))
        .filter(|&(x, y)| fb.at(x, y) == chimera_core::ui::theme::INK)
        .count();
    assert!(ink > 0);
    // Passes on, nothing moves: the learned value was never applied.
    for t in 1..20 {
        cc(&mut ui, ch(15), 1, 0, 20_000 + 4 * t);
    }
    assert_eq!(stored(&ui, CUTOFF), was);
    let p = ui.project();
    assert_eq!(part_status(p.part(P1), p.pool()), PartStatus::Edited, "*");
    // MIX + SEQ ends sensing; the next CC moves it.
    mix_seq(&mut ui);
    assert_eq!(ui.learn(), Learn::Off);
    cc(&mut ui, p1, 74, 0, 20_200);
    for t in 1..40 {
        cc(&mut ui, ch(15), 1, 0, 20_200 + 4 * t);
    }
    assert_ne!(stored(&ui, CUTOFF), was);
}

/// Sensing trusts its entry only while it is still CUTOFF's: a map
/// replaced under it (not by learn) ends learn (#820).
#[test]
fn a_replaced_map_ends_sensing() {
    use chimera_core::midi_map::{Input as MapInput, MidiMap};
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    let p1 = ui.project().part(P1).mix.channel;
    cc(&mut ui, p1, 74, 100, 20_000);
    assert_eq!(ui.learn(), Learn::Sensing { part: P1, entry: 0 });
    let mut m = MidiMap::EMPTY;
    m.learn(MapInput::cc(74).unwrap(), Mappable::new(RESO).unwrap())
        .unwrap();
    ui.project_mut().edit_part(P1).sound.map = m;
    cc(&mut ui, p1, 74, 90, 20_004);
    assert_eq!(ui.learn(), Learn::Off);
}

/// `n` mappable targets other than CUTOFF.
fn others(n: usize) -> Vec<Mappable> {
    Op::ALL
        .into_iter()
        .flat_map(|op| (0..32).map(move |p| ParamAddr::new(BlockRef::AlgoOp(op), ParamId(p))))
        .filter(|&a| a != CUTOFF)
        .filter_map(Mappable::new)
        .take(n)
        .collect()
}

#[test]
fn learn_refusals() {
    // Another channel: said, still armed.
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    assert_eq!(ui.project().part(P1).mix.channel, ch(0));
    cc(&mut ui, ch(1), 3, 64, 20_000);
    assert_eq!(band(&ui), "CC 3 ON CH 2, PART ON CH 1");
    assert!(armed(&ui));
    assert!(ui.project().part(P1).sound.map.is_empty());

    // Reserved: said, still armed.
    cc(&mut ui, ch(0), 120, 0, 20_004);
    assert_eq!(band(&ui), "CC 120 RESERVED");
    assert_eq!(
        ui.learn(),
        Learn::Armed {
            part: P1,
            note: Some(LearnNote::Reserved(Cc::new(120).unwrap()))
        }
    );
    assert!(ui.project().part(P1).sound.map.is_empty());
    // Reserved on another channel: the channel is said, as any CC's (#820).
    cc(&mut ui, ch(1), 121, 0, 20_008);
    assert_eq!(band(&ui), "CC 121 ON CH 2, PART ON CH 1");

    // FM3 IN patches (FLT › JACK, ADR 0190): not mappable, and a CC
    // changes nothing.
    let mut ui = on_cutoff();
    for _ in 0..3 {
        feed(&mut ui, Input::press(Edit));
    }
    assert_eq!(ui.page_def().id, FILTER_JACKS.id);
    mix_seq(&mut ui);
    feed(&mut ui, Input::turn(C, 1)); // FM3 IN, armed: focus only
    assert_eq!(ui.page_def().id, FILTER_JACKS.id, "no SEQ tap");
    assert_eq!(band(&ui), "FREQ SRC NOT MAPPABLE");
    let before = format!("{:?}", ui.project().part(P1).sound.params);
    cc(&mut ui, ch(0), 74, 100, 20_000);
    assert_eq!(band(&ui), "FREQ SRC NOT MAPPABLE");
    assert!(armed(&ui));
    assert!(ui.project().part(P1).sound.map.is_empty());
    assert_eq!(format!("{:?}", ui.project().part(P1).sound.params), before);

    // A full map, and a target not in it.
    let mut ui = on_cutoff();
    for (i, t) in others(MAP_LEN).into_iter().enumerate() {
        let s = ui.project_mut().edit_part(P1).sound;
        s.map
            .learn(chimera_core::midi_map::Input::cc(i as u8 + 40).unwrap(), t)
            .unwrap();
    }
    mix_seq(&mut ui);
    cc(&mut ui, ch(0), 74, 100, 20_000);
    assert_eq!(band(&ui), "MAP FULL");
    assert_eq!(ui.learn(), Learn::Off);
    assert_eq!(ui.project().part(P1).sound.map.len(), MAP_LEN);
}

/// Each nav key cancels; a later CC is heard as mapped, not learned.
#[test]
fn learn_cancels_on_nav() {
    let cancels: [fn(&mut UiState); 4] = [
        |ui| tap(ui, B2),
        |ui| tap(ui, Menu),
        |ui| feed(ui, Input::press(Plus)),
        mix_seq,
    ];
    for (i, cancel) in cancels.into_iter().enumerate() {
        let mut ui = on_cutoff();
        let s = ui.project_mut().edit_part(P1).sound;
        s.map
            .learn(
                chimera_core::midi_map::Input::cc(74).unwrap(),
                Mappable::new(RESO).unwrap(),
            )
            .unwrap();
        let map = s.map;
        mix_seq(&mut ui);
        assert!(armed(&ui), "{i}");
        cancel(&mut ui);
        assert_eq!(ui.learn(), Learn::Off, "{i}");
        let was = stored(&ui, RESO);
        cc(&mut ui, ch(0), 74, 127, 20_000);
        for t in 1..40 {
            cc(&mut ui, ch(15), 1, 0, 20_000 + 4 * t);
        }
        assert_eq!(ui.project().part(P1).sound.map, map, "{i}: not learned");
        assert_ne!(stored(&ui, RESO), was, "{i}: applied");
    }
}

/// MENU held (quick save) is a key like the others: it cancels.
#[test]
fn menu_hold_cancels_learn() {
    let mut ui = on_cutoff();
    mix_seq(&mut ui);
    assert!(armed(&ui));
    feed(&mut ui, Input::press(Menu).at(20_000));
    feed(&mut ui, Input::held(Menu).at(21_000));
    assert!(ui.job_queued_for_test(), "the quick save asked");
    assert_eq!(ui.learn(), Learn::Off);
}

/// ORBIT's pages learn as a Part's do (#512): MIX + SEQ arms on the
/// focused cell, no SEQ tap leaves and no scene is saved.
#[cfg(feature = "orbit")]
#[test]
fn learn_in_orbit() {
    let mut ui = on_cutoff();
    screen::hold(&mut ui, Seq);
    let at = ui.location();
    assert!(at.orbit().is_some());
    let rev = ui.project().rev();
    mix_seq(&mut ui);
    assert_eq!(ui.location(), at);
    assert!(armed(&ui));
    assert_eq!(ui.project().rev(), rev, "no scene saved");
    mix_seq(&mut ui);
    assert_eq!(ui.learn(), Learn::Off);
}

/// Every word learn says fits the band at its status face, on every cell
/// of every chain, with the widest CC and channels.
#[test]
fn learn_words_fit_the_band() {
    use chimera_core::params::ParamSnapshot;
    use chimera_core::ui::block_registry::ALL_CHAINS;
    use chimera_core::ui::components::status_font;
    use chimera_core::ui::draw::text_width;
    use chimera_core::ui::fmt::FmtBuf;
    use chimera_core::ui::learn::armed_text;
    use chimera_core::ui::theme::{CONTENT_W, LABEL_TRACKING};
    use chimera_core::ui::view::{SlotCtx, view};
    let fits = |b: &FmtBuf| {
        let w = text_width(status_font(b.as_str()), b.as_str(), LABEL_TRACKING);
        assert!(w <= CONTENT_W, "{}: {w}", b.as_str());
    };
    let cc127 = Cc::new(127).unwrap();
    let ctx = SlotCtx::read(&ParamSnapshot::default(), Op::A);
    let notes = [
        LearnNote::NotMappable,
        LearnNote::WrongChannel {
            cc: cc127,
            got: ch(15),
            part: ch(15),
        },
        LearnNote::Reserved(cc127),
        LearnNote::Full,
    ];
    // Sensing's words: the widest control of each kind, each guess, sure
    // or not; the hint line with MIX + SEQ  DONE after a sure guess.
    use chimera_core::midi_map::{ButtonMode, Cc14Msb, CcKind, Input as In, LearnCc, NrpnKind};
    use chimera_core::ui::learn::{Line, Sensed};
    use chimera_midi::rel::ByteEncoding;
    use chimera_midi::take::Takeover::Jump;
    use chimera_midi::value::U14;
    let c = LearnCc::new(119).unwrap();
    let n = U14::new(16383).unwrap();
    let mut inputs = vec![
        In::Cc(c, CcKind::Absolute(Jump)),
        In::Cc(c, CcKind::Button(ButtonMode::Momentary)),
        In::Cc14(Cc14Msb::new(31).unwrap(), Jump),
        In::Nrpn(n, NrpnKind::IncDec),
        In::Nrpn(n, NrpnKind::Absolute(Jump)),
        In::Pressure(Jump),
    ];
    for e in ByteEncoding::ALL {
        inputs.push(In::Cc(c, CcKind::Relative(e)));
        inputs.push(In::Nrpn(n, NrpnKind::Relative(e)));
    }
    let sensed: Vec<Sensed> = inputs
        .iter()
        .flat_map(|&input| [false, true].map(|sure| Sensed::new(input, sure)))
        .collect();
    for &s in &sensed {
        assert_eq!(Sensed::new(s.input().unwrap(), s.sure()), s, "packs");
        let mut hint = FmtBuf::new();
        chimera_core::ui::midi::learn::words(s, "", Line::Words, &mut hint);
        // The words in the value's place, at the status face.
        fits(&hint);
    }
    {
        use chimera_core::ui::hint::{Gesture, width};
        assert!(width(&[Gesture("MIX + SEQ", "DONE")]) <= CONTENT_W);
    }
    for chain in ALL_CHAINS {
        for b in chain.blocks {
            for def in core::iter::once(b.def).chain(b.sub_pages.iter().copied()) {
                for i in 0..def.params.len() {
                    let label = view(def, i, &ctx).label();
                    let mut t = FmtBuf::new();
                    armed_text(label, &mut t);
                    fits(&t);
                    for n in notes
                        .iter()
                        .copied()
                        .chain(sensed.iter().map(|&s| LearnNote::Sensing(s)))
                    {
                        let mut t = FmtBuf::new();
                        n.text(label, &mut t);
                        fits(&t);
                    }
                    // Sensing's label line, at the label's face.
                    for &s in &sensed {
                        let mut top = FmtBuf::new();
                        chimera_core::ui::midi::learn::words(s, label, Line::Label, &mut top);
                        let w = text_width(
                            &chimera_core::ui::theme::FONT_VALUE,
                            top.as_str(),
                            LABEL_TRACKING,
                        );
                        assert!(w <= CONTENT_W, "{}: {w}", top.as_str());
                    }
                }
            }
        }
    }
}

/// An MPE member channel's controls are its note's (ADR 0252): learn
/// never hears one (not even to say it came on the wrong channel), and
/// the master's still learns.
#[test]
fn learn_skips_an_mpe_member_channel() {
    use chimera_core::mpe::Zones;
    use chimera_core::ui::midi_settings::MidiSettings;
    let mut ui = on_cutoff();
    ui.set_midi(MidiSettings {
        mpe: Zones::OFF.with_lower(15),
        ..MidiSettings::DEFAULT
    });
    mix_seq(&mut ui);
    assert!(armed(&ui));
    cc(&mut ui, ch(2), 74, 100, 20_000);
    assert_eq!(
        ui.learn(),
        Learn::Armed {
            part: P1,
            note: None
        },
        "still waiting, no wrong-channel note: a finger's slide is no knob"
    );
    assert_eq!(ui.project().part(P1).sound.map.len(), 0);
    cc(&mut ui, ch(0), 74, 100, 20_004);
    assert_eq!(ui.project().part(P1).sound.map.len(), 1);
}
