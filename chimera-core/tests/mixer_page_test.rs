//! Mixer chain (instrument-core spec § UI): MIX + B<n> opens Part n's PART
//! and SENDS pages and the shared FX pages, all bound through slot bindings.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::part::{DacPair, PartMode, PartParams};
use chimera_core::preset::Performance;
use chimera_core::project::PartId;
use chimera_core::ui::block_def::{BlockDef, FxFlow, FxNode, SlotBinding, VizType};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::page::PageKey;
use chimera_core::ui::{UiState, part_page};
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

struct MockControls {
    buttons: Vec<(ButtonId, ButtonState)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl MockControls {
    fn new() -> Self {
        Self {
            buttons: Vec::new(),
            encoders: Vec::new(),
        }
    }
    fn button(mut self, id: ButtonId, state: ButtonState) -> Self {
        self.buttons.push((id, state));
        self
    }
    fn encoder(mut self, id: EncoderId, delta: i8) -> Self {
        self.encoders.push((id, delta));
        self
    }
}

impl Controls for MockControls {
    fn button_state(&self, id: ButtonId) -> ButtonState {
        self.buttons
            .iter()
            .find(|b| b.0 == id)
            .map_or(ButtonState::Up, |b| b.1)
    }
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
}

/// MIX + `b`: the mixer opens on SENDS (ADR 0057).
fn open_mixer(ui: &mut UiState, b: ButtonId) {
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(b, ButtonState::Pressed),
    );
}

/// MIX + `b`, then MINUS back to PART.
fn open_mixer_part(ui: &mut UiState, b: ButtonId) {
    open_mixer(ui, b);
    ui.handle_input(&MockControls::new().button(ButtonId::Minus, ButtonState::Pressed));
}

fn turn(ui: &mut UiState, enc: EncoderId, delta: i8) {
    ui.handle_input(&MockControls::new().encoder(enc, delta));
}

#[test]
fn mixer_chain_is_part_sends_and_fx() {
    let names: Vec<&str> = reg::MIXER_CHANNEL_CHAIN
        .blocks
        .iter()
        .map(|b| b.def.name)
        .collect();
    // TAPE before MASTER only with `master-tape` (ADR 0055).
    let want: &[&str] = if cfg!(feature = "master-tape") {
        &[
            "Part", "Sends", "Chorus", "Delay", "Reverb", "Tape", "Master",
        ]
    } else {
        &["Part", "Sends", "Chorus", "Delay", "Reverb", "Master"]
    };
    assert_eq!(names, want);
}

/// Every slot on the Mixer chain is bound to a real spec: no Legacy slot is
/// left to edit the wrong block.
#[test]
fn every_mixer_slot_is_bound() {
    for block in reg::MIXER_CHANNEL_CHAIN.blocks {
        for (i, slot) in block.def.params.iter().enumerate() {
            match slot.binding {
                SlotBinding::Empty => {}
                SlotBinding::Param(a) => assert!(a.spec().is_some(), "{} slot {i}", block.def.name),
                other => panic!("{} slot {i}: {other:?}", block.def.name),
            }
        }
    }
}

#[test]
fn part_page_binds_channel_mode_output_level_pan() {
    let at = |i: usize| match reg::PART.params[i].binding {
        SlotBinding::Param(a) => a,
        other => panic!("slot {i}: {other:?}"),
    };
    let ids = [
        PartParams::CHANNEL,
        PartParams::MODE,
        PartParams::OUTPUT,
        PartParams::LEVEL,
        PartParams::PAN,
    ];
    for (i, id) in ids.into_iter().enumerate() {
        assert_eq!(at(i), ParamAddr::new(BlockRef::Part, id));
    }
}

