//! Mixer chain (instrument-core spec § UI): MIX + B<n> opens Part n's PART
//! and SENDS pages and the shared FX pages, all bound through slot bindings.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::part::{DacPair, PartMode, PartParams};
use chimera_core::preset::Performance;
use chimera_core::ui::block_def::{BlockDef, SlotBinding};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::page::PageKey;
use chimera_core::ui::{part_page, UiState};
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

struct MockControls {
    buttons: Vec<(ButtonId, ButtonState)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl MockControls {
    fn new() -> Self {
        Self { buttons: Vec::new(), encoders: Vec::new() }
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
        self.buttons.iter().find(|b| b.0 == id).map_or(ButtonState::Up, |b| b.1)
    }
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
}

fn open_mixer(ui: &mut UiState, b: ButtonId) {
    ui.handle_input(&MockControls::new().button(ButtonId::Mix, ButtonState::Held).button(b, ButtonState::Pressed));
}

fn turn(ui: &mut UiState, enc: EncoderId, delta: i8) {
    ui.handle_input(&MockControls::new().encoder(enc, delta));
}

#[test]
fn mixer_chain_is_part_sends_and_fx() {
    let names: Vec<&str> = reg::MIXER_CHANNEL_CHAIN.blocks.iter().map(|b| b.def.name).collect();
    assert_eq!(names, ["Part", "Sends", "Chorus", "Delay", "Reverb"]);
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
    let ids = [PartParams::CHANNEL, PartParams::MODE, PartParams::OUTPUT, PartParams::LEVEL, PartParams::PAN];
    for (i, id) in ids.into_iter().enumerate() {
        assert_eq!(at(i), ParamAddr::new(BlockRef::Part, id));
    }
}

/// MIX + B2 selects Part 2 and its encoders edit Part 2's mix settings.
#[test]
fn mix_b2_edits_part_2() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B2);
    assert_eq!(ui.active_part, 1);
    assert!(matches!(ui.page(), PageKey::Part { def: 27, .. }));
    turn(&mut ui, EncoderId::A, 3); // CH 1 → 4
    turn(&mut ui, EncoderId::B, -1); // Poly → Mono
    turn(&mut ui, EncoderId::C, 2); // P1 → P3
    turn(&mut ui, EncoderId::D, -8); // level
    let m = &ui.performance.parts[1].mix;
    assert_eq!((m.channel.get(), m.mode, m.output), (4, PartMode::Mono, DacPair::P3));
    assert_eq!(m.level, 0.8 - 8.0 / 128.0);
    assert_eq!(ui.performance.parts[0].mix, PartParams::for_part(0), "part 1 untouched");
}

#[test]
fn sends_page_edits_the_part_sends() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B3);
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed)); // → SENDS
    turn(&mut ui, EncoderId::C, 64); // reverb send
    assert_eq!(ui.performance.parts[2].mix.sends, [0.0, 0.0, 0.5]);
}

fn turn_def(def: &BlockDef, slot: usize, delta: i8, perf: &mut Performance) {
    part_page::apply_encoder(def, slot, delta, &mut perf.edit(0), &mut Op::A);
}

/// FX pages keep the old encoder steps and edit the Performance's FX.
#[test]
fn fx_encoders_step_like_before() {
    let mut perf = Performance::new();
    turn_def(&reg::DELAY, 0, 2, &mut perf);
    assert_eq!(perf.fx.delay.time_ms, 375.0 + 2.0 * 8.0);
    turn_def(&reg::CHORUS, 0, 5, &mut perf);
    assert_eq!(perf.fx.chorus.mode, 3);
    turn_def(&reg::EFX, 0, 5, &mut perf);
    assert_eq!(perf.fx.reverb.reverb_type, 2);
    turn_def(&reg::EFX, 4, -1, &mut perf);
    assert_eq!(perf.fx.reverb.mix, 0.0);
    turn_def(&reg::EFX, 4, 1, &mut perf);
    assert_eq!(perf.fx.reverb.mix, 1.0 / 128.0);
    part_page::snap_encoder(&reg::DELAY, 5, 1, &mut perf.edit(0), Op::A);
    assert_eq!(perf.fx.delay.mix, 100.0 / 127.0);
}

/// One FX set for every Part: an edit from part 1 is what part 4 shows.
#[test]
fn fx_are_shared_across_parts() {
    let mut perf = Performance::new();
    turn_def(&reg::DELAY, 0, 2, &mut perf);
    let shown = part_page::read_values(&reg::DELAY, &perf.edit(3), Op::A)[0];
    assert_eq!(shown, (391.0 - 10.0) / (500.0 - 10.0));
}

/// ADR 0010: mix settings are not modulatable, so MIX + Plus primes nothing.
#[test]
fn priming_a_part_param_is_refused() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B1);
    turn(&mut ui, EncoderId::D, 1); // focus LEVEL
    ui.handle_input(&MockControls::new().button(ButtonId::Mix, ButtonState::Held).button(ButtonId::Plus, ButtonState::Pressed));
    assert!(ui.performance.parts[0].sound.dest_registry.is_empty());
}

mod screen;

fn overview(setup: impl FnOnce(&mut UiState), part_button: ButtonId) -> screen::Fb {
    let mut ui = UiState::new();
    setup(&mut ui);
    open_mixer(&mut ui, part_button);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    ui.render_with_scope(&mut fb, &chimera_core::ui::perf::PerfStats::zero(), &screen::scope_fixture());
    fb
}

