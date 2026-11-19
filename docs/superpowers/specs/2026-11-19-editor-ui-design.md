# Browser editor: the v6 panel in Elm (#1019, #1014's screens)

**Status:** working design, 2026-11-19. It builds the owner-approved v6 mockup (branch editor-mockups, `docs/editor/mockups/v6/`) as the real editor. The rulings it follows are in the browser-editor memory (2026-11-18/19), and none are re-decided here.

## Intent

Slices 1 and 1b gave the editor its protocol (`ed hello`, `describe`, `get`, `set`, `since`, protocol 3) and a placeholder face (slice 1's Prophet grid, ALGO only). This work replaces that face with v6's design language on every engine, built from one library of typed Elm components. Every page uses the same parts, so a page is a table that places addresses into modules. The parts are never redrawn per page.

## Rulings this design must meet

1. DAY (cream, Cascadia-style) and NIGHT (v3/v4 dark) themes. Every colour is a CSS token. The switch lives in the top bar, and the default follows the OS.
2. Black module title bars, which are also fold handles.
3. Modules sit in rows of two, and both modules in a row share its height, so borders line up to the pixel.
4. Controls are chosen by meaning: faders for levels and envelope stages, knobs for frequency, ratio and morph, printed toggles for 2–3 choices, a rotary selector for 4–8, a menu above 8, a word row in the settings sheet.
5. MOD is v4's patch bay with a PATCH | MATRIX toggle, at full width.
6. The screens show the unit's own glyphs.
7. No sideways scroll, and no module wider than half the window except MOD.
8. Nothing moves when a value changes.
9. DRUM mirrors the box: a KIT tab plus PAD 1–12, Analog Rytm Overbridge-style.
10. The editor controls everything on the unit, in Elm, through typed components.

## The component library

There is one module per component, under `editor/src/Ui/`. Each has an opaque `Config` built with typed builders (`Knob.new addr |> Knob.label "CUTOFF" |> Knob.hero`) and a `view`. The interactive ones share one message type, so a page has one update for every control.

| Module | What it is | Interactive |
|---|---|---|
| `Ui.Tokens` | Theme (`Day`, `Night`), the token names, and theme from the OS or a stored choice | – |
| `Ui.Control` | The shared `Msg` (`Grab`, `Step`, `Pick`, `Reset`), the drag state and its `update` (pointer travel → normalised value → `Edit`/`Pick`), and `Reading` (norm, text, pending) | yes |
| `Ui.Knob` | Rotary: ticks, pointer, readout; `hero` for the one big knob; `bipolar` fills from the centre | via Control |
| `Ui.Fader` | Vertical fader: slot, cap, readout; `bipolar` | via Control |
| `Ui.Toggle` | Printed 2–3 position mini toggle (lever plus words) | via Control |
| `Ui.Selector` | 4–8 printed positions around a small selector knob | via Control |
| `Ui.Menu` | More than 8 choices: a button with the value that opens a list (optionally with glyphs) | own open/close state |
| `Ui.WordRow` | A row of words with the active one lit (settings sheet, header type menus) | via Control |
| `Ui.Screen` | One screen unit: the unit's glyph for a panel and variant (see *Glyphs*), with an optional caption | – |
| `Ui.Module` | A module: black title bar (title, small note, header menus, folded summary) that folds the body; jacks along the bottom | fold msg |
| `Ui.Row` | An aligned pair of modules (or one `full` module): the row's height is the taller module's, and folding one folds both | – |
| `Ui.Page` | The page: top bar (Parts, unit pages, theme switch, link), chip bar (show/hide, fold all), and rows wrapped to the window | chip and theme msgs |
| `Ui.PatchBay` | Sources and destinations as jacks, cables with a mid-cable attenuverter (amber adds, blue subtracts), and the PATCH | MATRIX toggle | own msg |
| `Ui.MatrixGrid` | The same routes as a signed amount grid | via Control |
| `Ui.PadStrip` | DRUM's KIT tab plus PAD 1–12 as the unit's pad grid (machine icon, name) | select msg |
| `Ui.Ring` | ORBIT's step ring (once the protocol reaches ORBIT) | via Control |

### How a control picks its kind

The default comes from the schema's `kind`, `scale` and option count:

| Schema | Default control |
|---|---|
| choice, 2–3 options | Toggle |
| choice, 4–8 options | Selector |
| choice, more than 8 | Menu |
| toggle | Toggle |
| cont, octave scale (frequency) | Knob |
| cont, otherwise | Fader |

A page's layout table can override the default by meaning: the hero CUTOFF, knobs for ratios and morphs, faders for envelope stages. A layout can never invent a parameter. Labels, ranges, option names and value text all come from the unit.

**Steps count from the unit (M-e).** An arrow key or a wheel notch steps from the value the unit last reported, never from an edit still in flight: the editor holds no value of its own. Several quick steps before the next `since` answer each send the same next value, so they move the unit one step, not several. A drag is different: it follows the pointer's travel from where it was grabbed.

### The fixed-width rule, as a contract

Every component's width is a function of its `Config` alone, never of its value. Each readout is sized in `ch` with tabular numerals from what the config knows:

- **A choice** knows its widest option name.
- **A continuous value** has a declared `chars` (default 8, the longest text the box's formats print: `1.20 kHz`, `+100`, `C#-1`), and longer text is cut with an ellipsis, never grown.

The rule is enforced in two places:

- **Types:** `view : Config -> Reading -> Html msg` takes no value into its sizing, because `width : Config -> Int` has no `Reading` to look at.
- **Tests:** every component is rendered at several readings, and the test checks that the sizing style is identical across them.

The Gallery runs the same check visually (see below).

## The Gallery

`just editor-gallery` builds `editor/gallery/` (a second Elm `main`, `Gallery.elm`). It shows every component in every state, side by side in DAY and NIGHT:

- rest and dragging, pending, bipolar negative/zero/positive;
- every option selected;
- the longest and shortest value text;
- a folded and an open module, and rows with modules of unequal content;
- the patch bay and the matrix with one or many cables.

A JITTER button steps every control through its range and prints the largest box shift (it must stay 0.00 px), as v6's `?probe=jitter` did.

## Pages

A page is `Ui.Page` over rows built from a layout table. Each table (`Layout/<Engine>.elm`) maps the schema's panels to modules: the title, the addresses in order, overrides by meaning, and which screen goes in it. A parameter the unit describes that the table doesn't place still shows up in an OTHER module, so nothing is ever hidden (slice 1's rule).

The module order follows v6:

| Page | Modules, in order |
|---|---|
| ALGO | ALGORITHM, PITCH, OP 1–6, FILTER, FILTER MOD, VOICE FX, AMP · OUT, ENV 1–3, LFO 1–3, MOD |
| VA | OSC 1, OSC 2, MODE, MIXER, FILTER, FILTER MOD, VOICE FX, AMP · OUT, ENV 1–3, LFO 1–3, PITCH, MOD |
| MODAL | RESONATOR, EXCITER, MODAL-2, PITCH, FILTER, FILTER MOD, VOICE FX, AMP · OUT, ENV 1–3, LFO 1–3, MOD |
| TAPE | SOURCE (read-only, #1015), DECK, MACHINE, PITCH, FILTER, FILTER MOD, VOICE FX, AMP · OUT, ENV 1–3, LFO 1–3, MOD |
| DRUM | KIT view: KIT pads, then the whole kit's FX, OUT, FILTER MOD, ENV 1–3, LFO 1–3. PAD n view: PAD, MACHINE, FILTER, AMP, PITCH ENV, PITCH LFO, ROUTE 1–4 (a patch bay over the pad's four routes), then MOD |

The shared modules (FILTER, FX, AMP, ENV, LFO, PITCH) live in one shared layout module and are used by every engine.

**Variants.** A panel with variants (FILTER by KIND, ENV by TYPE, OPn by WAVE, DRUM MACHINE by machine) shows the variant the unit's value selects, from `Model`'s variants. A type change may change a module's height. That is v6's one allowed exception, and the row re-aligns.

### What the protocol doesn't reach yet

These pages are left for a stop-and-report, not built on invented addresses:

- MIXER (`PartMix`, Chorus, Delay, Reverb, Comp, master);
- ORBIT (`OrbitChords`, `OrbitRing`, scenes, transport);
- SETTINGS (`Midi`, `Theme`, `Tempo`, the settings sheets);
- MIDI MAP (bindings);
- the Part mod matrix (MTX is `route … next-slice`).

On the engine pages, the Part MOD module is drawn by PatchBay and MatrixGrid from routes, and shows "THE UNIT DOESN'T DESCRIBE ITS MOD MATRIX YET" until the routes exist. DRUM's pad routes are ordinary params, so its ROUTE patch bay is live now.

## Glyphs

**What the screens need:** the unit's own art, updated as values move: the ALGO chart morphing with MORPH, the VA wave as WAVE turns, the filter curve, the envelope and LFO shapes, the TAPE gauge, and the DRUM machine icons.

| | v6's cropped PNGs | The firmware's Rust renderer in WASM (owner's preference) |
|---|---|---|
| Fidelity | Exact for the frames that were cropped | Exact for every value: it is the firmware's own code (`Renderer`, `glyph.rs`, `alg_layout`, `kit_band`, `lfo_glyph`, `tape_glyph`, `viz`) |
| Moves with values | No: one still per variant. The ALGO chart and VA wave were redrawn in JS in v6, a second copy that drifts | Yes |
| Drift | Re-crop after every glyph change, by hand | None: one source, rebuilt with the firmware |
| Cost | ~75 PNGs on the site | A wasm32 build of chimera-core plus a small export crate. chimera-core already builds for wasm32-unknown-unknown with its default features (checked 2026-11-19, no changes needed). With `opt-level = "z"` and LTO it is likely a few hundred KB, measured in the plan |
| Elm side | `img` tags | A `<chimera-screen>` custom element that the port shim defines. Elm sets its attributes (engine, page, region, and the values it shows), and the element paints a canvas from the WASM. No port round trip and no Elm-side pixels |

**Recommendation: WASM.** It is the only route that meets "speaks the box's language" while values move, and it removes a copy, which is the class of drift the owner rules out. The crate (`chimera-screen`) holds a headless `UiState` mirror:

1. The editor forwards each `val` line (address, stored value) it pulls with `ed since` into the mirror.
2. `render(page, region)` draws that page's screen region with the firmware's own `Renderer` into an RGB565 buffer.
3. The element blits the buffer.

Until the crate lands, `Ui.Screen` falls back to v6's crops, so pages never wait on it.

## Testing

- **Unit tests:** elm-test for every component and its width contract; the layout tables against the five schema fixtures (every described address placed or in OTHER, no address placed twice); variant selection; and DRUM's KIT/PAD views.
- **Review:** elm-review with the full rule set (`editor/review`), every `let` annotated, and no catch-alls.
- **Simulator:** the editor keeps working against `just editor-sim` (WebSocket) and `just editor-e2e` throughout.
- **Firmware:** if any firmware changes, `just check` in full, with the bank rules applied.