/// MIX + B2 selects Part 2 and its encoders edit Part 2's mix settings.
#[test]
fn mix_b2_edits_part_2() {
    let mut ui = UiState::new();
    open_mixer_part(&mut ui, ButtonId::B2);
    assert_eq!(ui.active_part, PartId::ALL[1]);
    assert!(matches!(ui.page(), PageKey::Part { def: 27, .. }));
    turn(&mut ui, EncoderId::A, 3); // CH 1 → 4
    turn(&mut ui, EncoderId::B, -1); // Poly → Mono
    turn(&mut ui, EncoderId::C, 2); // P1 → P3
    turn(&mut ui, EncoderId::D, -8); // level
    let m = &ui.project().part(PartId::ALL[1]).mix;
    assert_eq!(
        (m.channel.get(), m.mode, m.output),
        (4, PartMode::Mono, DacPair::P3)
    );
    assert_eq!(m.level, 0.8 - 8.0 / 128.0);
    assert_eq!(
        ui.project().part(PartId::ALL[0]).mix,
        PartParams::for_part(0),
        "part 1 untouched"
    );
}

#[test]
fn sends_page_edits_the_part_sends() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B3); // SENDS
    turn(&mut ui, EncoderId::C, 64); // reverb send
    assert_eq!(ui.project().part(PartId::ALL[2]).mix.sends, [0.0, 0.0, 0.5]);
}

fn turn_def(def: &BlockDef, slot: usize, delta: i8, perf: &mut Performance) {
    part_page::apply_encoder(def, slot, delta, &mut perf.edit(PartId::ALL[0]), &mut Op::A);
}

/// Each slot's parameter address, `None` where the slot is not bound.
fn bound(def: &BlockDef) -> Vec<Option<ParamAddr>> {
    def.params
        .iter()
        .map(|s| match s.binding {
            SlotBinding::Param(a) => Some(a),
            _ => None,
        })
        .collect()
}

/// FX pages keep the old encoder steps and edit the Performance's FX.
#[test]
fn fx_encoders_step_like_before() {
    let mut perf = Performance::new();
    turn_def(&reg::DELAY, 0, 2, &mut perf);
    assert_eq!(perf.fx.delay.time_ms, 375.0 + 2.0 * 8.0);
    turn_def(&reg::CHORUS, 0, 5, &mut perf);
    assert_eq!(perf.fx.chorus.mode, 3);
    turn_def(&reg::EFX, 4, -1, &mut perf);
    assert_eq!(perf.fx.reverb.mix, 0.0);
    turn_def(&reg::EFX, 4, 1, &mut perf);
    assert_eq!(perf.fx.reverb.mix, 1.0 / 128.0);
    turn_def(&reg::EFX, 0, 5, &mut perf);
    assert!((perf.fx.reverb.grit - (0.3 + 5.0 / 128.0)).abs() < 1e-6);
    turn_def(&reg::EFX, 3, 1, &mut perf);
    assert!((perf.fx.reverb.size - (0.5 + 1.0 / 31.0)).abs() < 1e-6);
    turn_def(&reg::DELAY, 3, 64, &mut perf);
    assert_eq!(perf.fx.delay.rev_send, 0.5);
    turn_def(&reg::DELAY_CHAR, 0, 1, &mut perf);
    assert!((perf.fx.delay.wow_flutter - (0.15 + 1.0 / 128.0)).abs() < 1e-6);
    part_page::snap_encoder(&reg::DELAY, 4, 1, &mut perf.edit(PartId::ALL[0]), Op::A);
    assert_eq!(perf.fx.delay.mix, 100.0 / 127.0);
}

/// One FX set for every Part: an edit from part 1 is what part 4 shows.
#[test]
fn fx_are_shared_across_parts() {
    let mut perf = Performance::new();
    turn_def(&reg::DELAY, 0, 2, &mut perf);
    let shown = part_page::read_values(&reg::DELAY, &perf.edit(PartId::ALL[3]), Op::A)[0];
    assert_eq!(shown, (391.0 - 10.0) / (500.0 - 10.0));
}

/// ADR 0010: mix settings are not modulatable, so MIX + Plus primes nothing.
#[test]
fn priming_a_part_param_is_refused() {
    let mut ui = UiState::new();
    open_mixer_part(&mut ui, ButtonId::B1);
    turn(&mut ui, EncoderId::D, 1); // focus LEVEL
    ui.handle_input(
        &MockControls::new()
            .button(ButtonId::Mix, ButtonState::Held)
            .button(ButtonId::Plus, ButtonState::Pressed),
    );
    assert_eq!(
        ui.project().part(PartId::ALL[0]).sound.dest_registry.len(),
        1,
        "only the default CUTOFF"
    );
}

