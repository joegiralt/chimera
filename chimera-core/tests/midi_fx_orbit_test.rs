//! FX and ORBIT targets (#512): the master FX's knobs and ORBIT's ring
//! amounts take a binding in the Part's map. Learned on their pages,
//! moved on the Part's channel (an FX knob ramped, a ring amount binned
//! onto the playing scene and its slot), echoed, listed by block, saved.

#![cfg(feature = "midi-map")]

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::Motion;
use chimera_core::dsp::reverb::ReverbParams;
use chimera_core::midi_in::{Cc, CcEvent};
use chimera_core::midi_map::{Binding, Feedback, Input, Mappable, MidiMap, Span};
use chimera_core::orbit::RingIdx;
use chimera_core::orbit::scene::SceneRing;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::EFX;
use chimera_core::ui::learn::Learn;
use chimera_core::ui::nav::OrbitAt;
use chimera_core::ui::orbit::ORBIT_RHYTHM;
use chimera_hal::ButtonId::{B1, Mix, Plus, Seq};
use chimera_hal::EncoderId::{D, F};
use chimera_hal::Ms;
use screen::{Input as Key, feed, tap};

const P1: PartId = PartId::ALL[0];
const R1: BlockRef = BlockRef::OrbitRing(RingIdx::ALL[0]);
const CHANCE: ParamAddr = ParamAddr::new(R1, SceneRing::CHANCE);
const REV_MIX: ParamAddr = ParamAddr::new(BlockRef::Reverb, ReverbParams::MIX);
const MASTER_FX: [BlockRef; 5] = [
    BlockRef::Chorus,
    BlockRef::Delay,
    BlockRef::Reverb,
    BlockRef::Tape,
    BlockRef::Comp,
];

fn ui() -> Box<UiState> {
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, P1);
    ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
    ui
}

fn bind(ui: &mut UiState, n: u8, a: ParamAddr) {
    let m = &mut ui.project_mut().edit_part(P1).sound.map;
    m.learn(Input::cc(n).unwrap(), Mappable::new(a).unwrap())
        .unwrap();
}

fn cc(ui: &mut UiState, n: u8, value: u8, at: u32) {
    let ev = CcEvent {
        channel: ui.project().part(P1).mix.channel,
        cc: Cc::new(n).unwrap(),
        value,
    };
    ui.hear_ccs(core::iter::once(ev), Ms(at), true);
}

/// `a` as a page reads it: Part 1's, the master FX's or ORBIT's live scene.
fn read(ui: &UiState, a: ParamAddr) -> f32 {
    use chimera_core::block::Block;
    match a.block {
        BlockRef::OrbitRing(r) => ui.project().orbit().live().rings[r.get() as usize].get(a.param),
        _ => ui
            .project()
            .read_part(P1)
            .block(a.block)
            .unwrap()
            .get(a.param),
    }
}

/// MIX + SEQ pressed and let go, SEQ first.
fn mix_seq(ui: &mut UiState) {
    feed(ui, Key::chord(Mix, Seq).at(10_000));
    feed(ui, Key::release(Seq).and_held(Mix).at(10_100));
    feed(ui, Key::release(Mix).at(10_133));
}

fn band(ui: &UiState) -> String {
    ui.status_text()
        .map_or_else(String::new, |b| b.as_str().into())
}

/// ORBIT › RING's rhythm page on ring 1, CHANCE focused.
fn on_chance() -> Box<UiState> {
    let mut ui = ui();
    ui.go_for_test(OrbitAt::HOME.into());
    tap(&mut ui, B1);
    assert_eq!(ui.page_def().id, ORBIT_RHYTHM.id);
    feed(&mut ui, Key::turn(D, -1));
    assert_eq!(ui.focused_slot(), 3);
    ui
}

/// The master FX's REV page, MIX focused.
fn on_reverb_mix() -> Box<UiState> {
    let mut ui = ui();
    feed(&mut ui, Key::press(B1).and_held(Mix));
    feed(&mut ui, Key::release(B1));
    for _ in 0..3 {
        feed(&mut ui, Key::press(Plus));
    }
    assert_eq!(ui.page_def().id, EFX.id);
    feed(&mut ui, Key::turn(F, -1));
    assert_eq!(ui.focused_slot(), 5);
    ui
}

