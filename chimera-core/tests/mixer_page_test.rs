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

/// A 240×320 framebuffer for drawing the renderer's output in tests.
struct Fb(Vec<embedded_graphics::pixelcolor::Rgb565>);

impl embedded_graphics::geometry::OriginDimensions for Fb {
    fn size(&self) -> embedded_graphics::geometry::Size {
        embedded_graphics::geometry::Size::new(240, 320)
    }
}

impl embedded_graphics::draw_target::DrawTarget for Fb {
    type Color = embedded_graphics::pixelcolor::Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
    {
        for embedded_graphics::Pixel(p, c) in pixels {
            if (0..240).contains(&p.x) && (0..320).contains(&p.y) {
                self.0[p.y as usize * 240 + p.x as usize] = c;
            }
        }
        Ok(())
    }
}

/// The PART page's mixer viz reads the LEVEL and PAN slots (CH, MODE, OUT,
/// LEVEL, PAN), not slots 0 and 1: level 1 fills Part's bar, pan hard
/// right puts the pan marker at the right end.
#[test]
fn part_viz_reads_the_level_and_pan_slots() {
    use chimera_core::ui::chain::ChainNav;
    use chimera_core::ui::mod_grid::MatrixState;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::RegionKind;
    use chimera_core::ui::renderer::Renderer;
    use chimera_core::ui::theme;

    let mut r = Renderer::new();
    r.snap_to_current([0.0, 0.0, 0.0, 1.0, 1.0, 0.0]); // CH MODE OUT = 0; LEVEL 1, PAN right
    let mut fb = Fb(vec![theme::BG; 240 * 320]);
    let (nav, matrix) = (ChainNav::new(), MatrixState::new());
    let scope = [0.0; chimera_core::scope::SCOPE_LEN];
    let frame = chimera_core::ui::renderer::Frame {
        nav: &nav, def: &reg::PART, perf: &PerfStats::zero(), matrix: &matrix, sel_op: Op::A, focus: 0, scope: &scope, sounding: false,
    };
    r.draw_region_with_def(&mut fb, RegionKind::Viz, &frame);
    let px = |x: i32, y: i32| fb.0[y as usize * 240 + x as usize];
    // First bar: x 32..52, y 44..162 (theme::VIZ_LEFT + 20, VIZ_TOP + 16, VIZ_BOTTOM - 8).
    let filled = (44..162).filter(|&y| px(40, y) == theme::PARAM_BAR_FG).count();
    assert_eq!(filled, 162 - 44, "level 1 fills the bar");
    let marker: Vec<i32> = (0..240).filter(|&x| px(x, 32) == theme::ACCENT).collect();
    assert!(!marker.is_empty() && marker.iter().all(|&x| x > 150), "pan marker right: {marker:?}");
}

/// The sound browser's title names what it loads and the Part it loads
/// into: "LOAD SOUND: P2" for Part 2 (it was "LOAD PATCH: B2").
#[test]
fn sound_browser_title_names_the_part() {
    use chimera_core::preset::{ChainType, SoundPool};
    use chimera_core::ui::renderer::Renderer;
    use chimera_core::ui::theme;
    use embedded_graphics::mono_font::{ascii::FONT_6X10, MonoTextStyle};
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::Text;

    let title_band = |fb: &Fb| fb.0[..22 * 240].to_vec();
    let mut got = Fb(vec![theme::BG; 240 * 320]);
    Renderer::draw_sound_browser(&mut got, &SoundPool::new(), 1, 0, 0, ChainType::PizzaPoly);
    let mut want = Fb(vec![theme::BG; 240 * 320]);
    let style = MonoTextStyle::new(&FONT_6X10, theme::ACCENT);
    Text::new("LOAD SOUND: P2", Point::new(8, theme::HEADER_Y + 10), style).draw(&mut want).unwrap();
    assert!(title_band(&got) == title_band(&want), "title is LOAD SOUND: P2");
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