mod screen;

fn overview(setup: impl FnOnce(&mut UiState), part_button: ButtonId) -> screen::Fb {
    let mut ui = UiState::new();
    setup(&mut ui);
    open_mixer_part(&mut ui, part_button);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    ui.render_with_scope(
        &mut fb,
        &chimera_core::ui::perf::PerfStats::zero(),
        &screen::scope_fixture(),
    );
    fb
}

/// Mixer PART viz band: every Part's level bar and pan dot, the edited Part
/// lit and drawn from its LEVEL and PAN slots (CH, MODE, OUT, LEVEL, PAN).
#[test]
fn part_overview_shows_every_part_with_the_edited_one_lit() {
    use chimera_core::ui::theme;
    use chimera_core::ui::viz::{STRIP_H, STRIP_PAN_Y, STRIP_TOP, strip_x};
    let fb = overview(
        |ui| {
            ui.project_mut().edit_part(PartId::ALL[0]).part.mix.pan = -1.0;
            ui.project_mut().edit_part(PartId::ALL[1]).part.mix.level = 1.0;
            ui.project_mut().edit_part(PartId::ALL[1]).part.mix.pan = 1.0;
            ui.project_mut().edit_part(PartId::ALL[2]).part.mix.pan = 0.0;
            ui.project_mut().edit_part(PartId::ALL[3]).part.mix.level = 0.0;
        },
        ButtonId::B2,
    );
    let column = |i: usize, c| {
        (STRIP_TOP..STRIP_TOP + STRIP_H)
            .filter(|&y| fb.at(strip_x(i) + 4, y) == c)
            .count()
    };
    assert_eq!(
        column(1, theme::ACCENT),
        STRIP_H as usize,
        "Part 2: full, lit"
    );
    assert_eq!(column(0, theme::ACCENT), 0, "Part 1 not lit");
    assert!(column(0, theme::BAR_REST) > 0, "Part 1 at its stored level");
    assert_eq!(column(3, theme::BAR_REST), 0, "Part 4 at level 0");
    let dot: Vec<i32> = (strip_x(1) - 6..strip_x(1) + 16)
        .filter(|&x| fb.at(x, STRIP_PAN_Y) == theme::INK)
        .collect();
    assert!(
        dot.iter().all(|&x| x > strip_x(1) + 10),
        "Part 2 panned right: {dot:?}"
    );
    // Part 1: hard left (unselected, so MID) -- the dot sits left of centre.
    let l_dot: Vec<i32> = (strip_x(0) - 16..strip_x(0) + 6)
        .filter(|&x| fb.at(x, STRIP_PAN_Y) == theme::MID)
        .collect();
    assert!(!l_dot.is_empty(), "Part 1 dot must draw");
    assert!(
        l_dot.iter().all(|&x| x < strip_x(0) - 2),
        "Part 1 panned left: {l_dot:?}"
    );
    // Part 3: centre (unselected, so MID) -- the dot sits on the strip's centre x.
    let c_x = strip_x(2) + 4;
    let c_dot: Vec<i32> = (strip_x(2) - 6..strip_x(2) + 16)
        .filter(|&x| fb.at(x, STRIP_PAN_Y) == theme::MID)
        .collect();
    assert!(!c_dot.is_empty(), "Part 3 dot must draw");
    assert!(
        c_dot.iter().all(|&x| (c_x - 2..=c_x + 2).contains(&x)),
        "Part 3 centred: {c_dot:?}, want near {c_x}"
    );
}

#[test]
fn part_overview_redraws_as_the_level_lerps() {
    let mut ui = UiState::new();
    open_mixer_part(&mut ui, ButtonId::B1);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    let (perf, scope) = (
        chimera_core::ui::perf::PerfStats::zero(),
        screen::scope_fixture(),
    );
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    turn(&mut ui, EncoderId::D, -20);
    ui.update();
    let flushed = ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    assert!(
        flushed.contains(&(118, 186)),
        "viz band follows the lerped level: {flushed:?}"
    );
}

