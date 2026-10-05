# Architecture Decision Records

Why Chimera is built the way it is. One decision per file, never edited after
acceptance except to mark it superseded; a changed decision is a new ADR.
New ADRs use [`0000-template.md`](0000-template.md) and take the next number.

| # | Decision | Status |
|---|---|---|
| [0001](0001-record-decisions-as-adrs.md) | Record architectural decisions as ADRs | Accepted |
| [0002](0002-own-engine-implementations-no-elektron-code.md) | Monomachine-inspired engines are our own implementations | Accepted |
| [0003](0003-keep-faithful-tx81z-fm.md) | Keep the faithful TX81Z 4-op FM engine | Superseded by 0022 |
| [0004](0004-modal-is-the-physical-modeling-engine.md) | Modal is the physical-modeling engine; Rings/Elements are its reference | Accepted |
| [0005](0005-engine-selection-is-patch-based.md) | Engine selection is patch-based | Accepted |
| [0006](0006-sub-project-decomposition.md) | Decompose the pivot into five sub-projects | Accepted |
| [0007](0007-per-block-param-specs.md) | Each block owns its values and a const description | Accepted |
| [0008](0008-persistent-engine-instances.md) | Engines are persistent; never constructed in the audio interrupt | Superseded by [0051](0051-a-voice-holds-one-engine-rebuilt-in-place.md) |
| [0009](0009-semantic-parameter-addresses.md) | Address parameters by what they are, not where they sit | Accepted |
| [0010](0010-modulation-targets-are-honest.md) | Only parameters the voice reads per block are modulatable | Accepted; linear law superseded for CUTOFF by [0035](0035-every-connection-is-a-matrix-route.md), and for PITCH and FINE by [0042](0042-voice-pitch-is-a-matrix-destination.md); superseded in part by [0056](0056-modal-resonators-share-four-macros.md) (Modal's four macros) |
| [0011](0011-goldens-are-a-refactor-lock.md) | Golden recordings are a refactor lock, not a quality claim | Accepted |
| [0012](0012-type-driven-development.md) | Type-driven development where it pays | Accepted |
| [0013](0013-hardware-parity-budgets.md) | The simulator enforces the chip's limits | Accepted; budget clause superseded by 0020 |
| [0014](0014-audio-memory-map.md) | Voices in D2, FX bus in AXI; buffers sized to the range they serve | Accepted; 16-bit-line rejection and FX totals superseded in part by 0028; string sizing by [0040](0040-eight-voices-modal-strings-to-g1.md) |
| [0015](0015-voice-steal-and-fx-returns.md) | Steal released voices first; FX sends return wet only | Accepted; mono-return clause superseded in part by 0029 |
| [0016](0016-visual-direction-refined-elektron.md) | Visual direction: refined Elektron | Accepted; outline-box rule superseded in part by [0041](0041-mod-matrix-amount-grid.md) |
| [0017](0017-prime-status-feedback.md) | MIX+PLUS reports its outcome in place, until the next input | Accepted; hint text superseded by [0086](0086-every-gesture-shows-a-contextual-hint-in-the-focus-band.md) |
| [0018](0018-fm-alg4-follows-tx81z.md) | FM algorithm 4 follows the TX81Z, not p81z | Superseded by 0022 |
| [0019](0019-note-input-per-source-queues.md) | One parser and one single-producer queue per note source | Accepted |
| [0020](0020-audio-clocking-and-output.md) | Clock the chip by silicon revision; derive the cycle budget from it | Accepted; DTCM stack clause superseded in part by 0025 |
| [0021](0021-take-once-triple-buffer.md) | Audio↔UI shared state uses a take-once triple buffer | Accepted |
| [0022](0022-one-algorithmic-engine.md) | One algorithmic six-operator engine replaces Pizza, FM and VA | Accepted; VCA note superseded in part by [0035](0035-every-connection-is-a-matrix-route.md); superseded in part by [0060](0060-one-dc-blocker-per-voice.md) |
| [0023](0023-waves-from-our-own-recipes.md) | The waves come from our own recipes | Accepted |
| [0024](0024-morph-blends-link-weights.md) | MORPH blends link weights; one plan orders both algorithms | Accepted; output scale superseded in part by [0049](0049-algo-init-routed-morph-dims.md) |
| [0025](0025-dtcm-holds-wave-tables-and-stack.md) | DTCM holds the wave tables and the stack | Accepted |
| [0026](0026-algo-voices-billed-by-patch-shape.md) | Algo voices are billed by patch shape; heavy patches get fewer voices | Accepted; shedding clause and four-voice floor superseded in part by 0027; rev V voice counts superseded by [0031](0031-six-voices-on-rev-v.md) |
| [0027](0027-shedding-fades-tails-first.md) | Shedding fades, and takes tails before held notes | Proposed; "rev Y gets two" superseded by [0031](0031-six-voices-on-rev-v.md); cross-Part steal billing amended by [0091](0091-orbit-billed-while-it-plays.md) |
| [0028](0028-one-alesis-style-reverb.md) | One Alesis-style ring is the reverb; Plate, FDN and MidiVerb retire | Accepted |
| [0029](0029-stereo-fx-returns.md) | The reverb and chorus return stereo; the delay stays mono | Accepted |
| [0030](0030-master-section.md) | REV SEND feeds the delay into the reverb; tape on pair 1; a compressor linked across the pairs, last | Accepted; tape superseded in part by [0055](0055-master-tape-off-the-chain.md) |
| [0031](0031-six-voices-on-rev-v.md) | Every patch gets six voices on rev V | Accepted; six as the pool superseded in part by [0040](0040-eight-voices-modal-strings-to-g1.md) |
| [0032](0032-modal-code-provenance.md) | Modal's string is the owner's own code; Rings-derived parts keep Mutable's MIT notice | Accepted |
| [0033](0033-theme-applied-in-the-display-shell.md) | The theme is applied in the display shell, not the renderer | Proposed |
| [0034](0034-watchdog-kicked-by-audio-heartbeat.md) | The IWDG resets on a stalled audio interrupt, kicked from the controls tick | Proposed |
| [0035](0035-every-connection-is-a-matrix-route.md) | Every connection from a modulator to the sound is a matrix route | Accepted |
| [0036](0036-cascadia-style-modulators.md) | ENV slots follow the Cascadia's Envelopes A and B; LFO slots are CLASSIC or FUNC | Accepted |
| [0037](0037-kind-lays-out-the-filter-panel.md) | KIND lays out the filter panel; MODE follows KIND | Accepted; superseded by [0096](0096-filter-types-clean-2040-dirty-and-algo-taps.md) (Proposed) |
| [0038](0038-delay-sat-zero-still-saturates.md) | The delay's feedback loop saturates at every SAT, SAT 0 included | Proposed |
| [0039](0039-six-encoders-no-main-encoder.md) | Six encoders; no main encoder | Proposed |
| [0040](0040-eight-voices-modal-strings-to-g1.md) | Eight voices in D2; Modal strings sized to G1 (supersedes in part 0014 and 0031) | Accepted; line length superseded in part by [0056](0056-modal-resonators-share-four-macros.md); admission and voice counts while ORBIT plays superseded in part by [0091](0091-orbit-billed-while-it-plays.md) |
| [0041](0041-mod-matrix-amount-grid.md) | The mod matrix is an amount grid of outlined cells (supersedes in part 0016) | Accepted |
| [0042](0042-voice-pitch-is-a-matrix-destination.md) | Voice pitch is a matrix destination on each engine's PITCH page | Accepted; `ModalEngine::PITCH` superseded in part by [0056](0056-modal-resonators-share-four-macros.md) |
| [0043](0043-projects-own-their-sounds.md) | Projects own their sounds; marks are derived; one typed replace guard | Proposed; template amended by [0073](0073-new-plays-the-sparse-groove-on-factory-sounds.md) |
| [0044](0044-one-ladder-one-button-map.md) | One ladder and one button map for Parts, the mixer and ORBIT (absorbs 0057; amended by 0066 for SETTINGS) | Proposed |
| [0045](0045-card-format.md) | Store cards in 8.3 A/B files of versioned TLV records | Accepted; load staging superseded in part by [0046](0046-project-load-protocol.md) |
| [0046](0046-project-load-protocol.md) | Swap projects by epoch: fade the voices, then publish (supersedes in part [0045](0045-card-format.md)) | Proposed |
| [0048](0048-own-fat-layer.md) | Own the FAT layer; keep embedded-sdmmc only as the SD block driver | Accepted |
| [0049](0049-algo-init-routed-morph-dims.md) | Algo INIT is audibly routed; MORPH dims when A = B (supersedes in part 0024) | Proposed; INIT's RR and OUT LEVEL noted by [0063](0063-levels-at-the-factory-median.md) |
| [0050](0050-gain-staging-trim-and-final-limiter.md) | Gain staging: a 1/√8 output trim and one peak limiter at −1 dBFS, last | Proposed |
| [0051](0051-a-voice-holds-one-engine-rebuilt-in-place.md) | A voice holds one engine, rebuilt in place (supersedes 0008) | Accepted |
| [0052](0052-strings-stored-as-16-bit-block-float.md) | Store Modal's string delay lines as 16-bit block float | Superseded by [0054](0054-sympathetic-strings-from-a-shared-pool.md) (sympathetic slot pool, never accepted) |
| [0053](0053-delay-mechanics-wow-and-irregular-flutter.md) | The delay's WOW becomes MECHANICS: a slow wow and an irregular flutter | Proposed |
| [0054](0054-sympathetic-strings-from-a-shared-pool.md) | Sympathetic borrows its strings from a shared pool of four (supersedes 0052) | Accepted; voice size assert and `COST_SYMPATHETIC` superseded in part by [0056](0056-modal-resonators-share-four-macros.md) |
| [0055](0055-master-tape-off-the-chain.md) | The master tape stage is off the chain, kept behind `master-tape` (supersedes in part 0030) | Proposed |
| [0056](0056-modal-resonators-share-four-macros.md) | Modal's resonators share four modulatable macros; loops are stable by construction (supersedes in part 0010, 0040, 0042, 0054) | Proposed; superseded in part by [0060](0060-one-dc-blocker-per-voice.md), [0062](0062-modal-notes-ring-free.md), [0064](0064-the-one-loop-bow-in-tune.md) |
| [0057](0057-part-button-toggles-sound-and-mixer.md) | The Part button toggles sound and mixer; the mixer opens on SENDS | Superseded by [0044](0044-one-ladder-one-button-map.md) |
| [0058](0058-a-loudness-reference-modal-matches-algo-init.md) | A loudness reference: every Modal model's INIT plays as loud as ALGO INIT | Superseded by [0063](0063-levels-at-the-factory-median.md) |
| [0059](0059-music-theory-crate-built-from-types.md) | Music theory is its own crate, built from types | Proposed |
| [0060](0060-one-dc-blocker-per-voice.md) | Block DC once per voice, after its last nonlinear stage (supersedes in part 0022, 0056) | Proposed |
| [0061](0061-settings-ease-effects-run-at-mix-0.md) | Every continuous setting eases; an effect runs at MIX 0 | Proposed |
| [0062](0062-modal-notes-ring-free.md) | A Modal note rings free, a re-struck key re-plucks its string, the halo glides (supersedes in part 0056) | Proposed |
| [0063](0063-levels-at-the-factory-median.md) | INITs play at the factory median; BANK and the voice filter stop buzzing (supersedes 0058) | Proposed; BANK's out curve amended by [0097](0097-bank-and-symp-clip-with-stmlibs-curve.md) |
| [0064](0064-the-one-loop-bow-in-tune.md) | Bowed is the one-loop bow again, half-length and inverting, in tune (supersedes in part 0056) | Proposed |
| [0065](0065-steal-cut-or-glide.md) | A Part's own steal cuts, or glides the ring to the new note | Proposed |
| [0066](0066-settings-menu.md) | MENU opens a SETTINGS menu (amends 0044, supersedes 0057 through it) | Proposed |
| [0067](0067-focus-glyphs.md) | Focus glyphs: hand-assigned per parameter, ARC by default; composites draw from set values only | Proposed |
| [0068](0068-usb-console.md) | Read the unit over a polled USB CDC-ACM console | Proposed; amended by [0092](0092-put-files-over-the-usb-console.md) |
| [0069](0069-transpose-staff-glyph.md) | TRANSPOSE takes STAFF: a note on a three-line staff (amends 0067) | Proposed |
| [0070](0070-wave-scope-glyph.md) | WAVE takes a scope traced from its own table (amends 0067) | Proposed |
| [0071](0071-orbit-runs-on-the-audio-thread.md) | ORBIT runs on the audio thread, block-accurate; the engine owns the drift, transport acts on edges | Proposed; edge rule amended by [0080](0080-sync-int-or-ext-a-daw-drives-orbits-transport.md); superseded in part by [0082](0082-orbit-command-channel.md) |
| [0072](0072-orbit-follows-one-midi-clock.md) | ORBIT follows one MIDI clock through the note queues: ticks are places, the time between a guess, never back, never skipped | Proposed; transport amended by [0080](0080-sync-int-or-ext-a-daw-drives-orbits-transport.md) |
| [0073](0073-new-plays-the-sparse-groove-on-factory-sounds.md) | NEW plays the sparse groove on factory Sounds (amends 0043's template) | Proposed |
| [0074](0074-notes-edits-by-chip-clear-rolls-in-key.md) | Edit a ring's notes by chip; MIX + CLEAR up resets, down rolls in key | Proposed; CLEAR's focus amended by [0086](0086-every-gesture-shows-a-contextual-hint-in-the-focus-band.md) |
| [0075](0075-capture-is-armed-once-and-taken-once.md) | A capture is armed once and taken once; a STEP ring's spans key-ups | Proposed; superseded in part by [0082](0082-orbit-command-channel.md) |
| [0076](0076-live-keys-snap-per-part-held-keys-glide.md) | Live keys snap per Part in the Instrument; a held key re-snaps from its raw key on the pitch path | Proposed |
| [0077](0077-scenes-land-by-generation-saves-are-witnessed.md) | A scene switch lands by its generation; SAVE OVER is witnessed when it asks | Proposed; superseded in part by [0082](0082-orbit-command-channel.md); amended by [0083](0083-a-scene-keeps-its-edits-across-switches.md) |
| [0078](0078-orbit-saves-after-the-parts-and-loads-what-it-can.md) | ORBIT saves after the six Parts, and a load takes what it can | Proposed |
| [0079](0079-orbs-four-cells-mirror-key-scale-bars-and-scene.md) | ORB's four cells: KEY, SCALE, BARS and SCENE (amended: ORB the frame, CHRD the progression) | Proposed; amended by [0084](0084-edit-on-orb-always-opens-custom-rings-show-their-part.md) |
| [0080](0080-sync-int-or-ext-a-daw-drives-orbits-transport.md) | SYNC picks ORBIT's master: INT runs free, EXT lets a DAW drive the transport; stored in SYSTEM (amends 0071, 0072) | Proposed; amended by [0082](0082-orbit-command-channel.md) |
| [0081](0081-one-d-pad-on-every-chain.md) | One D-pad on every chain: SEQ up, EDIT down with wrap, MINUS and PLUS along (supersedes the SEQ tap of 0044 and 0066) | Proposed; browser hints amended by [0086](0086-every-gesture-shows-a-contextual-hint-in-the-focus-band.md) |
| [0082](0082-orbit-command-channel.md) | ORBIT's UI and runner talk through one sequenced command log and one ack (supersedes parts of 0071, 0075, 0077; amends 0080) | Proposed |
| [0083](0083-a-scene-keeps-its-edits-across-switches.md) | A scene keeps its edits across switches; drift restarts (amends 0077) | Proposed |
| [0084](0084-edit-on-orb-always-opens-custom-rings-show-their-part.md) | EDIT on ORB always opens CUSTOM, a named scale made CUSTOM by the first toggle; each ring shows its Part on ORB (amends 0079) | Proposed |
| [0086](0086-every-gesture-shows-a-contextual-hint-in-the-focus-band.md) | Every gesture shows a contextual hint in the focus band, one shared component, HINTS ON/OFF in THEME (amends 0074, 0081; supersedes 0017's hint text) | Proposed |
| [0089](0089-ship-with-fat-lto.md) | Ship the firmware with fat LTO in one codegen unit | Proposed |
| [0091](0091-orbit-billed-while-it-plays.md) | While ORBIT plays, reserve its own block and a note-on burst; cost the Parts once a block; bill a stolen voice until its fade ends (supersedes in part 0040; amends 0027) | Proposed |
| [0092](0092-put-files-over-the-usb-console.md) | Put project files onto the card over the USB console: `ls`, and `put` through the unit's own A/B save, checked as it streams, a refused side deleted (amends 0068) | Proposed |
| [0097](0097-bank-and-symp-clip-with-stmlibs-curve.md) | BANK and SYMP clip with stmlib's curve; the Elements stagger tried and dropped (amends 0063) | Proposed |
| [0096](0096-three-filter-types-cascadia-home_prophet-dirty.md) | Three filter TYPEs: the SVF as a Cascadia-style home filter, PROPHET (SSM2040) and DIRTY (Chamberlin); per-type modes, labels and bills (amends 0037) | Proposed |
| [0096](0096-filter-types-clean-2040-dirty-and-algo-taps.md) | Three filter TYPEs, CLEAN (Cascadia-style), 2040 (SSM2040) and DIRTY (Chamberlin), with per-type modes, labels and bills; ALGO OP 5/6 audio-rate taps to FREQ, Q, VCA and FOLD (amends 0037) | Proposed |
| [0096](0096-filter-types-clean-2040-dirty-and-algo-taps.md) | Three filter TYPEs, CLEAN (Cascadia-style), 2040 (SSM2040) and DIRTY (Chamberlin), with per-type modes, labels and bills; ALGO OP 5/6 audio-rate taps to FREQ, Q, VCA and FOLD (supersedes 0037) | Proposed |