#[test]
fn whitelist_takes_fx_knobs_and_ring_amounts() {
    for b in MASTER_FX {
        let mut any = false;
        for s in b.specs() {
            let knob = s.stored && !s.patching && matches!(s.motion(), Motion::Continuous(_));
            any |= knob;
            let a = ParamAddr::new(b, s.id);
            assert_eq!(Mappable::new(a).is_some(), knob, "{b:?} {}", s.label);
        }
        assert!(any, "{b:?} has a knob");
    }
    let amounts = [
        SceneRing::HITS,
        SceneRing::ROTATE,
        SceneRing::CHANCE,
        SceneRing::MUTATE,
        SceneRing::GATE,
        SceneRing::VEL,
        SceneRing::ACCENT,
        SceneRing::OCTAVE,
    ];
    for r in RingIdx::ALL {
        let b = BlockRef::OrbitRing(r);
        for s in b.specs() {
            let a = ParamAddr::new(b, s.id);
            assert_eq!(
                Mappable::new(a).is_some(),
                amounts.contains(&s.id),
                "{b:?} {}",
                s.label
            );
        }
    }
    for b in [BlockRef::OrbitChords, BlockRef::Tempo] {
        for s in b.specs() {
            assert!(Mappable::new(ParamAddr::new(b, s.id)).is_none(), "{b:?}");
        }
    }
}

#[test]
fn a_cc_glides_a_master_fx_knob() {
    let mut ui = ui();
    bind(&mut ui, 20, REV_MIX);
    let spec = REV_MIX.spec().unwrap();
    let from = read(&ui, REV_MIX);
    assert!(from > spec.min);
    cc(&mut ui, 20, 0, 1_000);
    let mut t = 1_000;
    let mut path = vec![];
    for _ in 0..100 {
        t += 4;
        ui.hear_ccs(core::iter::empty::<CcEvent>(), Ms(t), true);
        path.push(read(&ui, REV_MIX));
    }
    assert!(path[0] > spec.min, "lerped, never snapped: {path:?}");
    assert!(path.windows(2).all(|w| w[1] <= w[0]), "{path:?}");
    assert_eq!(*path.last().unwrap(), spec.min);
    // The FX are the Performance's.
    assert_eq!(ui.project().perf().fx.reverb.mix, spec.min);
}

#[test]
fn a_cc_bins_a_ring_amount_on_the_scene_and_its_slot() {
    let mut ui = ui();
    bind(&mut ui, 21, CHANCE);
    assert_eq!(read(&ui, CHANCE), 100.0);
    cc(&mut ui, 21, 0, 1_000);
    assert_eq!(read(&ui, CHANCE), 0.0);
    let song = ui.project().orbit();
    let slot = &song.scene(song.current()).rings[0];
    use chimera_core::block::Block;
    assert_eq!(
        slot.get(SceneRing::CHANCE),
        0.0,
        "the slot keeps it (ADR 0083)"
    );
    // Ring 2 never moved.
    assert_eq!(
        song.live().rings[1].get(SceneRing::CHANCE),
        100.0,
        "only ring 1"
    );
    cc(&mut ui, 21, 64, 1_010);
    let mid = read(&ui, CHANCE);
    assert!((45.0..=55.0).contains(&mid), "{mid}");
}

#[test]
fn learn_on_a_ring_page() {
    let mut ui = on_chance();
    mix_seq(&mut ui);
    assert!(matches!(ui.learn(), Learn::Armed { part: P1, .. }));
    assert_eq!(band(&ui), "MOVE A KNOB · CHANCE");
    let was = read(&ui, CHANCE);
    cc(&mut ui, 22, 0, 20_000);
    let map = ui.project().part(P1).sound.map;
    assert_eq!(map.len(), 1);
    assert_eq!(map.entries()[0].target, Mappable::new(CHANCE).unwrap());
    assert_eq!(read(&ui, CHANCE), was, "learn never applies");
    // MIX + SEQ ends it; the next CC moves ring 1's CHANCE.
    mix_seq(&mut ui);
    assert_eq!(ui.learn(), Learn::Off);
    cc(&mut ui, 22, 0, 30_000);
    assert_eq!(read(&ui, CHANCE), 0.0);
}

#[test]
fn an_orbit_key_cancels_learn() {
    let mut ui = on_chance();
    mix_seq(&mut ui);
    assert!(matches!(ui.learn(), Learn::Armed { .. }));
    feed(&mut ui, Key::press(Plus));
    assert_eq!(ui.learn(), Learn::Off);
}

#[test]
fn learn_on_the_reverb_page() {
    let mut ui = on_reverb_mix();
    mix_seq(&mut ui);
    assert!(matches!(ui.learn(), Learn::Armed { .. }));
    cc(&mut ui, 23, 5, 20_000);
    let map = ui.project().part(P1).sound.map;
    assert_eq!(map.entries()[0].target, Mappable::new(REV_MIX).unwrap());
}