/// FX flow node `k` (0 = CHR) is the lit pill in `fb` (INK: the map pill
/// is the page's one accent pill).
fn flow_lit(fb: &screen::Fb) -> Vec<usize> {
    use chimera_core::ui::dungeon_map::node_x;
    use chimera_core::ui::theme;
    use chimera_core::ui::viz::FLOW_Y;
    (0..3)
        .filter(|&k| fb.at(node_x(k + 1, 5) - 14, FLOW_Y) == theme::INK)
        .collect()
}

/// The flow's nodes carry no accent: on SENDS and the FX pages the map
/// pill stays the only accent pill.
#[test]
fn the_flow_node_is_not_a_second_accent_pill() {
    use chimera_core::ui::theme;
    use chimera_core::ui::viz::FLOW_Y;
    for name in ["mixer_sends", "mixer_fx_delay"] {
        let fb = screen::render(name);
        let rows = FLOW_Y - theme::PILL_H / 2..=FLOW_Y + theme::PILL_H / 2;
        let accent = rows
            .flat_map(|y| (0..screen::W as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.at(x, y) == theme::ACCENT)
            .count();
        assert_eq!(accent, 0, "{name}");
    }
}

#[test]
fn fx_pages_light_their_effect_in_the_flow() {
    let fb = screen::render("mixer_fx_delay");
    assert_eq!(flow_lit(&fb), [1], "Delay page lights DLY");
    assert_eq!(reg::SENDS.viz, VizType::EffectsFlow(FxFlow::Sends));
    assert_eq!(
        flow_lit(&screen::render("mixer_fx_delay_char")),
        [1],
        "DLY › CHAR lights DLY"
    );
    assert_eq!(
        flow_lit(&screen::render("mixer_fx_reverb")),
        [2],
        "Reverb page lights REV"
    );
}

/// FX diet spec § UI: DLY keeps TIME, FDBK, TONE, REV and MIX; WOW and SAT
/// move to DLY › CHAR (an assumed default, pending the owner), on both
/// Mix chains.
#[test]
fn the_delay_page_is_time_fdbk_tone_rev_mix_with_char_below() {
    use chimera_core::dsp::delay::DelayParams as D;
    let at = |p| Some(ParamAddr::new(BlockRef::Delay, p));
    assert_eq!(
        bound(&reg::DELAY),
        [
            at(D::TIME_MS),
            at(D::FEEDBACK),
            at(D::TONE),
            at(D::REV_SEND),
            at(D::MIX),
            None
        ]
    );
    assert_eq!(
        bound(&reg::DELAY_CHAR),
        [
            at(D::WOW_FLUTTER),
            at(D::SATURATION),
            None,
            None,
            None,
            None
        ]
    );
    let chain = &reg::MIXER_CHANNEL_CHAIN;
    let dly = chain
        .blocks
        .iter()
        .find(|b| b.def.id == reg::DELAY.id)
        .unwrap();
    let subs: Vec<u16> = dly.sub_pages.iter().map(|d| d.id).collect();
    assert_eq!(subs, [reg::DELAY_CHAR.id], "{}", chain.name);
}

/// FX diet spec § UI: TYPE's slot is GRIT; the rest keep their order.
#[test]
fn the_reverb_page_is_grit_time_damp_size_mix() {
    use chimera_core::dsp::reverb::ReverbParams;
    let at = |p| Some(ParamAddr::new(BlockRef::Reverb, p));
    assert_eq!(
        bound(&reg::EFX),
        [
            at(ReverbParams::GRIT),
            at(ReverbParams::TIME),
            at(ReverbParams::DAMPING),
            at(ReverbParams::SIZE),
            at(ReverbParams::MIX),
            None
        ]
    );
    assert_eq!(
        reg::EFX.viz,
        VizType::EffectsFlow(FxFlow::Effect(FxNode::Reverb))
    );
}

#[test]
fn sends_page_lights_the_focused_send() {
    assert_eq!(
        flow_lit(&screen::render("mixer_sends")),
        [2],
        "REV send focused"
    );
    let mut ui = screen::ui_for("mixer_sends");
    turn(&mut ui, EncoderId::A, 1);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    ui.render_with_scope(
        &mut fb,
        &chimera_core::ui::perf::PerfStats::zero(),
        &screen::scope_fixture(),
    );
    assert_eq!(flow_lit(&fb), [0], "CHR send focused");
}

#[test]
fn mixer_part_dirty_render_equals_full_render() {
    for name in ["mixer_part", "mixer_sends", "mixer_fx_delay"] {
        assert!(
            screen::render(name).px == screen::render_dirty(name).px,
            "{name}"
        );
    }
}

/// The sound browser's title names what it loads and the Part it loads
/// into: "LOAD SOUND → PART 2" for Part 2.
#[test]
fn sound_browser_title_names_the_part() {
    use chimera_core::project::Project;
    use chimera_core::ui::{browser, components, theme};
    use embedded_graphics::draw_target::DrawTarget;

    // `draw` expects a screen cleared to the ground.
    let mut got = screen::Fb::new();
    let _ = got.clear(theme::BG);
    browser::draw(&mut got, Project::boxed().pool(), PartId::ALL[1], 0, 0);
    let mut want = screen::Fb::new();
    let _ = want.clear(theme::BG);
    components::title_to(&mut want, "LOAD SOUND", "PART 2");
    assert!(
        got.px[..28 * 240] == want.px[..28 * 240],
        "title is LOAD SOUND → PART 2"
    );
}

/// The PART page shows CH as 1–16 (stored 0–15), MODE as MONO/POLY and OUT
/// as P1/P2/P3, through each slot's value format.
#[test]
fn part_page_shows_channel_mode_and_output_by_name() {
    use chimera_core::block::Block;
    use chimera_core::ui::fmt::{FmtBuf, fmt_val};

    let shown = |p: &PartParams, slot: usize| {
        let SlotBinding::Param(a) = reg::PART.params[slot].binding else {
            panic!("slot {slot}")
        };
        let mut buf = FmtBuf::new();
        fmt_val(
            &mut buf,
            p.normalized(a.param),
            reg::PART.params[slot].format(),
        );
        buf.as_str().to_string()
    };
    let mut p = PartParams::for_part(0);
    assert_eq!(
        [shown(&p, 0), shown(&p, 1), shown(&p, 2)],
        ["1", "POLY", "P1"]
    );
    p.set(PartParams::CHANNEL, 15.0);
    p.set(PartParams::MODE, 0.0);
    p.set(PartParams::OUTPUT, 2.0);
    assert_eq!(
        [shown(&p, 0), shown(&p, 1), shown(&p, 2)],
        ["16", "MONO", "P3"]
    );
    p.set(PartParams::OUTPUT, 1.0);
    assert_eq!(shown(&p, 2), "P2");
    // Near-integer display values (the renderer lerps) still round to a name.
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.97, reg::PART.params[1].format());
    assert_eq!(buf.as_str(), "POLY");
}

