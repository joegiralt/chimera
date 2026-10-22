# 0173. Draw each TAPE machine as a knob-driven glyph, still and live

- **Status:** Proposed
- **Deciders:** owner (the picks, 2026-10-19), firmware

## Context
TAPE's machine cells showed the plain arc, which says nothing about what a machine does to the sound. The owner picked a picture for each machine from mockups (#641; (private notes), outside the repo): ONE a match with 16 notches, LOOP a hamster wheel, PING Pong, TRON a window blind on a spring, STRETCH an accordion under a note, TRANSWAVE film through a gate, SCRUB a sponge.

The focus band redraws a region when its key changes. The animated composites (the chorus braid, the delay rings, the reverb cube and TAIL) put the UI clock in the key, so they redraw every frame. A glyph for every TAPE cell must not do that: TAPE is on screen while a Part plays, and idle redraws cost SPI1 time and look busy.

## Decision
- **One glyph a machine**, `FocusGlyph::Tape`, in the WAVE scope's 64 × 48 box (`theme::WAVE_X`, `WAVE_Y`). Every machine cell carries it: MODEL, MODE, POSITION, MOTION, SIZE, XFADE, DRAG, TYPE, WOW, TAPE and AGE. SLICE keeps none. PLAY shows its MODE's glyph: ONE, LOOP or PING.
- **Still and live.** MODEL's and MODE's cells show the machine's **still** pose: fixed inputs, unworn, its identity. Every other cell shows the **live** glyph, drawn from the Part's set values (`TAPE_GLYPH_PARAMS`, eased by `Renderer::eased_set`, never modulated).
- **Knob-driven, never idle.** A glyph changes only when a value moves, eases there with the cell (`AnimatedValue`), and holds still. Its inputs are quantised to a step that moves about a pixel (`tape_glyph::Q`: 62 steps for the 60 px window, 30 for marks of 30 px or less, 14 for SCRUB's SPEED and DRAG, 6 for XFADE and TAPE, and 14 for AGE). The focus band's key holds those steps (`TapeGauge::key`, through `Gauge::outside_key`) and **no frame term** (`anim_key` stays 0). So nothing redraws at idle, playing or not, and a dirty redraw equals a full render. **Only REVERB, DELAY and CHORUS animate continuously.** The playhead stays on the strip.
- **TAPE and AGE wear every glyph.** TAPE bends each row by a fixed sine of the row, up to 1.6 px at 100 %: a function of the value, not of time. AGE adds hashed grain on the ground, and above 50 % dims up to three fixed row bands as dropouts. Both are a `DrawTarget` wrapper over the box, so no 6 KB buffer.
- **Functional core.** `chimera_engine::tape_glyph::draw::render` is a pure function of a `TapeGauge` (a `Machine` enum with one typed input struct a machine, plus `Wear`) into a 64 × 48 `Canvas` of `Ink`s (the theme's tokens by name). The UI paints the canvas in its `Palette` (`chimera_core::ui::tape_glyph`), every pixel of the box.
- **The raster is bank 2's.** With `bank2` the `draw` module is linked into bank 2 (`bank2.x`) and bank 1 reaches it only through the door (`Bank2Door::glyph`, `tape_glyph::paint`), as the reverb's engines do (0142). Bank 1 keeps the inputs, the key and the paint. With bank 2 closed no glyph is drawn; TAPE is silent and labelled then anyway (0141). A build without TAPE's DSP (`tape` off: the bench builds) links no raster and draws no glyph.
- **ONE's match shows the play run.** The flame sits at START, the stick before it is charred, and the run to END is lit under it; END before START burns from the far end. The 16 notches are decoration only. SLICE is retired and slicing is KIT's (0170), so the glyph doesn't revive it. POSITION and LENGTH don't drive it, because ONE ignores them (`reads_in`).
- **SIZE is drawn linear in the knob**, though the real length is exponential, so it reads at 64 px. The value and the strip show the truth.
- **A cell the machine ignores keeps the glyph** (owner UAT, #645). Every machine reads only what it plays (`reads_in`), so an ignored param (TRON's POSITION and MOTION, ONE's POSITION, WALK, LENGTH and XFADE, PING's XFADE) moves neither the key nor a pixel. Its cell reads dimmed, and the focus band shows its dimmed value beside the machine's glyph, unchanged. Every other dimmed cell still shows no gauge.
- **LOOP's wheel rolls by POSITION along the whole window** (#645). Its radius is 10 to 15 px (LENGTH), so it never fills the box, and POSITION places it from end to end with its spokes turning as it goes. It rolls even at LENGTH 1, where the loop is the whole window and POSITION moves no sound: the fully lit strip shows that, as WALK's hamster runs whatever LENGTH is. The line under the wheel is the window strip with the loop lit, as designed.
- **XFADE dashes the loop's end** on LOOP's strip and SCRUB's run: its share of the loop (up to half), every other column. PING has none. **PLAY's FADE is not drawn**: it is the note-off fade, an amplitude release that changes neither where nor how the head plays, which is all the wheel, Pong and the match draw. TRON draws its FADE because there the snap back is the machine.
- **SCRUB's LENGTH is outlined, not filled** (#645), so the run reads beside the sponge's own accent. SPEED's arrow sits centred over the sponge, drawn last, so neither the sponge nor a bubble hides it.
- **STRETCH's chevrons count the speed**, not the knob: one per half × of `time_speed`, which runs −1× to +2×. Full left (1× back) shows two, as 1× forward does; full right (2×) shows four.
- **Long words drop a size.** A value wider than the room before the glyph (`WAVE_X − FOCUS_VALUE_X − 8`, such as CAPSTAN) reads in Logisoso 20.

## Alternatives considered
- **Idle animation (a turning wheel, a bouncing ball).** Rejected. It would redraw every frame on a page that is up while playing, and it would read as motion the sound doesn't have.
- **Drawing in bank 1 through embedded-graphics** (a `DrawTarget` wrapper for the wear). Rejected for flash: 15 KB of bank 1, mostly the triangle fill's line-join code and the inlined machines.
- **An RGB565 buffer for the box.** Rejected: 6 KB of stack, where the canvas of `Ink`s is 3 KB.
- **SCRUB's DRAG lag simulated in the glyph.** Rejected. It would need state and a timer in the drawing. The jog already steps POSITION with DRAG's momentum (0112), so the sponge follows the value. The wet trail's length is DRAG, and it trails the way SPEED plays.

## Consequences
- **Flash, ORBIT build:** bank 1 +3,288 B (45,340 → 42,052 B free), bank 2 +12,928 B (body 144,096 → 157,024 B). Drawn in bank 1 with embedded-graphics, the glyphs cost 15 KB and left 30 KB free, under the 40 KB floor; that is why the raster moved behind the door.
- **Stack:** the paint holds the 3 KB canvas on the UI's stack for one draw.
- KIT and REEL will need glyphs of their own when they land (ideas: an egg box of cups, a film reel with splice tabs). Their machine cells will carry `FocusGlyph::Tape`.
- A future glyph for any param follows the same rule: knob-driven, keyed on quantised steps, still at idle.

## Sources
- #641 (the picks), #615 (TAPE).
- The mockup: (private notes) and `notes.md`, the owner's ledger, outside the repo.
- ADR 0093 (the machine tables), 0112 (SCRUB's jog), 0170 (the lineup: PLAY's MODE, SLICE retired to KIT).
- `chimera-core/src/ui/tape_glyph.rs`, `chimera-core/tests/tape_glyph_test.rs`.