#[test]
fn the_hand_beats_a_cc_on_orbit() {
    let mut ui = on_chance();
    bind(&mut ui, 24, CHANCE);
    // A turn of CHANCE on its page and a CC in the same pass: the hand's.
    feed(&mut ui, Key::turn(D, -3));
    let turned = read(&ui, CHANCE);
    cc(&mut ui, 24, 127, 40_000);
    assert_eq!(read(&ui, CHANCE), turned);
    cc(&mut ui, 24, 127, 40_010);
    assert_eq!(read(&ui, CHANCE), 100.0, "the next pass hears it");
}

#[test]
fn midi_map_names_the_block() {
    let mut ui = ui();
    bind(&mut ui, 20, REV_MIX);
    bind(&mut ui, 21, CHANCE);
    let t = |ui: &UiState, i: usize| ui.map_band().text(i).label.as_str().to_string();
    // Row 0 is the Part's controller map (ADR 0226); the Sound's rows follow.
    assert_eq!(t(&ui, 1), "CC 20  REVERB MIX");
    assert_eq!(t(&ui, 2), "CC 21  RING 1 CHANCE");
}

#[test]
fn fx_and_orbit_bindings_save_and_load() {
    use chimera_core::midi_map::codec::{self, MapRecord};
    let mut m = MidiMap::EMPTY;
    for (n, a) in [(20, REV_MIX), (21, CHANCE)] {
        let t = Mappable::new(a).unwrap();
        m.learn(Input::cc(n).unwrap(), t).unwrap();
    }
    m.set(
        1,
        Binding {
            span: Span { min: 10, max: 200 },
            feedback: Feedback::Value,
            ..m.entries()[1]
        },
    );
    let mut r = chimera_core::storage::RecordBuf::new();
    codec::encode(&m, &mut r);
    let mut back = MidiMap::EMPTY;
    let lost = codec::decode(MapRecord::Bindings, r.as_slice(), Some(&mut back)).unwrap();
    assert_eq!(lost, 0);
    assert_eq!(back, m);
}

#[test]
fn a_ring_binding_echoes() {
    use chimera_core::out_ring::{OUT_RING_LEN, OutRing};
    let mut ui = ui();
    let ch = ui.project().part(P1).mix.channel;
    let m = &mut ui.project_mut().edit_part(P1).sound.map;
    m.learn(Input::cc(25).unwrap(), Mappable::new(CHANCE).unwrap())
        .unwrap();
    let e = m.entries()[0];
    m.set(
        0,
        Binding {
            feedback: Feedback::Value,
            ..e
        },
    );
    let ring = OutRing::new();
    let mut out = vec![];
    let mut b = [0u8; OUT_RING_LEN];
    for k in 0..100 {
        ui.hear_ccs(core::iter::empty::<CcEvent>(), Ms(1_000 + 4 * k), true);
        ui.send_out(&ring);
        let n = ring.pop_into(&mut b);
        out.extend_from_slice(&b[..n]);
    }
    // CHANCE 100 of 0..=100: the top bin's centre, sent as 127.
    assert!(
        out.windows(3).any(|w| w == [0xB0 | ch.get(), 25, 127]),
        "{out:?}"
    );
}

/// Ring 1 at `n` STEPS.
fn steps(ui: &mut UiState, n: u8) {
    use chimera_core::block::Block;
    ui.project_mut().orbit_mut().edit(&mut |s| {
        s.rings[0].write(SceneRing::STEPS, f32::from(n - 1));
    });
}

/// `a` after each value of an absolute CC 30 sweep over `vals`.
fn sweep(ui: &mut UiState, a: ParamAddr, vals: impl Iterator<Item = u8>) -> Vec<f32> {
    let mut t = 50_000;
    vals.map(|v| {
        t += 4;
        cc(ui, 30, v, t);
        read(ui, a)
    })
    .collect()
}

/// A full sweep up covers `0..=top` once, in order, every value given a
/// fair share of the travel; down comes back the same way.
fn covers_once(ui: &mut UiState, a: ParamAddr, top: f32) {
    bind(ui, 30, a);
    let up = sweep(ui, a, 0..=127);
    assert_eq!(up[0], 0.0, "{a:?} {up:?}");
    assert_eq!(*up.last().unwrap(), top, "{a:?} {up:?}");
    assert!(up.windows(2).all(|w| w[1] >= w[0]), "monotonic: {up:?}");
    let n = top as usize + 1;
    for k in 0..n {
        let share = up.iter().filter(|&&x| x == k as f32).count();
        // Each of the ring's values holds about 128 / n of the travel.
        assert!(share * n >= 128 / 2, "{a:?} {k} holds {share}: {up:?}");
    }
    let down = sweep(ui, a, (0..=127).rev());
    assert!(down.windows(2).all(|w| w[1] <= w[0]), "monotonic: {down:?}");
    assert_eq!(*down.last().unwrap(), 0.0);
}