/// ADR 0055: without `master-tape` the Mix chain has no TAPE page; MST
/// follows REV, and no page binds the tape's parameters.
#[cfg(not(feature = "master-tape"))]
#[test]
fn the_mix_chain_has_no_tape_page() {
    let chain = &reg::MIXER_CHANNEL_CHAIN;
    let ids: Vec<u16> = chain.blocks.iter().map(|b| b.def.id).collect();
    let rev = ids.iter().position(|&i| i == reg::EFX.id).unwrap();
    assert_eq!(ids[rev + 1], reg::MASTER.id, "{}", chain.name);
    for b in chain.blocks {
        let pages = core::iter::once(b.def).chain(b.sub_pages.iter().copied());
        for def in pages {
            assert!(
                bound(def)
                    .iter()
                    .flatten()
                    .all(|a| a.block != BlockRef::Tape),
                "{}",
                def.name
            );
        }
    }
}

/// FX diet spec § UI: TAPE is DRIVE, TONE, WOW, MIX, right after REV on
/// both Mix chains; its encoders edit the Performance's tape. Only with
/// `master-tape` (ADR 0055).
#[cfg(feature = "master-tape")]
#[test]
fn the_tape_page_is_drive_tone_wow_mix() {
    use chimera_core::dsp::tape::TapeParams as T;
    let at = |p| Some(ParamAddr::new(BlockRef::Tape, p));
    assert_eq!(
        bound(&reg::TAPE),
        [
            at(T::DRIVE),
            at(T::TONE),
            at(T::WOW),
            at(T::MIX),
            None,
            None
        ]
    );
    let chain = &reg::MIXER_CHANNEL_CHAIN;
    let ids: Vec<u16> = chain.blocks.iter().map(|b| b.def.id).collect();
    let rev = ids.iter().position(|&i| i == reg::EFX.id).unwrap();
    assert_eq!(ids[rev + 1], reg::TAPE.id, "{}", chain.name);
    let mut perf = Performance::new();
    turn_def(&reg::TAPE, 0, 64, &mut perf);
    turn_def(&reg::TAPE, 3, 32, &mut perf);
    assert_eq!((perf.fx.tape.drive, perf.fx.tape.mix), (0.5, 0.25));
}

