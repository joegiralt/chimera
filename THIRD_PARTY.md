# Third-party files

Chimera is licensed under the GNU General Public License, version 3 or (at
your option) any later version (`LICENSE`; SPDX `GPL-3.0-or-later`). The
code below comes from others or follows their published work. Every licence
here is compatible with GPLv3, and each upstream notice is kept in its file.

Most of it is permissive (MIT, BSD-3-Clause, Apache-2.0, CC BY 4.0, the Sun
notice), so it can also follow the author's own code into a closed product
(ADR 0271 § 3) as long as its notice goes with it. **One entry cannot:** the
reverb values derived from thement/midiverb_emulator are GPL-3.0, so they may
only ship under the GPL and must be re-derived before ROOM, BLOOM or LOOP go
into any closed or dual-licensed product.

- `chimera-engine/src/dsp/modal/` (`mod.rs`, `rings.rs`, `dispersion.rs`, `chords.rs`): see ADR 0032 for the Rings and stmlib (MIT) notice. `chords.rs` holds Rings' chord table (`dsp/part.cc`).
- Host-only crates (`chimera-desktop`, never linked into the firmware), used by the TAPE importer (TAPE spec § 6):
  - `hound` 3.5 (WAV reading): Apache-2.0. https://github.com/ruuda/hound
  - `rubato` 5.0 (sinc resampler): MIT OR Apache-2.0. https://github.com/HEnquist/rubato. It pulls in `audioadapter`, `audioadapter-buffers`, `audioadapter-sample` (MIT OR Apache-2.0), `windowfunctions` (MIT), `audio-codec-algorithms` (0BSD OR Apache-2.0), `visibility` (Zlib OR MIT OR Apache-2.0) and `num-traits`, `num-integer` (MIT OR Apache-2.0).
  - `rustfft` 6.4 (the transwave mips' FFT truncation): MIT OR Apache-2.0. https://github.com/ejmahler/RustFFT. It pulls in `num-complex`, `primal-check`, `strength_reduce`, `transpose` (MIT OR Apache-2.0), `num-traits`, `num-integer` and, to build, `autocfg` (MIT OR Apache-2.0).
- `chimera-engine/src/dsp/grain/core.rs` (the grain core: pulse-train clock, bell window, stealing fade and scheduler, Hermite read; TAPE STRETCH, ADR 0204): adapted from Mutable Instruments Beads (`beads/dsp/`, https://github.com/pichenettes/eurorack), MIT, Copyright 2017 Emilie Gillet; the notice is in the file.
- `chimera-engine/src/dsp/mod.rs` (`fast_tanh`): stmlib's `SoftClip` (`dsp/dsp.h`, MIT, Copyright 2012 Emilie Gillet; ADR 0097).
- `chimera-engine/src/dsp/grain/core.rs` and `chimera-engine/src/dsp/delay/grain.rs` (the grain core and the delay TYPE GRAIN; ADR 0202): an adaptation of Mutable Instruments Beads' grain engine (`beads/dsp/granular_player.h`, `delay_grain.h`, `grain_envelope.h`, `voice_stealing_envelope.h`, `trigger_generator.cc`, `granular_processor.h`, `delay_line.h`'s Hermite read), MIT, Copyright 2017 Emilie Gillet, https://github.com/pichenettes/eurorack; notice in the file.
- `chimera-engine/src/dsp/filter/coef.rs` (`tan_accurate`): stmlib's `FREQUENCY_ACCURATE` polynomial coefficients (`dsp/filter.h`, MIT, Copyright 2014 Emilie Gillet; ADR 0096).
- `chimera-engine/src/dsp/rev/` (`room.rs`, `bloom.rs`, `loop_engine.rs`; ADR 0095): ROOM, BLOOM and LOOP start from delay and coefficient values derived from thement/midiverb_emulator (GPL-3.0). The parameterizable engine built around them is original to Chimera. Upstream: https://github.com/thement/midiverb_emulator. Those values are the only part taken from it: the tables of delay lengths, allpass and tap positions and their coefficients, plus HALL's `DIFFUSER_MS`; no emulator code is in the tree, and the A/B tool (`chimera-utils`) only runs the emulator from outside the repo. GPL-3.0 is the project's own licence, so the firmware may carry them; **they cannot go into a closed product**, dual-licensed or not, until they are re-derived.
- `chimera-engine/src/dsp/va/lbfm.rs` (VA MODE LOOPFM; ADR 0162, 0166): Tamara Smyth, "Loopback Frequency Modulation Using a Time-Varying Delay Line", DAFx-26, paper 36, https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_36.pdf, licensed CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Adapted: the closed form (eq. 25) reimplemented in our own Rust with our own b0 curve, normalisation, pitch-dependent b0 cap and per-block ramps (the b0 mapping, eq. 26, only as a test's reference); no code or text from the paper is copied.
- `chimera-engine/src/dsp/filter/sem.rs` (filter TYPE SEM; ADR 0192) and `chimera-circuits/src/sem1a/` (its circuit lab, kept; ADR 0196): L. Köper, M. Holters, F. Esqueda, J. D. Parker, "A Virtual Analog Model of the EDP Wasp VCF", Proc. DAFx20in22, Vienna 2022, pp. 17–24, licensed CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Adapted: its nodal Newton method for the circuit lab's reference and its measured CA3080 fit (α, β; Table 3), which sets the integrators' tanh scale; our own model and Rust, derived from the Oberheim SEM-1A schematic and the parts' datasheets; no code or text from the paper is copied.
- `chimera-circuits` (host only, never linked into the firmware; ADR 0196), adapted code with its notice kept in the file:
  - `src/util/sig.rs` (`resample_poly`, `firwin_kaiser`, `kaiser`): SciPy's `scipy.signal` filter design, padding and alignment, BSD-3-Clause, Copyright 2001–2002 Enthought, Inc., 2003 SciPy Developers. https://github.com/scipy/scipy
  - `src/plaits_lpg/mod.rs`: an adaptation of Plaits' LPG (`plaits/dsp/fx/low_pass_gate.h`, `plaits/dsp/envelope.h` LPGEnvelope, the mapping in `plaits/dsp/voice.cc`, and stmlib's `Svf` with `FREQUENCY_DIRTY`, `dsp/filter.h`), MIT, Copyright 2014–2016 Emilie Gillet, https://github.com/pichenettes/eurorack; notice in the file. A behaviour reference for the 292B only.
- `chimera-circuits/src/vtl5c3/` and `src/buchla292b/` (host only; ADR 0196): the vactrol model of J. Najnudel, R. Müller, T. Hélie, D. Roze, "Power-balanced dynamic modeling of vactrols: application to a VTL5C3/2", Proc. DAFx-23, Copenhagen 2023, https://www.dafx.de/paper-archive/2023/DAFx23_paper_50.pdf, licensed CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/). Adapted: its equations and Table 1 reimplemented in our own Rust, the VTL5C3's rates refit by us to the PerkinElmer data sheet; no code or text from the paper is copied. J. Parker, S. D'Angelo, "A digital model of the Buchla lowpass-gate", DAFx-13: equations only.
- `chimera-engine/src/dsp/lpg.rs` (the VCA's low-pass gate, AMP's LPG, DECAY and STRIKE; ADR 0195): an adaptation of Mutable Instruments Plaits' low-pass gate, `plaits/dsp/fx/low_pass_gate.h`, the `LPGEnvelope` of `plaits/dsp/envelope.h`, the decay and colour mapping of `plaits/dsp/voice.cc` and stmlib's `Svf` (`dsp/filter.h`, FREQUENCY_DIRTY), github.com/pichenettes/eurorack, MIT, Copyright 2014–2016 Emilie Gillet. The MIT notice is kept in the file.
- `chimera-engine/src/dsp/delay/tape.rs` (`tanhf`, `expm1f`: TAPE's saturation, inlined bit for bit, #793): libm 0.2.16's (https://github.com/rust-lang/libm, MIT OR Apache-2.0, Copyright (c) 2018 Jorge Aparicio; musl: Copyright © 2005-2020 Rich Felker, et al.), from musl and FreeBSD msun's `s_tanhf.c` and `s_expm1f.c`, Copyright (C) 1993 Sun Microsystems, Inc., freely granted provided the notice is preserved; the notice is in the file.
- `chimera-utils/src/stim.rs` and `src/sig.rs` (host only; the A/B tool's stimuli and signal helpers): adapted from NumPy (`SeedSequence`, `random_standard_normal`, `next_double`, `random_interval`, `Generator.shuffle`; BSD-3-Clause, Copyright (c) 2005-2024 NumPy Developers, Copyright (c) 2019 Kevin Sheppard), PCG64 (Apache-2.0 or MIT, Copyright 2014 Melissa O'Neill) and SciPy (`resample_poly`, `butter`, `sosfilt`, `welch`; BSD-3-Clause, Copyright (c) 2001-2002 Enthought, Inc., 2003 SciPy Developers); each notice is in the file (`chimera-utils/README.md` § Third-party notices).
- `chimera-hal/src/panel.rs` (the ILI9341 panel's init list): martnak/STM32-ILI9341 (MIT, Copyright (c) 2017 martnak, https://github.com/martnak/STM32-ILI9341) via PreenFM3, whose `lib/Src/ili9341.c` (`ILI9341_Init`) says its list is based on it; the MIT notice is in the file.

- The screen's fonts (`chimera-core/src/ui/theme.rs`), linked into the firmware through the `u8g2-fonts` crate (MIT OR Apache-2.0). The font data is not under the crate's licence: each font keeps its U8g2 font licence (https://github.com/olikraus/u8g2/blob/master/LICENSE, https://github.com/olikraus/u8g2/wiki/fntgrp).
  - `helvR08`, `helvR10`, `helvB08`, `helvB10`, `helvB14` (U8g2's X11 group, from the X11 `HELV*.BDF` files; https://github.com/olikraus/u8g2/wiki/fntgrpx11). Their notice:

    > Copyright 1984-1989, 1994 Adobe Systems Incorporated.
    > Copyright 1988, 1994 Digital Equipment Corporation.
    >
    > Adobe is a trademark of Adobe Systems Incorporated which may be registered in certain jurisdictions. Permission to use these trademarks is hereby granted only in association with the images described in this file.
    >
    > Permission to use, copy, modify, distribute and sell this software and its documentation for any purpose and without fee is hereby granted, provided that the above copyright notices appear in all copies and that both those copyright notices and this permission notice appear in supporting documentation, and that the names of Adobe Systems and Digital Equipment Corporation not be used in advertising or publicity pertaining to distribution of the software without specific, written prior permission. Adobe Systems and Digital Equipment Corporation make no representations about the suitability of this software for any purpose. It is provided "as is" without express or implied warranty.

  - `logisoso20`, `logisoso42` (U8g2's Logisoso group; https://github.com/olikraus/u8g2/wiki/fntgrplogisoso): "Created by Mathieu Gabiot with FontForge 2.0 (http://fontforge.sf.net) - Brussels - 2009". openfontlibrary.org lists it under the SIL Open Font License; the COPYING.TXT in its archive reads:

    > Copyright (C) 2011 Mathieu-g (Mathieu Gabiot).
    >
    > This Font Software is an open font and is released under the GPL v2 with font exception; you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation.
    >
    > This Font Software is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
    >
    > As a special exception, if you create a document which uses this font, and embed this font or unaltered portions of this font into the document, this font does not by itself cause the resulting document to be covered by the GNU General Public License. This exception does not however invalidate any other reasons why the document might be covered by the GNU General Public License. If you modify this font, you may extend this exception to your version of the font, but you are not obligated to do so. If you do not wish to do so, delete this exception statement from your version.

- `chimera-core/src/ui/yellow_sign.rle` (the boot splash, `chimera-core/src/ui/splash.rs`): the Yellow Sign designed by Christian Knutsson for Pelgrane Press, which released it into the public domain in 2020 ("Use it personally or commercially"): https://pelgranepress.com/2020/02/06/our-yellow-sign-is-your-yellow-sign/. Attribution is not required; it is credited here with thanks.

## Chimera's own work

Everything in this repository that is not listed above was written for Chimera by Joe Giralt with Claude (AI agents he directs), under GPLv3 or later. Some items below are listed because they resemble or were inspired by other work; that inspiration is design only (taken from a manual, a paper or a published description), and no code was taken.

### Engines

- ALGO, the FM/PM engine (`chimera-engine/src/dsp/algo/`): design inspired by the Yamaha TX81Z (from its manual); the formant operators follow the structure of the Yamaha FS1R's formant synthesis, from its expired patent US 5,610,354; no code.
- Wave tables and recipes (`chimera-waves/`, `chimera-engine/src/dsp/algo/waves.rs`): generated from Chimera's own recipes; WAVE's approach is inspired by the PPG Wave and Waldorf Microwave wavetable synths (published descriptions); no code and no tables.
- MODAL, the physical-modelling engine (`chimera-engine/src/dsp/modal/`: `body.rs`, `ensemble.rs`, `loop_parts.rs`, `params.rs`): design inspired by Mutable Instruments Rings and Elements and by the Korg Volca Drum; the files that do hold Rings code are listed above.
- The MODAL string (`chimera-engine/src/dsp/modal/string.rs`, the Karplus-Strong string): the author's own code from his Carcosa firmware for the Ambika (`voicecard/karplus.h`), brought in by its author. The original Ambika firmware has no Karplus-Strong engine and none of its GPL code was copied (#56, ADR 0032).
- VA (`chimera-engine/src/dsp/va/`: `osc.rs`, `blep.rs`, `stability.rs`, `combine.rs`, `lbfm.rs`, `noise.rs`, `osc/`): design inspired by the Mutable Instruments Ambika's architecture (from its manual); no code. Clean room; each file's `GPL_CHECK` says what was read.
- TAPE, the sample engine (`chimera-engine/src/dsp/tape_engine/`, `tape.rs`): Chimera's own player, streaming from the card and prep. Its machines:
  - ONESHOT, LOOP, PING-PONG (`oneshot.rs`, `machine.rs`; the WALK machine): one-shot, looped and ping-pong play.
  - TRON (`tron.rs`): design inspired by the Mellotron M400's tape and run-out (from its published description); no code.
  - STRETCH (`stretch/`): CLASSIC is design inspired by the Akai S950's time stretch (Operator's Manual p. 32); GRAIN uses the grain core listed above.
  - TRANSWAVE (`wave.rs`): a wavetable made from a sample; design inspired by PPG and Waldorf wavetables.
  - SCRUB (`scrub.rs`): a playable scrubbing finger.
  - Tape ageing, drift and warble (`age.rs`, `drift.rs`, `warble.rs`) and the analogue tape stage (`chimera-engine/src/dsp/tape/`).
- DRUM, the drum engine (`chimera-engine/src/dsp/drum/`): design inspired by the Elektron Analog Rytm (architecture and pad scheme, from its manual) and the Elektron Machinedrum (machine structures and knob sets, from its manual); our own algorithms and names; no code. Its twelve machines are TONE KICK, SNARE, TOM, CLAP, COWBELL and CYMBAL, METAL HAT and JINGLE, BODY SHAKER, and PLAIN SINE, NOISE and IMPULSE (`kernel/`), with pad laws, choke groups and voice groups (`laws.rs`, `groups.rs`).
- The drum kit and kit library (`chimera-engine/src/project/kits.rs`, `kit_library.rs`, `chimera-core/src/ui/kit_band.rs`, `kit_groups.rs`): a 12-pad kit per Part, in the Analog Rytm's shape; Chimera's own.

### Voice chain

- The filters (`chimera-engine/src/dsp/filter/`): CLEAN (`clean.rs`, `coef.rs`), a Zavalishin TPT state-variable filter laid out after the Intellijel Cascadia's VCF (from its manual); DIRTY (`dirty.rs`), a Chamberlin SVF at 2x; LADDER (`ladder.rs`), a Moog transistor ladder after Zavalishin's TPT method (ADR 0104); SEM (`sem.rs`), modelled from the Oberheim SEM-1A VCF's circuit (ADR 0192); 2040 (`k2040.rs`), modelled from the SSM2040 four-pole with its REV1 and REV2 variants; EXPNDR (the Kobol's VCO and EXP stages, ADR 0105, 0191); filter feedback, stability and level (`fed.rs`, `stability.rs`, `level.rs`, `probe.rs`). Design only, from the chips' datasheets and the published papers named above; no code.
- The circuit lab (`chimera-circuits/`, host only, ADR 0196): SPICE-checked models and references of the SEM-1A (`sem1a/`), the SSM2040 as the Kobol and the Prophet use it (`ssm2040_kobol/`, `ssm2040_prophet/`), the CEM3320 as the OB-8 runs it (`cem3320_ob8/`), the Buchla 292B with its vactrol (`buchla292b/`, `vtl5c3/`) and the measurement harness (`util/`); `plaits_lpg/` is listed above.
- Per-voice effects (`chimera-engine/src/dsp/voice_fx/`): SOFT drive, FUZZ modelled from the Big Muff Pi's circuit (ADR 0156) and FOLD (`soft.rs`, `fuzz.rs`, `fold.rs`).
- The VCA and low-pass gate (`chimera-engine/src/dsp/lpg.rs`, `voice/gate.rs`, `gate.rs`): the low-pass gate is listed above; the amp gate, depth and stereo placement (AMP BASE, STEREO) are Chimera's own (`voice/depth.rs`, `chimera-engine/src/stereo/`).
- Envelopes and LFOs (`chimera-engine/src/dsp/modulator/`, `lfo.rs`, `envelope.rs`): design inspired by the Intellijel Cascadia's envelopes and modulators (behaviour and ranges from its manual, ADR 0036); no code. Free LFOs run on a Part clock.
- The modulation matrix (`chimera-engine/src/modulation.rs`, `mod_path.rs`, `chimera-core/src/ui/mod_grid.rs`, `mod_panel.rs`): every connection is a route; Chimera's own.
- Glide, DC blocker, ease and halfband (`glide.rs`, `dc_blocker.rs`, `ease.rs`, `halfband.rs`, `ring.rs`): Chimera's own.
- The knob laws (`chimera-engine/src/dsp/knobs.rs`, `params.rs`, `tracked.rs`): the lerped, declared-motion parameter model; Chimera's own.

### Mixer and effects

- The chorus (`chimera-engine/src/dsp/chorus.rs`): Juno I, II and I+II types, design inspired by the Roland Juno-60 chorus (from its published description); the flanger and phaser types are Chimera's own (ADR 0235, 0324); no code.
- The delays (`chimera-engine/src/dsp/delay.rs`, `delay/`): the TAPE delay with wow and flutter and its sync (`delay/tape.rs`, `delay/sync.rs`); the GRAIN delay uses the grain core listed above.
- The reverb engine (`chimera-engine/src/dsp/reverb.rs`, `rev/`): the parameterizable engine (QUAD, ROOM, HALL, BLOOM, GATE, REVERSE; `room.rs`, `bloom.rs`, `loop_engine.rs`, `burst.rs`, `arena.rs`, `parts.rs`, `sync.rs`) is original to Chimera; the values `room.rs`, `bloom.rs` and `loop_engine.rs` start from are credited above. REV DUCK (`duck.rs`) is Chimera's own.
- EQ and SMART EQ (`chimera-engine/src/part_eq/`, `chimera-core/src/ui/smart_eq.rs`, `smart_view.rs`): the per-Part tilt and lo-cut EQ; SMART EQ is design inspired by sonible smart:EQ (from its published description); no code (ADR 0284, 0296).
- The master section (`chimera-engine/src/dsp/master_out.rs`, `comp.rs`, `limiter.rs`, `fx_bus.rs`): trim, compressor, soft-knee limiter and the FX returns (ADR 0030, 0050, 0109).
- The compressor and KEY (`chimera-engine/src/comp_key/`): KEY listens to a range or a Part; design inspired by the Elektron Analog Rytm's compressor sidechain choices (from its manual); no code (ADR 0303).
- DUCK (`rev/duck.rs`, ADR 0295): the return turns down under its own input; Chimera's own.
- Voices and RESERVE (`chimera-engine/src/voice_alloc.rs`, `voice_limit.rs`, `budget.rs`, `instrument/admission.rs`, `sym_admit.rs`, `chimera-core/src/ui/part_voices.rs`, `pool_page.rs`): voice allocation and stealing, per-Part caps and reserves, the CPU and memory bills, shedding (ADR 0015, 0273, 0281).
- Part and mixer plumbing (`chimera-engine/src/part.rs`, `part_peaks/`, `meter.rs`, `taps.rs`, `scope.rs`): Chimera's own.

### ORBIT

- The ORBIT sequencer (`chimera-engine/src/orbit/`, `chimera-core/src/ui/orbit/`, `chimera-board/src/orbit.rs`): a generative polyrhythmic ring sequencer with a chord ring, step rings, scale fitting, probability, scenes and a command channel (ADR 0071 to 0084); design takes the Euclidean-rhythm idea from the published literature (`euclid.rs`) and is otherwise Chimera's own. It lives in bank 2.
- Music theory (`chimera-theory/`): scales, chords and tuning built from types.

### MIDI

- The MIDI core (`chimera-midi/`, `chimera-engine/src/midi_in.rs`, `midi_settings.rs`, `note_queue.rs`): parsing, learn, control laws, SysEx maps, USB-MIDI classes and the typed controller model (ADR 0217).
- MIDI maps (`chimera-engine/src/midi_map/`, `chimera-core/src/ui/midi/`, `settings/midi_map.rs`): per-Part maps, learn by pointing at a cell, the factory map, maps as card files (ADR 0139, 0226, 0255, 0264, 0288, 0323).
- MPE (`chimera-engine/src/mpe.rs`; ADR 0252): zones and per-note expression.
- NRPN (`chimera-engine/src/nrpn.rs`, `chimera-core/src/ui/param_out.rs`): a frozen NRPN address for every parameter (ADR 0216, 0258).
- SysEx (`chimera-midi/src/sysex.rs`, `sxmap.rs`; ADR 0241): controllers bound by a learned shape and key.
- Tuning and MTS (`chimera-engine/src/tuning.rs`, `scala_name.rs`, `chimera-theory/src/tuning.rs`, `chimera-core/src/ui/tuning.rs`, `tuning_walk.rs`): per-Part tuning tables, Scala files by name (ADR 0240, 0259).

### The UI and screens

- The UI (`chimera-core/src/ui/`): the chain navigation, pages, focus band and hints, cells, the mixer, the filter, FX, reverb, mod and LFO panels, the TAPE strip, the LIBRARY browser, the SETTINGS menu (`settings/`), dialogs, the screensaver and the boot splash (the Yellow Sign is listed above).
- The glyphs (`chimera-core/src/ui/glyph.rs`, `lfo_glyph.rs`, `tape_glyph.rs`, the boot splash's renderer in `splash.rs` (the Yellow Sign artwork itself is credited above), `chimera-engine/src/glyph.rs`, `lfo_glyph/`, `tape_glyph/`, `dungeon_map.rs`, `viz.rs`): every glyph that draws what a control does to the sound.
- The theme and animation (`theme.rs`, `theme_settings.rs`, `animation.rs`, `feel.rs`): the THEME page, ease and encoder feel.
- The desktop simulator (`chimera-desktop/`): the window, audio, controls, display, a fake card and the QA harness.
- The panel layer (`chimera-hal/`, `chimera-board/src/controls.rs`, `display.rs`, `chimera-stm32/`): the traits and the STM32H750 target (SAI, SPI, GPIO, DMA, clocks, watchdog).

### Storage

- chimera-fat (`chimera-fat/`): Chimera's own FAT layer and SD protocol, with its sweep of lost clusters (ADR 0045, 0048, 0094).
- Projects (`chimera-engine/src/project/`): projects own their Sounds, the pool, streamed sources, load protocol and swaps (ADR 0043, 0046, 0087).
- The library and the card format (`chimera-engine/src/storage/`, `project/library.rs`, `chimera-core/src/ui/library.rs`; ADR 0119): Sound files, records, frames and checked sources.
- Preset and the randomiser (`preset.rs`, `randomize/`; ADR 0232).

### USB

- USB audio (`chimera-board/src/usb_audio.rs`, `chimera-midi/src/uac*.rs`; ADR 0248, 0256, 0257, 0260): a UAC1 source for the main out, with playback and feedback.
- USB MIDI (`chimera-board/src/usb_midi.rs`, `usb.rs`, `chimera-midi/src/usb*.rs`; ADR 0243): a cable per Part.
- The console (`chimera-core/src/console/`, ADR 0068, 0092): status, shot, put, flash, card and the `ed` editor protocol (`console/ed/`, ADR 0276, 0285, 0289).

### The bootloader handoff and bank 2

- The bootloader handoff and image installer (`chimera-bootloader/`, `chimera-image/`, `chimera-engine/src/boot.rs`, `chimera-board/src/dfu.rs`, `chimera-core/src/ui/installing.rs`): image headers, judging and slots (ADR 0272, 0314). The stock bootloader is pointed to, not carried.
- Bank 2 and its doors (`chimera-engine/src/bank2.rs`, `chimera-board/src/bank2.rs`, `chimera-core/src/bank2.rs`, `chimera-core/src/console/bank2.rs`): sealed code behind one door (ADR 0137, 0194, 0320).

### Tools

- `tools/chimera-usb.py`: the USB console client (status, flash, put, update).
- chimera-utils (`chimera-utils/`, host only; ADR 0098, 0100): the A/B and reference tool, its stimuli and metrics. The parts adapted from NumPy and SciPy are listed above.
- The export and checks (`tools/oss_export.py`, `headers.py`, `bank1_budget.py`, `bank2_budget.py`, `check_itcm.py`, `loop_count.py`, `stack_depth.py`, `factory_zip.py`, `tools/ab/`, the `Justfile`): the public export and its scan, the licence headers and the budget guards.
- The importer (`chimera-engine/src/import/`, `chimera-desktop/src/bin/chimera-import.rs`): the host-side TX81Z patch importer (ADR 0088); a reader of a public file format, no bank bytes shipped.

### The browser editor's screen kit

- `chimera-screen/` (ADR 0334): the screen kit that draws the pages and the mixer in the browser editor, sharing the firmware's layout; it is checked against the console's `ed` and the simulator's frame.

### The factory Sounds and maps

- The factory Sounds (`chimera-engine/src/factory.rs`, `factory.bin`, `tools/factory-sounds/`, `chimera-desktop/src/factory_sounds/`): Chimera's own. Sounds inspired by the TX81Z's presets are mutated well away from them and carry no bank bytes (ADR 0138, `tools/factory-sounds/ports.toml`).
- The factory TAPE sources (`tools/factory-tapes/`, `chimera-desktop/src/factory_tapes.rs`): Chimera renders, the tool's own synthesis or CC0 recordings, credited in `tools/factory-tapes/`.
- The factory maps (`chimera-engine/src/midi_map/builtin.rs`, ADR 0264, 0266): the flat engine maps and the controller maps; Chimera's own.

### Everything else

- The firmware's board and memory plumbing (`chimera-engine/src/board.rs`, `hw.rs`, `budget.rs`, `block.rs`, `audio_out.rs`, `clock_plan.rs`, `itcm_geometry.rs`, `sym_alloc.rs`, `chimera-stm32/src/audio/`): the clocking, DMA, memory map and the audio callback (ADR 0014, 0020, 0021, 0124).
- The docs (`docs/`, `README.md`, `CONTRIBUTING.md`): Chimera's own text and mockups.