/// Mixer PART viz band: every Part's level bar and pan dot, the edited Part
/// lit and drawn from its LEVEL and PAN slots (CH, MODE, OUT, LEVEL, PAN).
#[test]
fn part_overview_shows_every_part_with_the_edited_one_lit() {
    use chimera_core::ui::theme;
    use chimera_core::ui::viz::{strip_x, STRIP_H, STRIP_PAN_Y, STRIP_TOP};
    let fb = overview(
        |ui| {
            ui.performance.parts[1].mix.level = 1.0;
            ui.performance.parts[1].mix.pan = 1.0;
            ui.performance.parts[3].mix.level = 0.0;
        },
        ButtonId::B2,
    );
    let column = |i: usize, c| (STRIP_TOP..STRIP_TOP + STRIP_H).filter(|&y| fb.at(strip_x(i) + 4, y) == c).count();
    assert_eq!(column(1, theme::ACCENT), STRIP_H as usize, "Part 2: full, lit");
    assert_eq!(column(0, theme::ACCENT), 0, "Part 1 not lit");
    assert!(column(0, theme::BAR_REST) > 0, "Part 1 at its stored level");
    assert_eq!(column(3, theme::BAR_REST), 0, "Part 4 at level 0");
    let dot: Vec<i32> = (strip_x(1) - 6..strip_x(1) + 16).filter(|&x| fb.at(x, STRIP_PAN_Y) == theme::INK).collect();
    assert!(dot.iter().all(|&x| x > strip_x(1) + 10), "Part 2 panned right: {dot:?}");
}

#[test]
fn part_overview_redraws_as_the_level_lerps() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B1);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    let (perf, scope) = (chimera_core::ui::perf::PerfStats::zero(), screen::scope_fixture());
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    turn(&mut ui, EncoderId::D, -20);
    ui.update();
    let flushed = ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    assert!(flushed.contains(&(118, 186)), "viz band follows the lerped level: {flushed:?}");
}

/// FX flow node `k` (0 = CHR) is the lit pill in `fb`.
fn flow_lit(fb: &screen::Fb) -> Vec<usize> {
    use chimera_core::ui::dungeon_map::node_x;
    use chimera_core::ui::theme;
    use chimera_core::ui::viz::FLOW_Y;
    (0..3).filter(|&k| fb.at(node_x(k + 1, 5) - 14, FLOW_Y) == theme::ACCENT).collect()
}

#[test]
fn fx_pages_light_their_effect_in_the_flow() {
    let fb = screen::render("mixer_fx_delay");
    assert_eq!(flow_lit(&fb), [1], "Delay page lights DLY");
    assert!(matches!(reg::SENDS.viz, chimera_core::ui::block_def::VizType::EffectsFlow));
}

#[test]
fn sends_page_lights_the_focused_send() {
    assert_eq!(flow_lit(&screen::render("mixer_sends")), [2], "REV send focused");
    let mut ui = screen::ui_for("mixer_sends");
    turn(&mut ui, EncoderId::A, 1);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    ui.render_with_scope(&mut fb, &chimera_core::ui::perf::PerfStats::zero(), &screen::scope_fixture());
    assert_eq!(flow_lit(&fb), [0], "CHR send focused");
}

#[test]
fn mixer_part_dirty_render_equals_full_render() {
    for name in ["mixer_part", "mixer_sends", "mixer_fx_delay"] {
        assert!(screen::render(name).px == screen::render_dirty(name).px, "{name}");
    }
}

/// The sound browser's title names what it loads and the Part it loads
/// into: "LOAD SOUND → PART 2" for Part 2.
#[test]
fn sound_browser_title_names_the_part() {
    use chimera_core::preset::SoundPool;
    use chimera_core::ui::{browser, components};

    let mut got = screen::Fb::new();
    browser::draw(&mut got, &SoundPool::new(), 1, 0, 0);
    let mut want = screen::Fb::new();
    want.px.fill(got.px[0]); // the ground
    components::title_to(&mut want, "LOAD SOUND", "PART 2");
    assert!(got.px[..28 * 240] == want.px[..28 * 240], "title is LOAD SOUND → PART 2");
}

/// The PART page shows CH as 1–16 (stored 0–15), MODE as MONO/POLY and OUT
/// as P1/P2/P3, through each slot's value format.
#[test]
fn part_page_shows_channel_mode_and_output_by_name() {
    use chimera_core::block::Block;
    use chimera_core::ui::fmt::{fmt_val, FmtBuf};

    let shown = |p: &PartParams, slot: usize| {
        let SlotBinding::Param(a) = reg::PART.params[slot].binding else { panic!("slot {slot}") };
        let mut buf = FmtBuf::new();
        fmt_val(&mut buf, p.normalized(a.param), reg::PART.params[slot].format());
        buf.as_str().to_string()
    };
    let mut p = PartParams::for_part(0);
    assert_eq!([shown(&p, 0), shown(&p, 1), shown(&p, 2)], ["1", "POLY", "P1"]);
    p.set(PartParams::CHANNEL, 15.0);
    p.set(PartParams::MODE, 0.0);
    p.set(PartParams::OUTPUT, 2.0);
    assert_eq!([shown(&p, 0), shown(&p, 1), shown(&p, 2)], ["16", "MONO", "P3"]);
    p.set(PartParams::OUTPUT, 1.0);
    assert_eq!(shown(&p, 2), "P2");
    // Near-integer display values (the renderer lerps) still round to a name.
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.97, reg::PART.params[1].format());
    assert_eq!(buf.as_str(), "POLY");
}