/// FX diet spec § UI: MST binds THRESH, RATIO, ATK, REL, MAKEUP, MIX; the
/// legacy VOL and PAN sit on MST › LEVEL (an assumed default, pending the
/// owner); MST ends both Mix chains.
#[test]
fn the_master_page_is_the_compressor_with_level_below() {
    use chimera_core::dsp::comp::CompParams as C;
    let at = |p| Some(ParamAddr::new(BlockRef::Comp, p));
    assert_eq!(
        bound(&reg::MASTER),
        [
            at(C::THRESH),
            at(C::RATIO),
            at(C::ATTACK),
            at(C::RELEASE),
            at(C::MAKEUP),
            at(C::MIX)
        ]
    );
    let labels: Vec<&str> = reg::MASTER_LEVEL.params.iter().map(|s| s.label()).collect();
    assert_eq!(labels[..2], ["VOL", "PAN"]);
    let chain = &reg::MIXER_CHANNEL_CHAIN;
    let last = chain.blocks.last().unwrap();
    assert_eq!(last.def.id, reg::MASTER.id, "{}", chain.name);
    let subs: Vec<u16> = last.sub_pages.iter().map(|d| d.id).collect();
    assert_eq!(subs, [reg::MASTER_LEVEL.id], "{}", chain.name);
    let mut perf = Performance::new();
    assert!(!perf.fx.comp.is_on());
    turn_def(&reg::MASTER, 1, 4, &mut perf);
    assert_eq!(perf.fx.comp.ratio, 4);
    assert!(perf.fx.comp.is_on());
}

/// FX diet spec § UI: MST's GR meter is dark at 0 dB and fills half the
/// plot at 12 dB, and a change redraws the viz. One test, because
/// `MASTER_GR` is global.
#[test]
fn the_master_page_meters_gain_reduction() {
    use chimera_core::meter::MASTER_GR;
    use chimera_core::ui::theme;
    use chimera_core::ui::viz::{GR_X, PLOT_BASE, PLOT_TOP};
    let lit = |fb: &screen::Fb| {
        (PLOT_TOP..PLOT_BASE)
            .filter(|&y| fb.at(GR_X + 2, y) == theme::ACCENT)
            .count() as i32
    };
    MASTER_GR.publish(0.0);
    assert_eq!(lit(&screen::render("mixer_master")), 0);
    MASTER_GR.publish(12.0);
    assert_eq!(
        lit(&screen::render("mixer_master")),
        (PLOT_BASE - PLOT_TOP) / 2
    );

    MASTER_GR.publish(0.0);
    let mut ui = screen::ui_for("mixer_master");
    let (perf, scope) = (
        chimera_core::ui::perf::PerfStats::zero(),
        screen::scope_fixture(),
    );
    let mut fb = screen::Fb::new();
    let moved = |f: &[(u16, u16)]| f.iter().any(|&r| r != (0, 0));
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    assert!(!moved(&ui.render_dirty_with_scope(&mut fb, &perf, &scope)));
    MASTER_GR.publish(6.0);
    assert!(
        moved(&ui.render_dirty_with_scope(&mut fb, &perf, &scope)),
        "the meter redraws"
    );
    MASTER_GR.publish(0.0);
}
