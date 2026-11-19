# Editor UX research: how synth editors handle depth

2026-11-18. This is input for Chimera's Elm browser editor. It reports patterns, with sources. Anything marked **unverified** comes from screenshots or forum posts, or could not be confirmed at all.

Owner constraints the findings are judged against:

- big, bold web controls rather than tiny knobs;
- settings that are not drawn as switches or dials;
- long vertical pages with sections you can show or hide;
- no sideways scrolling, and no section wider than half the screen;
- start from Overbridge.

## 1. Elektron Overbridge 2 plugin editors

Primary source: *Overbridge User Manual*, 2026-08-26 ([PDF](https://www.elektron.se/wp-content/uploads/2026/08/Overbridge-User-Manual_260826.pdf)). Page numbers below are printed page numbers. The manual text says little about layout, so most layout facts come from reading its editor screenshots: Analog Four p.17, Digitakt II p.27, Digitone II p.29, and browsers pp.18, 26, 27.

**Window structure**
- **Two top-level views.** Each plugin has two tabs in the top bar: *Kit Editor* and *Sound/Preset Browser* (pp.17–29, e.g. §6.9).
- **One tab per track or bus.** Below the top bar is a row of tabs, one per track plus buses:
  - Digitone II: Kit, TRK 1–16, Send FX, Mixer (§6.9.1, p.29).
  - Digitakt II: tracks, Send FX, Mixer, plus Assign Samples (p.27).
  - Analog Four: Kit, Track 1–4, FX, CV (§6.2.1, p.17).
- **Track tabs carry the Sound name.** On the A4 a tab reads "Track 1 - HEAVYMOOGER" (screenshot, p.17).
- **The tab is the track selector.** There is no separate track picker.

**Section grid**
- **Panels laid out by signal flow.** Each track page is a fixed grid of titled panels. On the A4 the controls "roughly follow the signal path of the sound, top-down, left to right in the first two of the three main columns. The third column shows the controls for the envelopes, LFOs, and modulators" (p.17).
- **Panel names.** Every panel has a small uppercase header: OSCILLATORS, FILTERS, AMP, ENVELOPES, LFOS, MODULATORS, SOUND SETTINGS (p.17). Digitone II adds SYNTH, BASE / WIDTH FILTER, FILTER, AMP, MODULATION, FX, PRESET and SETUP (p.29).
- **The page never scrolls.** Every panel is visible at once.

**How Overbridge shows and hides.** No kit editor panel collapses. Hiding happens in four ways:
1. **Sub-tabs in the panel header** swap that panel's contents:
   - A4: OSC 1 / OSC 2 / COMMON, FILTER 1 / FILTER 2, AMP / FILTER / USER envelopes, LFO 1 / LFO 2, and VEL / PB / MW / BRE / AT modulators (p.17).
   - Digitone II: MODULATION = LFO1 / LFO2 / LFO3 / KEY TR. / PITCH BEND / VELOCITY / MOD WHEEL / BREATH / AFTERTOUCH. SETUP = TRK / VOICE / PORTAMENTO / LAYER (p.29).
2. **A type menu in the panel header changes which parameters the panel holds.**
   - Digitone II: SYNTH has "Machine: FM Tone" and FILTER has "Lowpass 4" (p.29).
   - Digitakt II: SAMPLE has "Machine: One-shot" and FILTER has "Multi-Mode" (p.27).
   - Rytm "exhibits different synth engine parameters depending on the chosen Drum Track" (§6.3.1, p.18).
3. **Window-level toggles:**
   - *SHOW/HIDE KEYBOARD* opens a keyboard or pads strip at the bottom (§6.1.4, p.16).
   - Device settings open in a sidebar from an icon at top right (§6.1.5, p.16; §6.10, p.30).
   - The browser has a *HIDE FILTERS* toggle that folds away its tag grid (screenshots pp.18, 26, 27).
4. **Rarely used setup lives on the Kit tab, not on the track page.**
   - Macros are edited there: "go to the Kit tab and click the BD 1 Scene pad … (Edit)" (§6.1.3, p.15).
   - Assignment is a right-click on any parameter, which lists the LFO, velocity, mod-wheel and macro destinations (§6.1.3, pp.15–16).

**How settings-type parameters are drawn** (screenshots, pp.17, 27, 29)
- **Long enums use drop-downs.** Examples: Play Mode "Poly", Mono Note Prio "Last", Velocity to Volume "Exponential", LFO Destination, Sample Play Mode "Forward".
- **Short enums use a row of text words, with the active one lit.** LFO trigger mode is FREE TRIG HOLD ONE HALF. Phase reset is OFF ALL C A+B A+B2. RAM bank is A–H.
- **Waveforms use an icon row.** This applies to LFO and oscillator shapes.
- **Two-way routing uses a radio pair.** For example "Pre Filter / Post Filter".
- **Some ranges are horizontal bars instead of knobs.** For example Pitch Bend Depth, Octave Offset and Track Level.
- **Every control shows its value.** Each knob or bar has a numeric box under it.
- **Envelopes are drawn as graphs.** Below each graph sits a row of numeric fields (Delay, Attack, Decay, …).
- **The Digitone algorithm** is a row of numbers 1–8 above a drawing of the operator graph, with ◀ ▶ arrows (p.29).

**Scaling**
- **Zoom is a percentage menu next to the device name.** Screenshots show 80 %, 100 %, 120 % and 200 % (pp.17, 27, 29).
- **Zoom scales the whole fixed layout.** Nothing reflows.
- **The Control Panel has its own zoom** (p.11, item 6).
- **Zoom may not persist (unverified).** Users reported the plugin window "always reverting back to 100%" when reopened ([Elektronauts, OB 2.0.16 beta thread](https://www.elektronauts.com/t/overbridge-2-0-16-public-beta/95298/40)).

**Known weakness.** When OB2 put the A4's oscillators behind OSC 1/OSC 2 sub-tabs, users called it "a step back in usability". They wanted to "see both Osc and both Filters at the same time" and said the AMP panel wasted space ([Elektronauts: A4 OB1 vs OB2 GUI](https://www.elektronauts.com/t/a4-overbridge-1-vs-overbridge-2-gui/107866)). Sub-tabs hide exactly the things people want to compare.

**Lessons for Chimera**
- **Keep:**
  - one tab per Part, with the Sound name on it;
  - panels in signal order with plain uppercase titles;
  - a header menu that swaps a panel's contents (engine, filter type);
  - a value readout on every control;
  - text-word rows for short enums;
  - right-click to assign modulation.
- **Change:**
  - stack panels vertically and let them scroll, instead of a fixed grid scaled by zoom;
  - put parallel things (OP1–OP6, LFO1–LFO3) next to each other rather than behind sub-tabs.

## 2. Other hardware editors

| Product | What helps our problems | Source |
|---|---|---|
| **Novation Components** (Peak/Summit/Circuit) | Runs in the browser over Web MIDI (Chrome recommended), as editor, librarian and firmware updater in one page. This is the same model as Chimera's site. **Its page layout was not verified**: the support pages returned 403. | [Novation support](https://support.novationmusic.com/hc/en-gb/articles/360009550480) |
| **Korg wavestate / modwave editor** | "A full-screen visual representation of all the parameters", with visual feedback on modulation. Motion sequences can be seen and edited ("see how the motion sequencing works"). Lane layout **unverified**. opsix has only a Sound Librarian. | [gearnews](https://gearnews.com/?p=106055), [Korg download](https://www.korg.com/us/support/download/software/0/840/4808/) |
| **Arturia MiniFreak V** | Three top-level views: *Main* (mirrors the panel), *Advanced* (voices, scales, mod matrix; you open it to "unlock" it) and *Sequencer*. The interface can be resized. Depth sits behind one reveal, not many. | [Arturia MiniFreak V](https://www.arturia.com/products/software-instruments/minifreak-v/overview) |
| **ASM Hydrasynth** | The official ASM Manager is a librarian and updater, "not a full editor". A third-party **web** editor exists ([Synthmad](https://www.synthmad.com/hs-editor/)), but its layout is **unverified**. | [gearnews](https://www.gearnews.com/free-hydrasynth-editor-synth/) |
| **Modal (MODALapp)** | One app on all platforms, talking both ways ("hardware react[s] immediately"). Layout **unverified**. | [Modal](https://www.modalelectronics.com/modalapp/) |
| **Sequential / Oberheim** | No official editor found. The OB-X8 editor is SoundTower's; others are TouchOSC templates. | [Oberheim OB-X8](https://oberheim.com/products/ob-x8/) |
| **Dreadbox Nymphes** | Third-party editors only. *Blue and Pink* moves its sliders when the hardware moves and adds a visual chord editor. That is the pattern of using the editor to fix what the panel makes hard to program. | [Synth Anatomy](https://synthanatomy.com/2025/03/scott-lumsden-blue-and-pink-the-dreadbox-nymphes-editor-we-have-been-waiting-for.html) |
| **UDO Super 6, Polyend, 1010music, Waldorf** | No official editor layout could be verified. Not included. | none |

Takeaway: among first-party hardware editors, only Elektron (Overbridge) and Novation (Components, in the browser) are deep and documented. The rest are librarians or third-party editors.

## 3. Software synths

- **Ableton Live devices.**
  - Double-clicking a device's title bar, or choosing *Fold*, collapses the device "to save space".
  - Some devices have a triangle in the title bar that shows or hides an extra section, such as Phaser-Flanger's LFO.
  - An *Activator* toggle on each device works like "temporarily deleting it".
  - Source: [Live 12 manual ch.23](https://www.ableton.com/en/live-manual/12/working-with-instruments-and-effects/).
- **Bitwig.**
  - "To minimize/restore a device's interface: double-click the device header" ([user guide](https://www.bitwig.com/userguide/latest/working_with_devices)).
  - Each device has separate buttons that show its Remote Controls pane and its Modulators pane, plus an *Expanded Device View* ([Device Panel](https://www.bitwig.com/userguide/bws44-504/the_device_panel)).
  - Modulators are a per-device sub-section you can toggle on its own. That is the "hide the modulation section" request, one level down.
- **Phase Plant.**
  - Generators are stacked vertically, up to 32 per area.
  - Generator groups open and close (the docs show "open and closed generator groups"; how is undocumented).
  - Modulators live in a separate area along the bottom.
  - New modules go in a "large dashed area that appears at the bottom".
  - Source: [Kilohearts docs](https://kilohearts.com/docs/phase_plant).
- **Serum 2.**
  - The main views are tabs: Osc, Mix, FX, Matrix, Global.
  - The mod matrix lets you remove, bypass and reorder each modulation.
  - FX modules are reordered by dragging.
  - Source: [Serum 2 What's New PDF](https://static.xferrecords.com/Serum%202%20What's%20New.pdf).
- **Vital.**
  - "Drag and drop workflow" for modulation, with a preview before you commit ([vital.audio](https://vital.audio/)).
  - Controls are animated.
  - The matrix exists alongside drag-to-assign (forum, **unverified** detail).
- **Pigments.**
  - A strip across the middle holds every mod source.
  - You drag a source onto a parameter, and each source shows up as a ring on that parameter.
  - Tabs under the strip, colour-coded to the sources, open each modulator's editor.
  - Sources: [MusicRadar](https://musicradar.com/news/guide-arturia-pigments-4), [AudioTechnology review](https://audiotechnology.com/?p=36088).

What recurs:
- device-level fold (Live, Bitwig);
- modulators as their own area, or as rings on the parameters they move (Phase Plant, Pigments, Vital);
- a matrix list as the full view of every route (Serum, Vital).

## 4. Web-native patterns

- **Accordions (NN/g)**
  - Avoid them "when your audience needs most or all of the content on the page".
  - Allow several sections open at once.
  - Keep each section's state "until the user changes it".
  - Make headings descriptive enough to be worth clicking.
  - Source: [NN/g](https://www.nngroup.com/articles/accordions-complex-content/).
- **Accordions (GOV.UK)**
  - "Do not put accordions within accordions".
  - Don't disable a section; remove it or explain it.
  - Open state is remembered per session.
  - If a summary line is needed, keep it short.
  - Try headings or anchor links first.
  - Source: [GOV.UK accordion](https://design-system.service.gov.uk/components/accordion/).
- **Toggles (NN/g)**
  - A switch should take effect immediately.
  - Use radio buttons for one-of-many, and don't mix instant switches with Submit forms.
  - Labels should read correctly with "on/off" appended.
  - Source: [NN/g toggle guidelines](https://www.nngroup.com/articles/toggle-switch-guidelines/).
  - Chimera applies every change live, so instant controls are correct. But the owner doesn't want switches for settings, so render two-state values as a two-word choice (OFF | ON, POLY | MONO).
- **Material 3 segmented buttons and density** (pages render client-side; their text could not be fetched, **unverified** here). The commonly cited guidance is 2–5 segments, with a menu beyond that.

## 5. Patterns ranked by fit

1. **Sections in signal order with plain titles, stacked vertically** (Overbridge, Live, Phase Plant). This is the backbone.
2. **A type menu in the section header that swaps the body** (Overbridge Machine and Filter type). Fits engine choice per Part, filter type and FX type. Rows that don't apply disappear instead of greying out (GOV.UK: don't disable).
3. **Fold a section to its header, and remember it** (Live and Bitwig double-click; NN/g persistence). A folded header should still show 2–4 key values (e.g. "FILTER · 2040 · 3.2 kHz · RES 41"), so folding hides controls without hiding the state.
4. **A sticky header with Part tabs and section chips.** It holds Part tabs carrying Sound names (Overbridge) and one chip per section that shows or hides it page-wide (Bitwig pane toggles). One click hides MOD on every Part.
5. **A value readout on every control** (Overbridge). On the web, make the readout the control: a wide bar with the value printed large, drag to change, click to type.
6. **Words for enums.** Use a row of words for ≤5 options (Overbridge FREE TRIG HOLD ONE HALF), a menu or list beyond that, and icon rows only for waveforms.
7. **Parallel units side by side, not behind sub-tabs** (the lesson from the A4 OB2 complaint). Put OP1–OP6, LFO1–3 and envelopes in a two-column grid inside the half-width limit.
8. **Right-click a parameter to assign modulation, plus a matrix list as the full view** (Overbridge quick assign, Serum, Vital). Mark modulated parameters with a ring or tick on their bar (Pigments).
9. **Rarely used setup goes to its own tab** (Overbridge Kit tab, MiniFreak *Advanced*). For SETTINGS and MIDI MAP.

## 6. Candidate directions

All three satisfy the owner's constraints:
- a CSS grid with at most two columns, each section `max-width: 50vw`;
- one column below about 900 px;
- no horizontal scroll.

### A. "Overbridge, unfolded"

This is Overbridge's panel set, stacked and scrollable, with header menus and folding.

```
[P1 GLASS BELL][P2 RUBBER][P3 …][P4][P5][P6] | MIX  FX  ORBIT  SETTINGS     (sticky)
 show: [ENGINE] [FILTER] [AMP] [MOD] [FX SEND]       zoom: S M L

▾ ENGINE ─────────── type [ALGO ▾] ─┐ ▾ OPERATORS ──────────────────────┐
  ALGORITHM  ◀ 7 ▶  (graph drawing) │  OP1 RATIO ████░░ 2.00  LVL ███ 88 │
  FEEDBACK   ██████░░░░  41         │  OP2 RATIO ██░░░░ 1.00  LVL ██░ 62 │
                                    │  … OP3–OP6, same rows               │
▾ FILTER ──────────── type [2040 ▾] ┐ ▸ AMP · ATK 3 · REL 64 · LVL 100    (folded)
  CUTOFF  ████████░░░  3.2 kHz      │
  RES     ████░░░░░░  41            │
▸ MOD · 3 routes · LFO1 → CUTOFF     ▸ FX SEND · REV 31 · DLY 0            (folded)
```

- **Pros:** closest to the owner's starting point; reads as a signal path; folded headers keep state visible.
- **Cons:** two columns of signal flow can be read in the wrong order. Engine-specific sections need careful pairing so columns stay balanced.

### B. "Rack lanes"

Sections are full-width lanes (each still ≤ half the screen), one under another, in Live/Bitwig style. Each lane has an on/off word and a fold arrow. Modulation is not a section: each modulated bar gets a ring, and a MOD drawer opens per parameter.

```
▾ ENGINE  [ALGO ▾]      ON
   ALGORITHM ◀ 7 ▶   FEEDBACK ██████░░ 41
   OP1 … OP6 (one row each)
▾ FILTER  [2040 ▾]      ON
   CUTOFF ████████░░ 3.2 kHz ◉      ← ◉ = modulated; click opens:
     └ LFO1  ███░░ +22   VELOCITY ██░ +10   [+ ADD SOURCE]
▸ AMP  · ATK 3 · REL 64
```

- **Pros:** reads in one order; modulation sits next to what it moves; hiding MOD is free because rings stay out of the way.
- **Cons:** wide screens leave an empty half; pages get longer. Seeing every route at once needs a separate MATRIX section.

### C. "Settings-sheet"

Every page is a list of rows: label on the left, value as large text, editing inline. Ranges are bars and enums are words. The look is a typeset settings sheet, close to the unit's own Digitakt-style SETTINGS. Sound pages use the same rows grouped under sticky section headings, with a table-of-contents chip bar.

```
SETTINGS                                    jump: MIDI · CLOCK · DISPLAY · PROJECT
── MIDI ───────────────────────────────
  CHANNEL P1        1   2   3 … 16 → [ 1 ▾ ]
  CLOCK IN          OFF | USB | DIN
  PROGRAM CHANGE    OFF | RECEIVE | SEND
── DISPLAY ────────────────────────────
  BRIGHT            ██████████░░░  70
  ACCENT            TEAL ▾
```

- **Pros:** strongest answer to "settings aren't switches and dials"; the most readable; trivial to make responsive; one row type to build in Elm.
- **Cons:** sound pages lose the drawings (envelopes, algorithm) unless rows are mixed with drawings; less "instrument" character.

**Suggested blend.** Use A for Part pages, C for SETTINGS and MIDI MAP, and B's modulation rings plus per-parameter MOD drawer inside A's MOD section.

## 7. What to avoid

- **Sub-tabs for parallel units.** OB2's OSC 1/OSC 2 tabs drew the "step back" complaint.
- **Fixed-size canvases scaled by zoom.** They don't reflow, and zoom reportedly doesn't stick. Use the browser's own zoom and responsive CSS instead.
- **Accordions inside accordions** (GOV.UK). One level of folding only.
- **Hiding state when folding.** A folded section should still say what it is set to.
- **Greyed-out parameters for the wrong engine.** Swap the body, as Overbridge's Machine menu does.
- **Knob skeuomorphism and tiny value boxes.** Overbridge's 10-px value boxes only work because the whole window is zoomed.
- **Switch widgets for settings, and switches mixed with forms that need saving** (NN/g). Every change is live, so use words.
- **Modulation reachable only through a matrix.** Every product that handles modulation well also offers assignment at the parameter: right-click or drag.
