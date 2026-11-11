//! The built-in layer (ADR 0263): GM's and RP-021's CCs on each Part's own
//! channel, under the Part's map, gliding through the canonical path.
//! The chart is `docs/guide/MIDI.md`'s.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Blocks, Op, ParamAddr};
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::dsp::modal::ModalParams;
use chimera_core::dsp::modulator::EnvSlot;
use chimera_core::midi_in::CtrlEvent;
use chimera_core::midi_map::builtin::{BUILT_IN, Builtin, builtin};
use chimera_core::midi_map::{Input, Mappable};
use chimera_core::params::{EngineType, EnvParams, FilterParams, OutParams};
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, project_crc};
use chimera_core::ui::UiState;
use chimera_core::ui::midi::control::mixer_target;
use chimera_core::ui::midi_settings::MidiSettings;
use chimera_core::ui::nav::PartChain;
use chimera_core::{MidiChannel, mpe::Zones};
use chimera_hal::{EncoderId, Ms};
use chimera_midi::ctl::{Ctl, FreeCc};
use chimera_midi::value::U7;
use screen::{Input as Key, feed};

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];
const LEVEL: ParamAddr = ParamAddr::new(BlockRef::Part, PartParams::LEVEL);
const PAN: ParamAddr = ParamAddr::new(BlockRef::Part, PartParams::PAN);
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const RESO: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
const ATTACK: ParamAddr = ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::ATTACK);
const RELEASE: ParamAddr = ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::RELEASE);
const DAMP: ParamAddr = ParamAddr::new(BlockRef::Modal, ModalParams::DAMP);
const CHORUS: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::SEND1);
const DELAY: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::SEND2);
const REVERB: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::SEND3);

fn op(i: u8, p: chimera_core::block::ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::AlgoOp(Op::ALL[usize::from(i)]), p)
}

struct Rig {
    ui: Box<UiState>,
    now: u32,
}

impl Rig {
    /// Part 1 playing `e`'s plain INIT, its map empty.
    fn new(e: EngineType) -> Self {
        let mut ui = Box::new(screen::stack_ui());
        screen::init_part(&mut ui, P1);
        let was = PartChain::of(&ui.project().part(P1).sound);
        **ui.project_mut().edit_part(P1).sound = Sound::plain_init(e);
        ui.replaced_for_test(was);
        Rig { ui, now: 1_000 }
    }