#[test]
fn hits_cover_the_ring_once() {
    let mut ui = ui();
    steps(&mut ui, 8);
    covers_once(&mut ui, ParamAddr::new(R1, SceneRing::HITS), 8.0);
}

#[test]
fn rotate_covers_the_ring_once() {
    let mut ui = ui();
    steps(&mut ui, 8);
    covers_once(&mut ui, ParamAddr::new(R1, SceneRing::ROTATE), 7.0);
}

#[test]
fn octave_covers_its_range_once() {
    let mut ui = ui();
    covers_once(&mut ui, ParamAddr::new(R1, SceneRing::OCTAVE), 8.0);
}

/// A fader resting on a bin's edge, jittering a count either way, moves
/// the ring at most once.
#[test]
fn no_chatter_at_a_bin_edge() {
    let mut ui = ui();
    steps(&mut ui, 8);
    let a = ParamAddr::new(R1, SceneRing::ROTATE);
    bind(&mut ui, 30, a);
    // ROTATE's 1|2 edge for 8 bins: 128 * 2 / 8 = 32.
    sweep(&mut ui, a, 0..=31);
    let jitter = sweep(&mut ui, a, [31, 32, 31, 32, 33, 32, 31, 32].into_iter());
    let moves = jitter.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(moves <= 1, "{jitter:?}");
}

/// Two Parts on their own channels bind the same master FX knob: the
/// newer message wins, and the older glide stops, never alternating.
#[test]
fn two_parts_on_one_fx_knob_last_wins() {
    const P2: PartId = PartId::ALL[1];
    let mut ui = ui();
    screen::init_part(&mut ui, P2);
    ui.project_mut().edit_part(P2).sound.map = MidiMap::EMPTY;
    ui.project_mut().edit_part(P2).mix.channel = chimera_core::MidiChannel::new(1).unwrap();
    ui.project_mut().edit_part(P1).mix.channel = chimera_core::MidiChannel::new(0).unwrap();
    bind(&mut ui, 20, REV_MIX);
    let m = &mut ui.project_mut().edit_part(P2).sound.map;
    m.learn(Input::cc(20).unwrap(), Mappable::new(REV_MIX).unwrap())
        .unwrap();
    let spec = REV_MIX.spec().unwrap();
    let on = |ch: u8, v: u8, t: u32, ui: &mut UiState| {
        let ev = CcEvent {
            channel: chimera_core::MidiChannel::new(ch).unwrap(),
            cc: Cc::new(20).unwrap(),
            value: v,
        };
        ui.hear_ccs(core::iter::once(ev), Ms(t), true);
    };
    // Part 1 heads down slowly (its messages 100 ms apart: a 100 ms
    // glide); mid-glide Part 2 heads up.
    on(0, 64, 1_000, &mut ui);
    on(0, 0, 1_100, &mut ui);
    for k in 1..=4 {
        ui.hear_ccs(core::iter::empty::<CcEvent>(), Ms(1_100 + 4 * k), true);
    }
    on(1, 127, 1_120, &mut ui);
    let mut t = 1_120;
    let mut path = vec![read(&ui, REV_MIX)];
    for _ in 0..100 {
        t += 4;
        ui.hear_ccs(core::iter::empty::<CcEvent>(), Ms(t), true);
        path.push(read(&ui, REV_MIX));
    }
    assert!(
        path.windows(2).all(|w| w[1] >= w[0]),
        "no alternation: {path:?}"
    );
    assert_eq!(*path.last().unwrap(), spec.max, "the newer wins");
}

/// STEPS shrunk under a fader resting on the old top bin: the next sweep
/// stays inside the shrunk ring and covers it once.
#[test]
fn steps_shrunk_under_a_held_bin() {
    let mut ui = ui();
    steps(&mut ui, 8);
    let a = ParamAddr::new(R1, SceneRing::ROTATE);
    bind(&mut ui, 30, a);
    assert_eq!(*sweep(&mut ui, a, 0..=127).last().unwrap(), 7.0);
    steps(&mut ui, 4);
    assert!(read(&ui, a) <= 3.0);
    let down = sweep(&mut ui, a, (0..=126).rev());
    assert!(down.iter().all(|&x| x <= 3.0), "inside the ring: {down:?}");
    assert!(down.windows(2).all(|w| w[1] <= w[0]), "monotonic: {down:?}");
    assert_eq!(*down.last().unwrap(), 0.0);
    let up = sweep(&mut ui, a, 0..=127);
    assert!(up.windows(2).all(|w| w[1] >= w[0]), "monotonic: {up:?}");
    assert_eq!(*up.last().unwrap(), 3.0);
    for k in 0..4 {
        let share = up.iter().filter(|&&x| x == k as f32).count();
        assert!(share * 4 >= 64, "{k} holds {share}: {up:?}");
    }
}