    fn va() -> Self {
        Self::new(EngineType::Va)
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

    fn set(&mut self, a: ParamAddr, v: f32) {
        let s = self.ui.project_mut().edit_part(P1).sound;
        s.params.block_mut(a.block).unwrap().set(a.param, v);
    }

    fn ch(&self, p: PartId) -> u8 {
        self.ui.project().part(p).mix.channel.get()
    }

    fn cc(&self, p: PartId, n: u8, v: u8) -> CtrlEvent {
        cc(self.ch(p), n, v)
    }
}

fn cc(c: u8, n: u8, v: u8) -> CtrlEvent {
    CtrlEvent::Ctl(
        MidiChannel::new(c).unwrap(),
        Ctl::Cc {
            cc: FreeCc::new(n).unwrap(),
            value: U7::new(v).unwrap(),
        },
    )
}

#[test]
fn each_cc_moves_its_cell_on_the_parts_channel() {
    let mut r = Rig::va();
    let ev = [
        r.cc(P1, 7, 0),
        r.cc(P1, 10, 0),
        r.cc(P1, 71, 127),
        r.cc(P1, 74, 0),
        r.cc(P1, 91, 127),
        r.cc(P1, 93, 127),
        r.cc(P1, 94, 127),
    ];
    let p2 = r.stored(P2, LEVEL);
    r.passes(&ev, 120);
    assert_eq!(r.stored(P1, LEVEL), 0.0, "CC 7: LEVEL");
    assert_eq!(r.stored(P1, PAN), -1.0, "CC 10: PAN");
    let top = |a: ParamAddr| a.spec().unwrap().max;
    let bottom = |a: ParamAddr| a.spec().unwrap().min;
    assert_eq!(r.stored(P1, RESO), top(RESO), "CC 71: RESONANCE");
    assert_eq!(r.stored(P1, CUTOFF), bottom(CUTOFF), "CC 74: CUTOFF");
    assert_eq!(r.stored(P1, REVERB), 1.0, "CC 91: REVERB send");
    assert_eq!(r.stored(P1, CHORUS), 1.0, "CC 93: CHORUS send");
    assert_eq!(r.stored(P1, DELAY), 1.0, "CC 94: DELAY send");
    assert_eq!(
        r.stored(P2, LEVEL),
        p2,
        "Part 2, on its own channel, untouched"
    );
}

/// Two Parts' CC 7 in one pass each land: a Part's LEVEL is its own, not
/// a shared target the last message claims.
#[test]
fn two_parts_levels_move_together() {
    let mut r = Rig::va();
    let ev = [r.cc(P1, 7, 0), r.cc(P2, 7, 127)];
    r.passes(&ev, 120);
    assert_eq!(r.stored(P1, LEVEL), 0.0);
    assert_eq!(r.stored(P2, LEVEL), 1.0);
}

#[test]
fn a_cc_glides_never_snaps() {
    let mut r = Rig::va();
    let ev = [r.cc(P1, 7, 0)];
    r.pass(&ev);
    let first = r.stored(P1, LEVEL);
    assert!(first > 0.0, "one pass in, LEVEL is on its way: {first}");
}

/// VA and TAPE: ENV 1, their VCA's (`Sound::plain_init`), shifted from
/// the Sound's own (RP-021): 64 gives the Sound back.
#[test]
fn attack_and_release_shift_env_1_on_va_and_tape() {
    for e in [EngineType::Va, EngineType::Tape] {
        let mut r = Rig::new(e);
        r.set(ATTACK, 0.2);
        r.set(RELEASE, 0.5);
        let ev = [r.cc(P1, 73, 96), r.cc(P1, 72, 32)];
        r.passes(&ev, 4);
        let step = 1.0 / 127.0;
        assert!(
            (r.stored(P1, ATTACK) - (0.2 + 32.0 * step)).abs() < 1e-6,
            "{e:?}"
        );
        assert!(
            (r.stored(P1, RELEASE) - (0.5 - 32.0 * step)).abs() < 1e-6,
            "{e:?}"
        );
        let ev = [r.cc(P1, 73, 64), r.cc(P1, 72, 64)];
        r.passes(&ev, 4);
        assert!(
            (r.stored(P1, ATTACK) - 0.2).abs() < 1e-6,
            "{e:?}: 64 is the Sound"
        );
        assert!((r.stored(P1, RELEASE) - 0.5).abs() < 1e-6, "{e:?}");
    }
}

/// ALGO: every operator's own AR and RR shifted alike, turned round (a
/// higher rate is a shorter time), so the patch's shape survives; AR
/// never 0 (an attack that never rises).
#[test]
fn attack_and_release_shift_every_operators_own_rates_on_algo() {
    let mut r = Rig::new(EngineType::Algo);
    for i in 0..6u8 {
        r.set(op(i, AlgoOpParams::AR), f32::from(5 + 5 * i));
        r.set(op(i, AlgoOpParams::RR), f32::from(2 + i));
    }
    // +32: 8 AR steps slower; -64: 7 RR steps faster.
    let ev = [r.cc(P1, 73, 96), r.cc(P1, 72, 0)];
    r.passes(&ev, 4);
    for i in 0..6u8 {
        let ar = (f32::from(5 + 5 * i) - 8.0).max(1.0);
        assert_eq!(r.stored(P1, op(i, AlgoOpParams::AR)), ar, "op {i} AR");
        assert_eq!(
            r.stored(P1, op(i, AlgoOpParams::RR)),
            f32::from(9 + i),
            "op {i} RR"
        );
    }
    let ev = [r.cc(P1, 73, 64), r.cc(P1, 72, 64)];
    r.passes(&ev, 4);
    for i in 1..6u8 {
        assert_eq!(
            r.stored(P1, op(i, AlgoOpParams::AR)),
            f32::from(5 + 5 * i),
            "op {i}"
        );
        assert_eq!(
            r.stored(P1, op(i, AlgoOpParams::RR)),
            f32::from(2 + i),
            "op {i}"
        );
    }
    assert_eq!(
        r.stored(P1, op(0, AlgoOpParams::AR)),
        5.0,
        "op 0 clamped at 1, 64 gives it back"
    );
}

/// The unit's case: AR at 31 can't go higher, and 64 is still 31. Each
/// cell is the Sound's value shifted, clamped, never the clamp shifted.
#[test]
fn sixty_four_restores_algo_rates_after_a_clamp() {
    let mut r = Rig::new(EngineType::Algo);
    for i in 0..6u8 {
        r.set(op(i, AlgoOpParams::AR), 31.0);
        r.set(op(i, AlgoOpParams::RR), 1.0);
    }
    r.set(op(1, AlgoOpParams::AR), 1.0);
    r.set(op(1, AlgoOpParams::RR), 15.0);
    for (ar, rr) in [(0, 127), (64, 64), (127, 0), (64, 64)] {
        let ev = [r.cc(P1, 73, ar), r.cc(P1, 72, rr)];
        r.passes(&ev, 2);
    }
    for i in 0..6u8 {
        let (ar, rr) = if i == 1 { (1.0, 15.0) } else { (31.0, 1.0) };
        assert_eq!(r.stored(P1, op(i, AlgoOpParams::AR)), ar, "op {i} AR");
        assert_eq!(r.stored(P1, op(i, AlgoOpParams::RR)), rr, "op {i} RR");
    }
}

/// 0, every value up to 127, then 64: each engine's cells exactly as the
/// Sound had them, whatever clamped on the way.
#[test]
fn a_sweep_to_both_ends_and_back_to_64_is_the_sound() {
    let cells: [(EngineType, &[(ParamAddr, f32)]); 4] = [
        (EngineType::Va, &[(ATTACK, 0.05), (RELEASE, 0.93)]),
        (EngineType::Tape, &[(ATTACK, 0.97), (RELEASE, 0.02)]),
        (EngineType::Modal, &[(DAMP, 0.81)]),
        (
            EngineType::Algo,
            &[
                (op(0, AlgoOpParams::AR), 31.0),
                (op(1, AlgoOpParams::AR), 2.0),
                (op(2, AlgoOpParams::AR), 0.0),
                (op(0, AlgoOpParams::RR), 15.0),
                (op(1, AlgoOpParams::RR), 1.0),
                (op(2, AlgoOpParams::RR), 8.0),
            ],
        ),
    ];
    for (e, cells) in cells {
        let mut r = Rig::new(e);
        for &(a, v) in cells {
            r.set(a, v);
        }
        for v in [0].into_iter().chain(0..=127).chain([64]) {
            let ev = [r.cc(P1, 73, v), r.cc(P1, 72, v)];
            r.passes(&ev, 2);
        }
        for &(a, v) in cells {
            assert_eq!(r.stored(P1, a), v, "{e:?} {a:?}");
        }
    }
}

/// MODAL rings until DAMP stops it: RELEASE shifts DAMP; ATTACK has no
/// cell.
#[test]
fn release_shifts_damp_on_modal() {
    let mut r = Rig::new(EngineType::Modal);
    r.set(DAMP, 0.3);
    let attack = r.stored(P1, ATTACK);
    let ev = [r.cc(P1, 72, 127), r.cc(P1, 73, 127)];
    r.passes(&ev, 4);
    assert!((r.stored(P1, DAMP) - (0.3 + 63.0 / 127.0)).abs() < 1e-6);
    assert_eq!(r.stored(P1, ATTACK), attack, "no ATTACK on MODAL");
}

/// A Sound load is the new Sound as it is: the offset counts from 0.
#[test]
fn a_sound_load_starts_the_shift_again() {
    let mut r = Rig::va();
    r.set(ATTACK, 0.2);
    let ev = [r.cc(P1, 73, 100)];
    r.passes(&ev, 2);
    let src = chimera_core::project::PartSource {
        part: P1,
        from: chimera_core::project::PartFrom::Init(EngineType::Va),
    };
    let t = r.ui.template();
    common::project::load_anyway(r.ui.project_mut(), t, src).unwrap();
    let init = r.stored(P1, ATTACK);
    let ev = [r.cc(P1, 73, 64)];
    r.passes(&ev, 2);
    assert_eq!(
        r.stored(P1, ATTACK),
        init,
        "64 on the new Sound is the Sound"
    );
}

/// A binding on the same CC, the Sound's or the controller map's, wins.
#[test]
fn the_parts_own_binding_wins() {
    let mut r = Rig::va();
    let s = r.ui.project_mut().edit_part(P1).sound;
    s.map
        .learn(Input::cc(74).unwrap(), Mappable::new(RESO).unwrap())
        .unwrap();
    let cutoff = r.stored(P1, CUTOFF);
    let ev = [r.cc(P1, 74, 0)];
    r.passes(&ev, 120);
    assert_eq!(r.stored(P1, CUTOFF), cutoff, "the built-in CUTOFF yields");
    assert_eq!(
        r.stored(P1, RESO),
        RESO.spec().unwrap().min,
        "the binding moves"
    );
}

#[test]
fn off_in_midi_config_it_hears_nothing() {
    let mut r = Rig::va();
    assert!(MidiSettings::DEFAULT.built_in(), "ON out of the box");
    let m = r.ui.midi();
    r.ui.set_midi(m.with_built_in(false));
    let was = r.stored(P1, LEVEL);
    let ev = [r.cc(P1, 7, 0)];
    r.passes(&ev, 60);
    assert_eq!(r.stored(P1, LEVEL), was);
}

/// On an MPE zone's master channel CC 74 is SLIDE alone (ADR 0252); the
/// rest still answer. Member channels never reach a Part's map.
#[test]
fn under_mpe_cc_74_is_slide_alone() {
    let mut r = Rig::va();
    let mut m = r.ui.midi();
    m.mpe = Zones::OFF.with_lower(7);
    r.ui.set_midi(m);
    assert_eq!(r.ch(P1), 0, "Part 1 on the lower zone's master");
    let cutoff = r.stored(P1, CUTOFF);
    let ev = [r.cc(P1, 74, 0), r.cc(P1, 71, 127), cc(2, 7, 0)];
    let level = r.stored(P1, LEVEL);
    r.passes(&ev, 120);
    assert_eq!(r.stored(P1, CUTOFF), cutoff, "CC 74 on the master: SLIDE");
    assert_eq!(
        r.stored(P1, RESO),
        RESO.spec().unwrap().max,
        "CC 71 answers"
    );
    assert_eq!(
        r.stored(P1, LEVEL),
        level,
        "a member channel's CC 7 is its note's"
    );
}

/// CC 11 scales LEVEL as the audio gets it, from 127 (none), and is no
/// edit: not saved, no `*`.
#[test]
fn expression_scales_level_and_is_no_edit() {
    use chimera_core::dsp::knobs::KnobPublisher;
    use chimera_core::instrument::AudioShared;
    use chimera_core::part::expressed;
    let mut r = Rig::va();
    assert_eq!(r.ui.project().part(P1).expression(), 127);
    let crc = project_crc(r.ui.project());
    let level = r.stored(P1, LEVEL);
    let ev = [r.cc(P1, 11, 64)];
    r.passes(&ev, 4);
    assert_eq!(r.ui.project().part(P1).expression(), 64);
    assert_eq!(r.stored(P1, LEVEL), level, "LEVEL itself stays");
    assert_eq!(project_crc(r.ui.project()), crc, "not saved");
    let mut shared = Box::new(AudioShared::default());
    let mut knobs = Box::new(KnobPublisher::new());
    shared.update_from_project(r.ui.project(), 0, &mut knobs);
    assert_eq!(shared.parts[0].expression, 64, "published");
    assert_eq!(shared.parts[0].mix.level, level, "LEVEL as set");
    let e = 64.0f32 / 127.0;
    assert_eq!(expressed(level, 64), level * (e * e), "GM's 40 log10 law");
    assert_eq!(
        expressed(level, 127).to_bits(),
        level.to_bits(),
        "127: LEVEL"
    );
}

/// Reset All Controllers (CC 121) puts expression back to 127, as GM
/// resets CC 11: through DIN's pipe into the UI.
#[test]
fn reset_all_controllers_restores_expression() {
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
    let mut r = Rig::va();
    let mut out = Ctrl(Vec::new());
    let mut pipe = DinPipe::new();
    pipe.feed(&[0xB0, 11, 0], &mut out);
    r.passes(&out.0, 2);
    assert_eq!(r.ui.project().part(P1).expression(), 0);
    out.0.clear();
    pipe.feed(&[0xB0, 121, 0], &mut out);
    assert!(
        out.0.iter().all(|e| !matches!(e, CtrlEvent::Ctl(..))),
        "its own event, never a CC learn could take: {:?}",
        out.0
    );
    r.passes(&out.0, 2);
    assert_eq!(r.ui.project().part(P1).expression(), 127);
}

/// Every built-in CC reaches the UI (none MIDI keeps: bank select, data
/// entry, its steps, RPN and NRPN select, the pedal, channel mode), none
/// is CC 1 (the wheel), and the chart in the guide is this one.
#[test]
fn every_built_in_cc_is_free_and_published() {
    let guide = include_str!("../../docs/guide/MIDI.md");
    for &cc in &BUILT_IN {
        assert!(FreeCc::new(cc).is_some(), "CC {cc} never reaches the UI");
        assert_ne!(cc, 1);
        assert!(
            guide.contains(&format!("| {cc} |")),
            "the guide lacks CC {cc}"
        );
    }
    let m = MidiSettings::DEFAULT;
    let v = U7::new(5).unwrap();
    for cc in 0..128u8 {
        let hit = builtin(&m, MidiChannel::new(0).unwrap(), cc, v, EngineType::Va);
        assert_eq!(hit != Builtin::None, BUILT_IN.contains(&cc), "CC {cc}");
    }
    // MIXER's chart is heard on MIXER's channel, never a Part's: both may
    // use a CC (71-74, 91-94) with no clash.
    let _ = mixer_target;
}

/// MIDI CONFIG › CONTROL CHANNELS' D: BUILT-IN, ON by default.
#[test]
fn the_control_page_turns_it_off() {
    use chimera_core::ui::settings::leaves::CONTROL;
    let mut ui = Box::new(screen::stack_ui());
    screen::to_leaf(&mut ui, &["MIDI CONFIG", "CONTROL CHANNELS"]);
    assert_eq!(ui.page_def().id, CONTROL.id);
    let shot = screen::render_ui(&ui);
    assert!(screen::cell_reads(&shot, 3, "ON"));
    feed(&mut ui, Key::turn(EncoderId::D, -1));
    assert!(!ui.midi().built_in());
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    shot.dump("midi_control_built_in_off");
    assert!(screen::cell_reads(&shot, 3, "OFF"));
}

/// Expression swells the whole Part: at 0 its FX feed is silent too.
#[test]
fn the_mix_plays_expression_into_the_sends() {
    use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
    use chimera_core::dsp::rev::sync::Tempo;
    use chimera_core::hw::{DAC_PAIRS, MAX_PARTS};
    use chimera_core::instrument::{AudioShared, DacBlocks, MixState, mix_parts};
    use chimera_hal::BLOCK_SIZE;
    let run = |expression: u8| {
        let mut shared = Box::new(AudioShared::default());
        shared.parts[0].params.out.sends = [0.2, 0.3, 0.4];
        shared.parts[0].expression = expression;
        let mut buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
        buses[0] = core::array::from_fn(|i| (i as f32 * 0.37).sin() * 0.5);
        let mut written = [false; MAX_PARTS];
        written[0] = true;
        let mut fx = Box::new(FxBus::new());
        let mut send = [[0.0; BLOCK_SIZE]; FX_SENDS];
        let mut out = Box::new(DacBlocks::new());
        *out.mix() = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
        let mut m = MixState::default();
        for _ in 0..400 {
            mix_parts(
                &buses,
                &written,
                &mut send,
                &mut m,
                &mut fx,
                &shared,
                Tempo::DEFAULT,
                48_000,
                &mut out,
            );
        }
        send
    };
    assert!(run(127).iter().all(|s| s.iter().any(|&x| x != 0.0)));
    assert!(
        run(0).iter().all(|s| s.iter().all(|&x| x == 0.0)),
        "expression 0: no FX feed"
    );
}

/// Two Parts' unbound NRPNs on one shared target (REVERB MIX): the last
/// message wins, each Part's canonical slot claimed by the other's.
#[test]
fn two_parts_canonical_nrpns_on_a_shared_target_end_on_the_last() {
    use chimera_core::nrpn::NrpnAddr;
    use chimera_midi::ctl::{DataValue, Sel};
    let mix = ParamAddr::new(
        BlockRef::Reverb,
        chimera_core::dsp::reverb::ReverbParams::MIX,
    );
    let n = NrpnAddr::of(mix).unwrap().get();
    let mut r = Rig::va();
    let nrpn = |c: u8, v: u8| {
        CtrlEvent::Ctl(
            MidiChannel::new(c).unwrap(),
            Ctl::Data {
                sel: Sel::Nrpn(n),
                value: DataValue::Coarse(U7::new(v).unwrap()),
            },
        )
    };
    let (c1, c2) = (r.ch(P1), r.ch(P2));
    r.passes(&[nrpn(c1, 0), nrpn(c2, 127)], 200);
    assert_eq!(r.stored(P1, mix), 1.0, "Part 2's, the last");
}

/// INIT's and the factory Sounds' CC 74 → CUTOFF row is SLIDE's too on a
/// zone's master (ADR 0252).
#[test]
fn a_cutoff_row_on_cc_74_yields_to_slide_on_an_mpe_master() {
    let mut r = Rig::va();
    let s = r.ui.project_mut().edit_part(P1).sound;
    s.map
        .learn(Input::cc(74).unwrap(), Mappable::new(CUTOFF).unwrap())
        .unwrap();
    let mut m = r.ui.midi();
    m.mpe = Zones::OFF.with_lower(7);
    r.ui.set_midi(m);
    let cutoff = r.stored(P1, CUTOFF);
    let ev = [r.cc(P1, 74, 0)];
    r.passes(&ev, 120);
    assert_eq!(r.stored(P1, CUTOFF), cutoff);
}

/// What you hear is what you see and save (owner): the cell shows the
/// shifted value, a save stores it, a reload sounds the same, and the
/// save is the new 64.
#[test]
fn a_shift_saved_is_the_sound_and_the_new_64() {
    use chimera_core::project::{PartFrom, PartSource, SlotId, test_support};
    let mut r = Rig::va();
    r.set(ATTACK, 0.2);
    let ev = [r.cc(P1, 73, 96)];
    r.passes(&ev, 2);
    let shifted = r.stored(P1, ATTACK);
    assert!(
        (shifted - (0.2 + 32.0 / 127.0)).abs() < 1e-6,
        "the cell shows it"
    );
    let s = SlotId::new(100).unwrap();
    test_support::save_part_to(r.ui.project_mut(), P1, s);
    // The knob stays at 96 a pass: no move. Then 64: the Sound as saved.
    let ev = [r.cc(P1, 73, 64)];
    r.passes(&ev, 2);
    assert_eq!(r.stored(P1, ATTACK), shifted, "64 is the Sound as saved");
    r.set(ATTACK, 0.9);
    let src = PartSource {
        part: P1,
        from: PartFrom::Slot(s),
    };
    let t = r.ui.template();
    common::project::load_anyway(r.ui.project_mut(), t, src).unwrap();
    assert_eq!(
        r.stored(P1, ATTACK),
        shifted,
        "reloaded, it sounds the same"
    );
}

/// Reset All Controllers is the Sound as saved: the shifts undone.
#[test]
fn reset_all_controllers_undoes_the_shifts() {
    let mut r = Rig::va();
    r.set(ATTACK, 0.2);
    r.set(RELEASE, 0.5);
    let ev = [r.cc(P1, 73, 100), r.cc(P1, 72, 10)];
    r.passes(&ev, 2);
    let ev = [CtrlEvent::Reset(
        chimera_midi::value::Channel::new(r.ch(P1)).unwrap(),
    )];
    r.passes(&ev, 2);
    assert!((r.stored(P1, ATTACK) - 0.2).abs() < 1e-6);
    assert!((r.stored(P1, RELEASE) - 0.5).abs() < 1e-6);
}

/// Expression at 127 is no expression, bit for bit: the mix's factor is
/// exactly 1.0, so LEVEL and every send multiply to themselves.
#[test]
fn expression_127_is_bit_exact() {
    use chimera_core::part::expressed;
    assert_eq!(expressed(1.0, 127).to_bits(), 1.0f32.to_bits());
    let mut x = 0x3f00_0001u32;
    for _ in 0..10_000 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let v = f32::from_bits(0x3000_0000 | x >> 4);
        assert_eq!((v * expressed(1.0, 127)).to_bits(), v.to_bits(), "{v}");
        assert_eq!(expressed(v, 127).to_bits(), v.to_bits(), "{v}");
    }
}
