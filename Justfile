# Justfile for Chimera

# The bench's builds, an area each (all of it no longer fits FLASH), and
# the filter probes `tools/loop_count.py` counts. Each is built with
# `--no-default-features` (the `bench` feature brings back all of the
# shipping set but the console's `put`; so does `filter-probes`). Every
# firmware build has bank 2 (ADR 0194): TAPE, the reverb, VA's MOD path
# (ADR 0163, 0174) and the bench itself play from it, and Gate 3 found
# bank 2 never slower (ADR 0142). A bench run needs its build's .bk2
# installed. `bench-stress,orbit` is out: it fails the stack check
# (private issue #656).
bench_sets := "fault-inject bench-algo bench-algo,master-tape bench-routing bench-memory bench-stress bench-orbit bench-tape filter-probes"
bench_all := "bench-algo,bench-routing,bench-memory,bench-stress,bench-orbit"

# Build desktop simulator
desktop:
    cargo run -p chimera-desktop --bin chimera-desktop

# The browser editor against the simulator: the sim, and its console on ws://127.0.0.1:7342
editor-sim:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build -q --profile utils -p chimera-utils
    {{utils}} ed-bridge &
    bridge=$!
    # cargo run's child is the bridge: take both, never the whole group.
    trap 'pkill -P $bridge || true; kill $bridge 2>/dev/null || true' EXIT
    cargo run -p chimera-desktop --bin chimera-desktop

# The editor's protocol end to end, against a running `just editor-sim` (the simulator has no headless mode)
editor-e2e:
    node editor/e2e/sim.mjs

# The browser editor's Elm tests, then elm-review (the site's gate, editor/review)
editor-test:
    cd editor && npm install --no-audit --no-fund --silent && node tests/gen-fixture.js && npx elm-test && npx elm-review && node public/ports.check.js && node public/theme.check.js

# The browser editor, optimised, into editor/dist/
editor-build:
    cd editor && mkdir -p dist && elm make src/Main.elm --optimize --output=dist/elm.js && cp public/index.html public/ports.js public/style.css dist/

# The UI component gallery (every component, every state, DAY and NIGHT, and
# the JITTER probe), into editor/gallery/
editor-gallery:
    cd editor && mkdir -p gallery/glyphs && elm make src/Gallery.elm --optimize --output=gallery/gallery.js && cp public/gallery.html gallery/index.html && cp public/style.css public/theme.js gallery/ && cp public/glyphs/*.png gallery/glyphs/

# TAPE's listening rig (spec § 9): each model on each source into
# target/listen/ (WAVs, report.txt, and index.html once every render passes
# QA). `just tape-listen loop --src chimera --src <file.wav>`; a bare file
# name is looked for in $CHIMERA_TAPES_SRC (~/chimera-tapes-src)
tape-listen *args:
    cargo run -q -p chimera-desktop --bin tape-listen -- {{args}}

# Streamed PLAY's desktop QA (slice 1 § 10.5): every factory bed in
# target/tapes-v1/ (`just factory-tapes` first), ONE and LOOP, read through
# the card model at m1, slow and m1 with a fault, judged by the click rule;
# then six voices admission takes, one at inc 2, on a 28 s bed with every
# read at slow's 19.6 ms ceiling: drop-outs that must all be fades.
# `just stream-qa <dir>` also writes RAIN, TRON BOWED and PAD CHORD's LOOPs
# there as WAVs.
stream-qa dir="":
    STREAM_QA_WAVS="{{dir}}" cargo test -q -p chimera-desktop --test stream_qa -- --ignored --nocapture

# A WAV to a TAPE source (spec § 6), sealed in the desktop sim's card
# (CHIMERA_CARD, else chimera-card/), never the unit's:
# `just tape take.wav [--rate 24k] [--root C3] [--transwave <cycle> [--top 1024] [--frames 32]]
# [--name CHOIR] [--channel mix|L|R]`. `just put-tape <file>` sends it to the unit.
[positional-arguments]
tape file *args:
    #!/usr/bin/env bash
    set -euo pipefail
    shift
    cargo run -q -p chimera-desktop --bin chimera-import -- tape "{{ if file =~ '^/' { file } else { invocation_directory() / file } }}" "$@"

# A TX81Z bank's voices to private port Sources (showcase spec § 6):
# `just import-tx <bank.syx> [--voices 1,4,9-12] [--out DIR] [--force]` writes
# <voice>.toml and import-report.txt into DIR (default $CHIMERA_TX_PORTS,
# ledgers/showcase-private/ports/), never a Yamaha voice name. Refuses a
# DIR inside a git work tree unless git ignores it, and an existing
# <voice>.toml without --force.
[positional-arguments]
import-tx bank *args:
    #!/usr/bin/env bash
    set -euo pipefail
    shift
    cargo run -q -p chimera-desktop --bin chimera-import -- tx "{{ if bank =~ '^/' { bank } else { invocation_directory() / bank } }}" "$@"

# The TAPE factory set (spec § 7) into target/tapes-v1/: every .SRC, the
# factory TAPE Sounds (S9000001.A …), manifest.json and cc0-originals.zip
# (each original with its licence). CC0 originals come from
# $CHIMERA_TAPES_SRC (~/chimera-tapes-src); nothing here writes a card.
# `just factory-tapes qa [--wav <dir>]` plays each source through every
# machine and checks each render.
factory-tapes *args:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run -q --release -p chimera-desktop --bin factory-tapes -- {{args}}
    if [ -z "{{args}}" ]; then
        cd target/tapes-v1
        rm -f cc0-originals.zip
        zip -qrX cc0-originals.zip cc0-originals
        rm -r cc0-originals
    fi

# The public factory Sounds (spec § 3, ADR 0134): tools/factory-sounds/'s
# Sources parsed, checked (ids, names, descriptions, routes) and built
# through the firmware's encoder into target/factory-sounds/public/
# (CHIMERA/SOUNDS/S91xxxxx.A, the demo projects from projects.toml as
# CHIMERA/PROJECTS/P000000n.A, and manifest.json). Refuses a port Source.
# `just factory-sounds fmt` writes each value's display comment.
# `--private` (with build, fmt or qa) also reads the TX81Z ports in
# $CHIMERA_TX_PORTS (default ledgers/showcase-private/ports/, never the
# repo): built into target/factory-sounds/private/ only; qa pages them in
# showcase-private/qa/, never the published showcase/.
factory-sounds *args:
    cargo run -q --release -p chimera-desktop --bin factory-sounds -- {{args}}

# The TAPE sources onto the unit's card: each through `put` (ADR 0106; a
# re-run answers OK PRESENT for each). `dir` is `just factory-tapes`'
# output, or a downloaded `tapes-vN` release. Sources only: the factory
# TAPE Sounds (S9xxxxxx.A) ride in factory-v1.zip with the others, for
# the FACTORY tab (`just release`).
put-factory-tapes dir="target/tapes-v1":
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    srcs=("{{dir}}"/*.SRC)
    [ ${#srcs[@]} -gt 0 ] || { echo "no .SRC in {{dir}}: just factory-tapes" >&2; exit 1; }
    for f in "${srcs[@]}"; do python3 tools/chimera-usb.py put "$f" /CHIMERA/TAPES/; done

# The public factory set onto the unit's card over USB: each Sound with
# `--over` (a re-run replaces it), each demo project as the unit saves one
# (`put` takes no `--over` for projects; it saves a new generation). Older
# alpha firmware saved user projects from P0000001, and the demos land on
# P0000001-P0000009, so this refuses, naming them, if any of those is on
# the card, unless `--replace-demos`. `just factory-sounds` builds `dir`.
# A card without PROJECTS lists nothing. CHIMERA_USB_CMD overrides the
# console command (tools/test_put_factory_sounds.py stubs it); CHIMERA_USB
# still picks the port, as everywhere.
put-factory-sounds dir="target/factory-sounds/public" *flags:
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    USB=${CHIMERA_USB_CMD:-"python3 tools/chimera-usb.py"}
    replace=0
    for f in {{flags}}; do
        case $f in --replace-demos) replace=1;; *) echo "unknown flag $f" >&2; exit 1;; esac
    done
    sounds=("{{dir}}"/CHIMERA/SOUNDS/*.A); projects=("{{dir}}"/CHIMERA/PROJECTS/*.A)
    [ ${#sounds[@]} -gt 0 ] && [ ${#projects[@]} -gt 0 ] || { echo "no factory set in {{dir}}: just factory-sounds" >&2; exit 1; }
    listing=$($USB ls 2>&1) || case $listing in
        *"FILE NOT FOUND"*) listing="";;
        *) printf '%s\n' "$listing" >&2; exit 1;;
    esac
    taken=$(printf '%s\n' "$listing" | grep -E '^P000000[1-9]\.[AB] ' | cut -d' ' -f1 | cut -d. -f1 | sort -u | tr '\n' ' ' || true)
    if [ -n "$taken" ] && [ $replace = 0 ]; then
        echo "the card already has projects ${taken}: the demos would replace them." >&2
        echo "back them up (get), or pass --replace-demos to overwrite them." >&2
        exit 1
    fi
    for f in "${sounds[@]}"; do $USB put "$f" /CHIMERA/SOUNDS/"$(basename "$f")" --over; done
    for f in "${projects[@]}"; do $USB put "$f" /CHIMERA/PROJECTS/"$(basename "$f")"; done

# Build STM32 firmware (release): target/chimera.bin and .bk2
firmware:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera

# Build STM32 firmware with ORBIT (release): target/chimera-orbit.bin and .bk2
firmware-orbit:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features orbit
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-orbit

# ORBIT with usb-device writing the configuration (`usb-config-writer`),
# the fallback if the class's static (ADR 0260) fails on a host:
# target/chimera-orbit-writer.bin and .bk2.
firmware-orbit-writer:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features orbit,usb-config-writer
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-orbit-writer

# Hardware Gate 2's power-pull aid (`bank2-gate`): the install holds 3 s at
# each phase for a hand to pull the power, and the console takes `bank2
# erase`; and the slot gates' (ADR 0272): `bank2 slot-poke`, `erase slots`,
# `ecc-tear`, `ecc-read <addr>` and `pull erase-s4|erase-s6|tag-mixer|tag-edit`. target/chimera-gate.bin and .bk2. Never flashed by a recipe, never
# released: every other .bin is refused if it holds the aid.
firmware-gate:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bank2-gate
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-gate

utils := "cargo run -q --profile utils -p chimera-utils --"

# The NRPN guide page (#926), from the chart the firmware answers with.
nrpn-guide:
    {{utils}} nrpn-guide > docs/guide/NRPN.md

# SUGGEST, the factory controller map, from the param tables: its card
# file into DIR/CHIMERA/MAPS (the factory pack's), or `--tables` for
# docs/guide/MIDI.md's controller map section.
suggest-map dir="target/factory-maps":
    {{utils}} suggest-map {{dir}}

# The ELF's two banks: <out>.bin (bank 1, never .bank2) and <out>.bk2. Every
# firmware build has bank 2 (ADR 0194): an empty .bank2 fails.
bank2-split elf out:
    #!/usr/bin/env bash
    set -euo pipefail
    rust-objcopy -R .bank2 -O binary {{elf}} {{out}}.bin
    just check-bin-size {{out}}.bin
    if [ "$(basename {{out}})" != chimera-gate ]; then just check-no-gate {{out}}.bin; fi
    rm -f {{out}}.bk2
    sections=$(rust-objdump -h {{elf}})
    if awk '$2 == ".bank2" && $3 != "00000000" { f = 1 } END { exit !f }' <<< "$sections"; then
        pkg=$(cargo pkgid -p chimera-stm32)
        hash=$(git rev-parse --short=8 HEAD)
        ver="${pkg##*[#@]} $hash"
        rust-objcopy -j .bank2 -O binary {{elf}} {{out}}.bk2
        {{utils}} door-check {{elf}}
        just check-door-placed {{elf}}
        just check-itcm {{elf}}
        {{utils}} bank2-seal {{elf}} {{out}}.bin {{out}}.bk2 --version "$ver"
        just check-slots {{elf}} {{out}}.bk2
        just check-bin-paths {{out}}.bk2
    else
        echo "{{elf}} has no bank 2 (ADR 0194)" >&2; exit 1
    fi

# Release binaries: the shipping firmware and its ORBIT variant with every build
# path remapped (#413), so no home directory, user name or registry path
# survives in the .bin. `--config` arrays append to .cargo/config.toml's
# target rustflags (RUSTFLAGS would replace them).
# Panics carry no locations (.cargo/config.toml). Output: target/release-bin/chimera{,-orbit}.{bin,bk2}
release-bin:
    #!/usr/bin/env bash
    set -euo pipefail
    T=thumbv7em-none-eabihf
    cargo_home="${CARGO_HOME:-$HOME/.cargo}"
    flags="['--remap-path-prefix=$HOME=/build/hm','--remap-path-prefix=$cargo_home=/build/cargo','--remap-path-prefix=$PWD=/build/chimera']"
    mkdir -p target/release-bin
    for v in "" orbit; do
        if [ -n "$v" ]; then f="--features $v"; out=chimera-orbit; else f=""; out=chimera; fi
        cargo build --release -p chimera-stm32 --target $T $f --config "target.$T.rustflags=$flags"
        just bank2-split target/$T/release/chimera-stm32 target/release-bin/$out
        just check-bin-paths target/release-bin/$out.bin
        just check-no-gate target/release-bin/$out.bin
    done

# A .bin holding the power-pull aid (`bank2-gate`: its markers, the slot
# commands' among them, or its PULL NOW) never ships.
# VA's MOD path (ADR 0163), the LFO page's raster (ADR 0177) and PIT's
# tuner (ADR 0201), the delay's TAPE loop and the chorus's (ADR 0213), and TAPE's UI door entries and STRETCH GRAIN's note-on
# (its own-period search inlined there, ADR 0204) are all
# bank 2's on a bank-2 build: a `bank2.x` glob that misses a new door entry
# or run leaves it silently in bank 1. ORBIT's runner and pages (ADR 0208)
# too, on a build that links them; and the NRPN address space (ADR 0216)
# once anything links it. The map's codec (MidiDoor) always, and the
# console's `ed` (EdDoor, ADR 0251). Bank 2 runs to 0x08200000; nothing
# checked may sit in a slot's 1 KB reserve or its end marker (ADR 0272).
check-door-placed elf:
    #!/usr/bin/env bash
    set -euo pipefail
    syms=$(rust-nm -C {{elf}})
    orbit=()
    if grep -q 'chimera_engine::orbit::door::' <<< "$syms"; then orbit=('chimera_engine::orbit::door::' 'ui::orbit::raster::' 'ui::orbit::draw::cell_text$' 'ui::orbit::draw::viz_key$'); fi
    nrpn=()
    # A text symbol: a gate on any symbol also trips on rodata alone.
    if grep -qE ' [tT] .*chimera_engine::nrpn::' <<< "$syms"; then nrpn=('chimera_engine::nrpn::'); fi
    # USB audio's stream (ADR 0248), where `usb-audio` links it (every
    # shipping build, ADR 0256): whatever of it the inliner leaves out of
    # USB MIDI's door poll.
    if grep -qE ' [tT] .*(usb_audio::door::|chimera_midi::uac::)' <<< "$syms"; then nrpn+=('(usb_audio::door::|chimera_midi::uac::)'); fi
    pipe=()
    # The pipe's layers inline into `DinPipe::feed`; any left out of line
    # are checked too.
    if grep -q 'midi_in::DinPipe' <<< "$syms"; then
        pipe=('midi_in::DinPipe>::door_feed$' 'midi_in::DinPipe>::door_idle$' 'midi_in::DinPipe>::')
        for p in 'midi_in::Counted' 'chimera_midi::stream::' 'chimera_midi::ctl::' 'chimera_midi::sense::' 'chimera_midi::sysex::'; do
            if grep -qE " [tT] .*$p" <<< "$syms"; then pipe+=("$p"); fi
        done
    fi
    # The MIDI UI's door entries always link; whatever of `ui::midi` the
    # inliner leaves out of them is checked too, and MIDI MAP's entries
    # (`hear_in`, `map_in`) where `midi-map` links them.
    midi_ui=('ui::UiState>::hand_in$')
    for p in 'ui::midi::' 'ui::UiState>::hear_in$' 'ui::UiState>::map_in$' 'ui::UiState>::focus_note$' 'ui::UiState>::out_in$' 'chimera_midi::take::' 'chimera_midi::law::' 'chimera_midi::rel::' 'chimera_midi::learn::' 'chimera_midi::out::'; do
        if grep -qE " [tT] .*$p" <<< "$syms"; then midi_ui+=("$p"); fi
    done
    # DRUM's voice groups (ADR 0280): the door's entry always links, the
    # count where the inliner leaves it out of them.
    midi_ui+=('ui::kit_groups::')
    if grep -qE ' [tT] .*drum::groups::' <<< "$syms"; then midi_ui+=('drum::groups::'); fi
    # MAX VOICES and RESERVE (ADR 0281): the door's entry; and what the
    # linker script pins beside it, where it has a symbol of its own (the
    # toast check's instance, the rule when not inlined).
    midi_ui+=('ui::part_voices::')
    if grep -qE ' [tT] .*ToastTimer>::shows' <<< "$syms"; then midi_ui+=('ToastTimer>::shows'); fi
    if grep -qE ' [tT] .*Shares>::refusal' <<< "$syms"; then midi_ui+=('Shares>::refusal'); fi
    for pat in "${orbit[@]}" "${nrpn[@]}" "${pipe[@]}" "${midi_ui[@]}" 'midi_map::codec::' 'va::osc::door::' 'va::osc::solo::' 'va::lbfm::render_loop' 'VaEngine>::render_loopfm' 'chimera_engine::lfo_glyph::draw::' 'voice_fx::fuzz::Fuzz' 'voice_fx::look::render' 'filter::clean::Clean>::fed$' 'filter::dirty::Dirty>::fed$' 'filter::ladder::Ladder>::fed$' 'filter::k2040::K2040>::fed::' 'filter::sem::kern::' 'ui::jog::Jog>::pass' 'va::noise::kern::' 'dsp::delay::grain::' 'dsp::delay::tape::' 'dsp::chorus::kern::' 'dsp::grain::core::' 'voice::gate::kern::' 'voice::depth::kern::' 'dsp::modal::kern::' 'dsp::modal::render_bowed$' 'StringVoice>::render$' 'StringVoice>::colour$' 'algo::engine::kern::' 'algo::kernel::Kernel>::render_ops::' 'ui::pit_tuner::calc::' 'ui::walk::machine_edit_in' 'ui::walk::tape_note_in' 'ui::walk::tape_held_in' 'ui::walk::tape_fmt_in' 'ui::walk::law_text_in' 'ui::walk::delay_note_in' 'ui::walk::readout_word_in' 'ui::walk::tape_writes_in' 'ui::walk::band_note_in' 'stretch::grain::Grains>::on' 'randomize::kern::' 'console::ed::'; do
        grep -E " [tT] .*$pat" <<< "$syms" | awk -v pat="$pat" '{ n++; a = strtonum("0x" $1); o = (a - 0x08100000) % 0x40000; if (a < 0x08100000 || a >= 0x08200000) { print "not in bank 2: " $0 > "/dev/stderr"; bad = 1 } else if (a - 0x08100000 >= 0x80000 && (o < 0x400 || o >= 0x3FFE0)) { print "in a slot reserve or end marker: " $0 > "/dev/stderr"; bad = 1 } } END { if (!n) print pat ": nothing linked" > "/dev/stderr"; exit bad || !n }'
    done
    # ed is pinned to the S4 group (ADR 0272), not anywhere in bank 2.
    grep -E ' [tT] .*console::ed::' <<< "$syms" | awk '{ a = strtonum("0x" $1); if (a < 0x08180400 || a >= 0x081BFFE0) { print "ed outside the S4 group: " $0 > "/dev/stderr"; bad = 1 } } END { exit bad }'
    # So are VA's solos and their table (ADR 0283).
    grep -E ' [tT] .*va::osc::solo::' <<< "$syms" | awk '{ a = strtonum("0x" $1); if (a < 0x08180400 || a >= 0x081BFFE0) { print "VA solo outside the S4 group: " $0 > "/dev/stderr"; bad = 1 } } END { exit bad }'
    # And no solo code of theirs sits in bank 1: the quiet pulse's or its runs' (ADR 0283).
    # (awk alone: when all of it is inlined into `solo::play` there is none, and grep would fail.)
    awk '/ [tT] .*PulseQuiet/ { a = strtonum("0x" $1); if (a < 0x08100000 || a >= 0x08200000) { print "VA solo code in bank 1: " $0 > "/dev/stderr"; bad = 1 } } END { exit bad }' <<< "$syms"

# ITCM's state (ADR 0266, 0286): CPU-only, never a DMA's. Only the
# residents tools/itcm-residents.txt lists lie in `[__sitcm, __eitcm)`, and
# `AUDIO` is there wherever the synth is linked (not in sd-probe).
check-itcm elf:
    python3 tools/check_itcm.py {{elf}}

check-no-gate bin:
    #!/usr/bin/env bash
    set -euo pipefail
    n=$(grep -a -c -e BANK2-GATE-BUILD -e SLOT-GATE-BUILD -e 'PULL NOW' "{{bin}}") || [ $? -eq 1 ]
    if [ "${n:-0}" != 0 ]; then echo "{{bin}} is a bank2-gate build: never ship it" >&2; exit 1; fi

# Stock bootloader SD flash erases sectors 1..size/128K+1: a .bin of 917,504 B
# or more reaches sector 8, which wraps to sector 0 and erases the bootloader.
# Every shipped .bin stays strictly under it.
check-bin-size bin:
    #!/usr/bin/env bash
    set -euo pipefail
    n=$(stat -c %s "{{bin}}")
    if [ "$n" -ge 917504 ]; then echo "{{bin}} is $n B: the stock bootloader's SD flash would erase itself at 917,504 B or more" >&2; exit 1; fi
    echo "{{bin}}: $n B, $((917504 - n)) B under the SD-flash limit"

# Bank 1 is frozen (ADR 0198): the last-built firmware's bank 1 (the .bin
# without .bank2) may not outgrow its set's line in tools/bank1-budget.txt
# by more than 64 B, and must lower the line when it frees 1 KB. Sets
# without bank 2 (`--no-default-features`, `sd-probe`) carry no budget:
# code moved behind a door lands in their bank 1.
check-bank1 set:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p target/bank1
    rust-objcopy -R .bank2 -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 "target/bank1/{{set}}.bin"
    python3 tools/bank1_budget.py "{{set}}" "target/bank1/{{set}}.bin"

# Bank 2's room (ADR 0272): the last-built firmware's free space in S0-S3,
# S4-S5 and S6-S7, from its end symbols. Under 8 KB free in all fails (ADR
# 0251), and the bytes in use are held to the set's line in
# tools/bank2-budget.txt under bank 1's rules.
check-bank2 set:
    python3 tools/bank2_budget.py "{{set}}" target/thumbv7em-none-eabihf/release/chimera-stm32

# The .bk2's slot stubs and end markers (ADR 0272): chimera-utils checks
# their bytes, the size and the 0xFF reserves; here, every gap from a
# region's end symbol to its fixed point is 0xFF (lld's fill), each stub
# decodes to the trampoline, and no symbol but the stubs' own lies in a
# reserve or an end marker.
check-slots elf bk2:
    #!/usr/bin/env bash
    set -euo pipefail
    {{utils}} check-slots {{bk2}}
    syms=$(rust-nm -C {{elf}})
    sym() { awk -v s="$1" '$3 == s { print strtonum("0x" $1) }' <<< "$syms"; }
    for gap in "__bank2_lo_end 0x08180000" "__bank2_s4_end 0x081BFFE0" "__bank2_s6_end 0x081FFFE0"; do
        read -r name to <<< "$gap"
        from=$(sym "$name"); to=$((to))
        [ -n "$from" ] || { echo "{{elf}}: $name missing: not the 1 MB bank2.x layout (ADR 0272)" >&2; exit 1; }
        n=$(head -c $((to - 0x08100000)) {{bk2}} | tail -c $((to - from)) | tr -d '\377' | wc -c)
        if [ "$n" != 0 ]; then echo "{{bk2}}: $n B not 0xFF from $name to $(printf %#x $to): lld's fill did not hold" >&2; exit 1; fi
    done
    # The stubs are data to the ELF (objdump shows .word): their code bytes
    # from the .bk2, disassembled as Thumb.
    mkdir -p target/slots
    want='ldr r0, [pc, #0x14]; ldr r1, [pc, #0x18]; str r0, [r1]; dsb sy; isb sy; ldr r1, [r0]; ldr r2, [r0, #0x4]; msr msp, r1; bx r2; '
    for off in 0x80008 0xC0008; do
        dd if={{bk2}} of=target/slots/stub.raw bs=1 skip=$((off)) count=24 status=none
        rust-objcopy -I binary -O elf32-littlearm --rename-section .data=.text,code target/slots/stub.raw target/slots/stub.elf
        ops=$(rust-objdump -d --triple=thumbv7em-none-eabihf --no-show-raw-insn target/slots/stub.elf \
            | awk '/^ *[0-9a-f]+:/ { sub(/^ *[0-9a-f]+:[ \t]*/, ""); sub(/[ \t]*@.*/, ""); gsub(/\t/, " "); printf "%s; ", $0 }')
        if [ "$ops" != "$want" ]; then echo "{{bk2}}: the stub at +$off decodes to: $ops" >&2; exit 1; fi
    done
    awk '{ a = strtonum("0x" $1); o = (a - 0x08100000) % 0x40000; name = $3; for (i = 4; i <= NF; i++) name = name " " $i }
        a >= 0x08180000 && a < 0x08200000 && (o < 0x400 || o >= 0x3FFE0) && name !~ /^(__slot_(mixer|edit)|__bank2_.*|\$.*|.*::SLOT_(MIXER|EDIT)(_END)?)$/ {
            print "in a slot reserve or end marker: " $0 > "/dev/stderr"; bad = 1 }
        END { exit bad }' <<< "$syms"
    echo "{{bk2}}: slots, fill and stubs ok"

# Draft a release of the path-clean firmware on the PUBLIC chimera-releases
# repo (#413): `just release 0.1.0 [--dry-run]`. Refuses a dirty tree or a
# version already tagged or released; builds `release-bin`; names the files
# CHIMERA.BIN (8.3, so the stock bootloader lists it, #558; the ORBIT build,
# the only firmware shipped), CHIMERA.BK2 (sealed for that .bin: dropped in
# the card root, the next boot installs it) and factory-v1.zip (the
# card's whole CHIMERA folder: every factory Sound, TAPE's sources, the
# demo projects and the SUGGEST controller map); writes
# SHA256SUMS and the notes from docs/alpha/RELEASE-NOTES.md.tmpl; asserts
# the assets are exactly those four and none is a private file's bytes. A
# draft only: the owner publishes. --dry-run does everything but `gh release create`.
release version *flags:
    #!/usr/bin/env bash
    set -euo pipefail
    V={{version}}; REPO=joegiralt/chimera-releases; dry=0
    for f in {{flags}}; do
        case $f in --dry-run) dry=1;; *) echo "unknown flag $f" >&2; exit 1;; esac
    done
    [[ $V =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "version must look like 0.1.0" >&2; exit 1; }
    [ -z "$(git status --porcelain)" ] || { echo "dirty tree: commit or stash first" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/v$V" >/dev/null || { echo "v$V is already tagged" >&2; exit 1; }
    if gh release view "v$V" -R $REPO >/dev/null 2>&1; then echo "v$V is already released on $REPO" >&2; exit 1; fi
    just release-bin
    out="$PWD/target/release-assets/v$V"
    rm -rf "$out"; mkdir -p "$out"
    # One firmware ships: the ORBIT build, which does everything the plain build does (owner, 2026-10-14),
    # with its bank 2 beside it, one build: the seal must match.
    cp target/release-bin/chimera-orbit.bin "$out/CHIMERA.BIN"
    cp target/release-bin/chimera-orbit.bk2 "$out/CHIMERA.BK2"
    {{utils}} bank2-seal --check "$out/CHIMERA.BIN" "$out/CHIMERA.BK2"
    for b in "$out/CHIMERA.BIN" "$out/CHIMERA.BK2"; do just check-bin-paths "$b"; done
    # No .bk2 under a .bin name: only the .bk2 starts CHIMERA2.
    if head -c 8 "$out/CHIMERA.BIN" | grep -qa CHIMERA2; then echo "CHIMERA.BIN is a .bk2" >&2; exit 1; fi
    head -c 8 "$out/CHIMERA.BK2" | grep -qa CHIMERA2 || { echo "CHIMERA.BK2 is not a .bk2" >&2; exit 1; }
    # The factory set: the card's CHIMERA folder as a tester copies it, one
    # root: the public build's Sounds and demo projects, TAPE's Sounds and
    # the sources they play in TAPES, SUGGEST in MAPS (fresh from the param
    # tables), nothing private, refused unless every
    # count is the builds' (tools/factory_zip.py). FACTORY_PUBLIC and
    # FACTORY_TAPES override the dirs and skip the fresh builds, --dry-run only.
    [ -z "${FACTORY_PUBLIC:-}${FACTORY_TAPES:-}" ] || [ $dry = 1 ] || { echo "FACTORY_PUBLIC and FACTORY_TAPES are for --dry-run only: a release packs fresh builds" >&2; exit 1; }
    if [ -z "${FACTORY_PUBLIC:-}" ]; then just factory-sounds; fi
    if [ -z "${FACTORY_TAPES:-}" ]; then just factory-tapes; fi
    pub=${FACTORY_PUBLIC:-target/factory-sounds/public}; tapes=${FACTORY_TAPES:-target/tapes-v1}
    rm -rf target/factory-maps; just suggest-map target/factory-maps
    python3 -I tools/factory_zip.py "$pub" "$tapes" target/factory-maps "$out/factory-v1.zip"
    (cd "$out" && sha256sum CHIMERA.BIN CHIMERA.BK2 factory-v1.zip > SHA256SUMS)
    # Nothing private ships: exactly these assets, and none is a private
    # file (the stock bootloader backup, the bootloader, a dev chimera.bin or .bk2).
    [ "$(ls "$out" | LC_ALL=C sort | tr '\n' ' ')" = "CHIMERA.BIN CHIMERA.BK2 SHA256SUMS factory-v1.zip " ] || { echo "unexpected assets in $out" >&2; exit 1; }
    for p in preenfm3-bootloader-backup.bin bootloader.bin chimera.bin chimera.bk2 target/chimera.bin target/chimera.bk2; do
        [ -f "$p" ] || continue
        h=$(sha256sum "$p" | cut -d' ' -f1)
        if grep -q "$h" "$out/SHA256SUMS"; then echo "asset is $p: refusing" >&2; exit 1; fi
    done
    # The demo list comes from projects.toml, so the notes never drift from the build.
    python3 -I - > "$out/demos.md" <<'PY'
    import tomllib
    for p in tomllib.load(open("tools/factory-sounds/projects.toml", "rb"))["project"]:
        if "held" not in p:
            print("- **%s**: %s." % (p["name"], p["try"].rstrip(".")))
    PY
    # The crates the firmware links (both variants), from Cargo, so the notices never drift either.
    for f in "" orbit; do cargo tree -q -p chimera-stm32 --target thumbv7em-none-eabihf -e normal --prefix none --format '{p} {l}' ${f:+--features=$f}; done \
        | grep -v -e '^chimera' -e 'proc-macro' | sed 's/ (\*)$//' | sort -u | sed 's/^\([^ ]*\) \(v[^ ]*\) \(.*\)$/- `\1` \2: \3/' \
        | sed 's|^\(- `u8g2-fonts` [^:]*\): *$|\1: MIT OR Apache-2.0; its fonts are U8g2'"'"'s, each under its own licence (https://github.com/olikraus/u8g2/blob/master/LICENSE)|' > "$out/crates.md"
    ! grep -q ': *$' "$out/crates.md" || { echo "a linked crate has no licence: add it to the notes" >&2; exit 1; }
    [ -s "$out/crates.md" ] || { echo "no crate list" >&2; exit 1; }
    sounds=$(python3 -I -m zipfile -l "$out/factory-v1.zip" | grep -c 'CHIMERA/SOUNDS/.*\.A ' || true)
    demos=$(python3 -I -m zipfile -l "$out/factory-v1.zip" | grep -c 'CHIMERA/PROJECTS/.*\.A ' || true)
    sed -e "s/@VERSION@/$V/g" -e "s/@SOUNDS@/$sounds/g" -e "s/@PROJECTS@/$demos/g" -e "s/@COMMIT@/$(git rev-parse --short HEAD)/g" -e "s/@DATE@/$(date -u +%F)/g" \
        -e "/@DEMOS@/{r $out/demos.md" -e "d}" -e "/@CRATES@/{r $out/crates.md" -e "d}" docs/alpha/RELEASE-NOTES.md.tmpl > "$out/RELEASE-NOTES.md"
    rm "$out/demos.md" "$out/crates.md"
    ! grep -q '@[A-Z]*@' "$out/RELEASE-NOTES.md" || { echo "unfilled template field" >&2; exit 1; }
    assets=("$out/CHIMERA.BIN" "$out/CHIMERA.BK2" "$out/factory-v1.zip" "$out/SHA256SUMS")
    echo "assets:"; (cd "$out" && ls -l CHIMERA.BIN CHIMERA.BK2 factory-v1.zip SHA256SUMS && cat SHA256SUMS)
    if [ $dry = 1 ]; then
        echo "dry run: would draft v$V on $REPO with notes $out/RELEASE-NOTES.md"; exit 0
    fi
    gh release create "v$V" "${assets[@]}" -R $REPO --draft --title "Chimera $V" --notes-file "$out/RELEASE-NOTES.md"
    echo "drafted v$V on $REPO: review and publish it there"

# Fail if a binary carries a home directory, the cargo registry or the user name (#413).
check-bin-paths bin:
    #!/usr/bin/env bash
    set -euo pipefail
    if strings -a {{bin}} | grep -E -e '/home/' -e '\.cargo/registry' -e "$(id -un)"; then
        echo "{{bin}} leaks a build path" >&2; exit 1
    fi

# Build all (desktop targets only)
build:
    cargo build -p chimera-core -p chimera-engine -p chimera-midi -p chimera-hal -p chimera-desktop

# Everything must pass before a commit (ADR 0013): core + hal tests (and core
# with the master tape back on, ADR 0055, and the whole suite without ORBIT), desktop
# tests and its no-MIDI build, the dosfstools cross-checks, the firmware built and linked with default
# features, with none, with ORBIT, each bench area (ALGO also with the master
# tape), the filter probes and the SD probe, all with bank 2 (ADR 0194), clippy on host
# and firmware (every feature set), rustfmt and the stack check; each firmware set but no-default and sd-probe is held to its bank-1 budget (ADR 0198). The desktop needs ALSA's
# pkg-config file; point PKG_CONFIG_PATH at it if it is not installed
# system-wide. The host tool's tests run first, against a fake unit.
# chimera-circuits runs its fast tests here; its slow and ngspice
# references are #[ignore]d (chimera-circuits/README.md).
# Its firmware is stamped clean (CHIMERA_STAMP_CLEAN): a dirty tree's flag
# folds into code, and bank 1 is measured as the commit would build (#874).
check $CHIMERA_STAMP_CLEAN="1":
    python3 -m unittest discover -s tools -p 'test_*.py'
    # The editor's Elm tests: its Part count is pinned to the unit's (UnitTest).
    just editor-test
    cargo test -p chimera-core -p chimera-engine -p chimera-image -p chimera-midi -p chimera-hal -p chimera-waves -p chimera-fat -p chimera-theory -p chimera-utils -p chimera-circuits --features chimera-hal/testkit
    cargo test -p chimera-core -p chimera-engine --features master-tape
    cargo test -p chimera-core -p chimera-engine --no-default-features
    cargo test -p chimera-core --features fault-inject --test console_fault_test --test console_answer_test
    # WALK's kept glyph styles (ADR 0209): host only, never in firmware.
    cargo test -p chimera-core --features walk-styles --test tape_walk_glyph_test
    # TAPE's UI with bank 2 closed, through the host's own door (ADR 0175).
    cargo test -p chimera-core --features bank2 --test tape_closed_ui_test
    cargo test -p chimera-core --features bank2-gate --test console_parse_test --test console_answer_test --test bank2_status_test --test bank2_close_test
    # TAPE's, the reverb's, VA FM's and solos' (and the solos fuzzed against their pairs), the LFO raster's, FUZZ's, FB's, SEM's, GRAIN's, the VCA gate's, ALGO's and the streamed loads' bank-1 sides through the host's own door.
    cargo test -p chimera-engine --features bank2,test-support --test tape_proxy_test --test reverb_proxy_test --test va_proxy_test --test va_solo_door_test --test va_solo_fuzz_test --test lfo_proxy_test --test fuzz_proxy_test --test fed_proxy_test --test sem_proxy_test --test stream_door_test --test glyph_door_test --test grain_proxy_test --test gate_proxy_test --test depth_proxy_test --test algo_door_test --test modal_door_test --test midi_door_test
    # ORBIT's runner and pages through the host's own door: open it plays as before the move, closed nothing (ADR 0208); a closed door keeps each Part's map record for its saves (#815).
    cargo test -p chimera-core --features bank2 --test orbit_door_test --test randomize_door_test --test drum_groups_door_test --test part_voices_door_test --test midi_ui_door_test --test midi_map_door_test --test midi_boot_door_test --test ed_door_test --test mpe_closed_door_test --test bindings_codec_test --test replace_guard_test
    just test-fat-tools
    cargo test -p chimera-desktop
    cargo build -p chimera-desktop --no-default-features
    # The default build in release too: with TAPE its dev build passes FLASH (#303).
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release
    # Every other feature set in release: a build check, not a debug flash;
    # their dev builds sit at or past FLASH
    # (private issue #303).
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release
    just check-bank1 default
    just check-bank2 default
    just check-itcm target/thumbv7em-none-eabihf/release/chimera-stm32
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --no-default-features
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --features orbit
    just check-bank1 orbit
    just check-bank2 orbit
    just check-itcm target/thumbv7em-none-eabihf/release/chimera-stm32
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --features bank2-gate
    just check-bank1 bank2-gate
    just check-bank2 bank2-gate
    just check-itcm target/thumbv7em-none-eabihf/release/chimera-stm32
    # ORBIT with usb-device writing the configuration, the static's fallback (ADR 0260).
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --features orbit,usb-config-writer
    just check-bank1 orbit,usb-config-writer
    just check-itcm target/thumbv7em-none-eabihf/release/chimera-stm32
    # The gate's doors and placement, USB audio's stream among them (ADR
    # 0256); `release-bin` checks the default and ORBIT builds'.
    {{utils}} door-check target/thumbv7em-none-eabihf/release/chimera-stm32
    just check-door-placed target/thumbv7em-none-eabihf/release/chimera-stm32
    # The bench, an area a build: all of it no longer fits FLASH. Each is
    # door-checked, as bank2-split checks the builds it splits, and held to
    # its bank-1 budget (ADR 0198).
    for f in {{bench_sets}}; do cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --no-default-features --features "$f" || exit 1; {{utils}} door-check target/thumbv7em-none-eabihf/release/chimera-stm32 || exit 1; just check-bank1 "$f" || exit 1; just check-itcm target/thumbv7em-none-eabihf/release/chimera-stm32 || exit 1; done
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --no-default-features --features sd-probe
    # Its split as `flash-sd-probe` makes it (the seal, check-itcm with ITCM empty).
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-sd-probe
    # The bootloader links cortex-m-rt's script itself: without it lld warns
    # and links an ELF with no vector table.
    out=$(cargo build -p chimera-bootloader --target thumbv7em-none-eabihf 2>&1) || { echo "$out"; exit 1; }; echo "$out"; if echo "$out" | grep -q "cannot find entry symbol"; then exit 1; fi
    rust-objdump -h target/thumbv7em-none-eabihf/debug/chimera-bootloader | grep -q '[.]vector_table' || { echo "chimera-bootloader: no .vector_table" >&2; exit 1; }
    cargo build -p chimera-theory --target thumbv7em-none-eabihf
    just clippy
    just loom
    cargo fmt --all -- --check
    just release-bin
    # The single-cut sweep over the linked ORBIT .bk2 (ADR 0272), release.
    CHIMERA_BK2={{justfile_directory()}}/target/release-bin/chimera-orbit.bk2 cargo test --release -p chimera-image --test install_test -- --ignored single_cuts_through_the_linked_image
    just stack-check

# The tear pair matrix with words in every sector (ADR 0272): ~2.5 min in
# release, so not in `check`. Run it before any PR that touches
# chimera-image, the slot layout or bank2.x.
tear-matrix:
    cargo test --release -p chimera-image --test install_test -- --ignored two_tears_with_words_in_every_sector --nocapture

# The stack is the 63.75 KB of DTCM below the wave tables (ADR 0025): fail if any release function moves
# SP by 8 KB or more in one step, or by a register (a large value built on
# the stack instead of in a static); then `tools/stack_depth.py` sums nested
# frames from `main` and from each interrupt entry (the audio's DMA1_STR0,
# render_half and the FX bus under it, MIDI's USART1 and USB's OTG_FS, one
# level charged its deeper, SysTick, HardFault), all nested at once, against
# the stack, each call through a pointer charged the deepest address-taken
# function (bank 2's door table among the data dumped, so a door entry is
# one; the USB poll's class calls the deepest class method), three deep;
# and that the audio
# interrupt reaches no small-profile code (ADR 0108); each run three times
# under different hash seeds, all alike. Checked for the default,
# no-default, orbit, bank2-gate, every bench set (`bench_sets`) and sd-probe feature
# sets (sd-probe has no interrupts: frames only).
# Needs the llvm-tools rustup component; a missing llvm-objdump or an
# unreadable ELF fails the recipe instead of passing silently.
stack-check:
    #!/usr/bin/env bash
    set -euo pipefail
    bin="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin"
    objdump="$bin/llvm-objdump"
    nm="$bin/llvm-nm"
    test -x "$objdump" && test -x "$nm"
    elf=target/thumbv7em-none-eabihf/release/chimera-stm32
    sets=("" "--no-default-features" "--features orbit" "--features bank2-gate")
    for f in {{bench_sets}}; do sets+=("--no-default-features --features $f"); done
    sets+=("--no-default-features --features sd-probe")
    for features in "${sets[@]}"; do
        cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf $features
        test -r "$elf"
        dis="$("$objdump" -d --mcpu=cortex-m7 --no-show-raw-insn -C "$elf")"
        data="$("$objdump" -s -j .rodata -j .data -j .bank2 "$elf")"
        if grep -E -e '^[0-9a-f]{8} <' -e 'sub(\.w)?[[:space:]]+sp, (sp, )?(#0x([2-9a-f][0-9a-f]{3}|[0-9a-f]{5,})$|r[0-9])' <<<"$dis" \
            | grep -B1 -E '^[[:space:]]' \
            | grep -v -e '^--$'; then
            echo "stack-check: large stack frame (features: ${features:-default})" >&2
            exit 1
        fi
        if [[ "$features" != *sd-probe* ]]; then
            addr() { "$nm" "$elf" | awk -v s="$1" '$3 == s { print strtonum("0x" $1) }'; }
            echo "stack-depth (features: ${features:-default})"
            # Three hash seeds, one answer (#582): the walk's order never moves it.
            stack=$(( $(addr _stack_start) - $(addr _stack_end) ))
            runs=()
            for seed in 0 1 2; do
                runs+=("$(PYTHONHASHSEED=$seed python3 tools/stack_depth.py "$stack" <<<"$dis"$'\n'"$data"; echo "exit $?")")
            done
            echo "${runs[0]%exit *}"
            if [[ "${runs[0]}" != "${runs[1]}" || "${runs[0]}" != "${runs[2]}" ]]; then
                echo "stack-depth: the answer changes with PYTHONHASHSEED" >&2
                exit 1
            fi
            [[ "${runs[0]}" == *"exit 0" ]]
        fi
    done

# The source table's gate under loom (ADR 0087): every interleaving of
# one audio block loop and one UI close, wait, move and open; and a stream
# slot's want, cancel and fill against the card's claim; and the card
# bus's wakeups, no want stranded. Its own target dir: `--cfg loom` would
# rebuild everything else.
loom:
    RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p chimera-core --features test-support --test loom_source_table --test loom_stream_slot --test loom_card_bus --release

# Run tests
test:
    cargo test -p chimera-core -p chimera-engine -p chimera-image -p chimera-midi -p chimera-hal -p chimera-waves -p chimera-fat -p chimera-theory -p chimera-utils -p chimera-circuits --features chimera-hal/testkit
    cargo test -p chimera-desktop --lib --tests
    cargo test -p chimera-core --features master-tape --test screen_golden_test
    just test-fat-tools
    just test-firmware-features

# TAPE's load tests under the shipping firmware's own core and engine
# features, read from Cargo's resolve of `--features orbit`: a gate the
# host's defaults hide but the firmware lacks fails here.
test-firmware-features:
    #!/usr/bin/env bash
    set -euo pipefail
    resolved() { cargo tree -q -p chimera-stm32 --target thumbv7em-none-eabihf --features orbit -e features -i "$1" --prefix none --format '{p} [{f}]' | sed -n "s/^$1 v.*\[\(.*\)\]$/\1/p" | head -1; }
    core=$(resolved chimera-core); engine=$(resolved chimera-engine)
    [[ ",$core," == *,tape,* ]] || { echo "the firmware's chimera-core has no tape: [$core]" >&2; exit 1; }
    f="$core,$(sed 's|\([^,]*\)|chimera-engine/\1|g' <<<"$engine")"
    echo "firmware features: $f"
    cargo test -p chimera-core --no-default-features --features "$f" --test tape_part_load_test --test tape_source_cell_test --test turn_reaches_engine_test

# The FAT layer checked by dosfstools (ADR 0048): mkfs.fat images must read
# back and fsck.fat -n must pass, after the suite and after console puts
# that a device error stopped, and a stock PreenFM3 tree kept bit-identical (#405). The tests are #[ignore]d so a plain
# `cargo test` stays hermetic; without dosfstools this fails with an install
# message, it never skips.
test-fat-tools:
    cargo test -p chimera-fat --features chimera-hal/testkit --test dosfstools_test --test console_put_test --test stock_files_test -- --ignored

# Clippy on every target and feature set `check` builds, test targets included
clippy:
    cargo clippy -p chimera-core -p chimera-engine -p chimera-image -p chimera-midi -p chimera-hal -p chimera-desktop -p chimera-waves -p chimera-fat -p chimera-theory -p chimera-utils -p chimera-circuits --features chimera-hal/testkit --all-targets -- -D warnings
    cargo clippy -p chimera-core -p chimera-engine --features master-tape --all-targets -- -D warnings
    cargo clippy -p chimera-core -p chimera-engine --no-default-features --all-targets -- -D warnings
    cargo clippy -p chimera-desktop --no-default-features --all-targets -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --no-default-features -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --features orbit -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --features bank2-gate -- -D warnings
    cargo clippy -p chimera-core --features bank2-gate --all-targets -- -D warnings
    cargo clippy -p chimera-core --features walk-styles --test tape_walk_glyph_test -- -D warnings
    for f in {{bench_sets}}; do cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --no-default-features --features "$f" -- -D warnings || exit 1; done
    # Every bench area at once: too big to link, but no area's code is dead.
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --no-default-features --features {{bench_all}} -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --no-default-features --features {{bench_all}},master-tape -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --no-default-features --features sd-probe -- -D warnings
    # usb-console alone: `stats` answers from no reader
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --no-default-features --features usb-console -- -D warnings
    cargo clippy -p chimera-bootloader --target thumbv7em-none-eabihf -- -D warnings

# Render every screen (the golden cases and the atlas) with the real renderer
# and write docs/screens/<case>.png at 2x (nearest neighbour), replacing the
# old set. Needs ImageMagick (`magick`).
# SCREEN_DUMP must be absolute: cargo runs the test binary with its CWD set
# to chimera-core/, not the workspace root, so a relative path lands there.
screens:
    rm -rf target/screens docs/screens/*.png && mkdir -p target/screens docs/screens
    SCREEN_DUMP="$(pwd)/target/screens" cargo test -p chimera-core --test screen_golden_test --test screen_atlas_test -q
    for f in target/screens/*.ppm; do magick "$f" -filter point -resize 200% "docs/screens/$(basename "$f" .ppm).png"; done

# Read the unit over USB (CHIMERA_USB=sim for the desktop sim)
usb +cmd:
    python3 tools/chimera-usb.py {{cmd}}
shot *args:
    python3 tools/chimera-usb.py shot {{args}}
stats:
    python3 tools/chimera-usb.py stats
status:
    python3 tools/chimera-usb.py status
# About 10 s, up to 45 the first time while it writes two 4 MiB files in
# /CHIMERA/BENCH; it plays a chord on channel 1. `just card seed=N` repeats
# a run's random reads; `just card clean` removes /CHIMERA/BENCH.
# Bench beds: `beds write DIR` (chirp WAVs), `beds check OUT.CHK bed0.SRC …` (the CRC table `put-beds` sends)
beds *args:
    {{utils}} beds {{args}}
# Drop-outs in a MOTU recording of stream 0: `just dropouts find rec.wav --ref bed0.wav [--channel N]`
dropouts *args:
    {{utils}} dropouts {{args}}
# The card's read speed beside audio and the streams it holds (a bench build first: `bench-stress` or `bench-tape`)
card *args:
    python3 tools/chimera-usb.py card {{args}}
# A project file onto the card over USB (ADR 0092), saved as the unit saves:
# `just put avril14.prj /CHIMERA/PROJECTS/P0000014.A`. 1 MiB at most; never the open project.
put local remote:
    python3 tools/chimera-usb.py put "{{local}}" "{{remote}}"
# A TAPE source onto the card (ADR 0106), in checked chunks, sealed:
# `just put-tape chimera-card/CHIMERA/TAPES/0BADF00D.SRC`. A re-run answers OK PRESENT.
put-tape file:
    python3 tools/chimera-usb.py put "{{ if file =~ '^/' { file } else { invocation_directory() / file } }}" /CHIMERA/TAPES/
# The bench's check table onto the card (the product build runs; 9 KiB at most): `just put-beds BEDS.CHK`
put-beds file:
    python3 tools/chimera-usb.py put "{{ if file =~ '^/' { file } else { invocation_directory() / file } }}" /CHIMERA/BENCH/BEDS.CHK
# The project files on the card and their sizes; `just ls tapes`: the sources
ls *args:
    python3 tools/chimera-usb.py ls {{args}}
# The lost-cluster sweep at the next boot (ADR 0094): the unit restarts into it; `just status` reads it
sweep:
    python3 tools/chimera-usb.py sweep

# Flash firmware to PreenFM3, both halves in one go (`chimera-usb.py update
# <bin> <bk2>`; a tester copies the .bk2 to the card root instead): the .bk2 staged on the card under the
# running firmware (or after the flash, for one from before bank 2), the
# .bin by DFU (`to-dfu` sends `dfu` with the console up; without it, bridge
# BOOT0 and re-plug; dfu-util's 74 after `:leave` counts only if the
# download completed), the first boot installing the .bk2, then `status`
# showing it open.
flash:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera
    python3 tools/chimera-usb.py update target/chimera.bin target/chimera.bk2

# Flash one bench area to PreenFM3 via DFU: `just flash-bench <area> [extra]`,
# area one of algo, routing, memory, stress, orbit, tape; extra more
# features (`just flash-bench stress orbit`: STRESS plays ORBIT;
# `just flash-bench algo master-tape`: the REEL row). A bench's .bk2 is
# split, not installed: under a console build first, `python3
# tools/chimera-usb.py bank2 target/chimera-bench.bk2` stages it (exit 2,
# needs firmware) and the bench's first boot installs it; without that, the
# bench can't run (it lives in bank 2, ADR 0194). All of the bench no
# longer fits FLASH at once; a bench build has no console `put` (re-flash
# the shipping build to move files). The bench holds its screens for minutes
# before the UI loop (and so the console) starts, so a 74 after a
# completed download is accepted unwaited.
flash-bench area extra="":
    #!/usr/bin/env bash
    set -euo pipefail
    features="bench-{{area}}"
    if [ -n "{{extra}}" ]; then features="$features,{{extra}}"; fi
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features --features "$features"
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-bench
    echo "bank 2 not installed by this: stage target/chimera-bench.bk2 first (chimera-usb.py bank2, under a console build)"
    python3 tools/chimera-usb.py to-dfu
    log=$(mktemp) && trap 'rm -f "$log"' EXIT
    rc=0 && dfu-util -a0 -d 0x0483:0xdf11 -D target/chimera-bench.bin -s 0x8020000:leave 2>&1 | tee "$log" || rc=$?
    if [ "$rc" -eq 74 ] && grep -q 'File downloaded successfully' "$log"; then
        echo "bench running ~4 min; console appears after"
    elif [ "$rc" -ne 0 ]; then exit "$rc"; fi

# Flash the fault-injection build (console `fault dma <1|2>`, for #25;
# `fault card`) with its bank 2, both halves as `flash` does: `fault-inject`
# less `put`, so TAPE streams and `fault card` has reads to fail. A test
# build only.
flash-fault:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features --features fault-inject
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-fault
    python3 tools/chimera-usb.py update target/chimera-fault.bin target/chimera-fault.bk2

# Copy the stock bootloader (0x08000000, 128 KB) to ~/chimera-backups with a
# .sha256, never over an existing file. Upload only: it never writes the
# unit. Needs DFU (the console's `dfu` or the BOOT0 jumper). Never commit the
# copy; docs/recovery.md has the restore.
backup-bootloader:
    #!/usr/bin/env bash
    set -euo pipefail
    out=~/chimera-backups/preenfm3-bootloader-$(date +%F).bin
    if [ -e "$out" ]; then echo "backup-bootloader: $out exists, not overwriting" >&2; exit 1; fi
    mkdir -p ~/chimera-backups
    trap 'rm -f "$out"; exit 1' ERR INT TERM  # a failed or interrupted read leaves no partial backup
    python3 tools/chimera-usb.py to-dfu
    dfu-util -a0 -d 0483:df11 -s 0x08000000:131072 -U "$out"
    test "$(stat -c %s "$out")" -eq 131072
    trap - ERR INT TERM
    cd ~/chimera-backups && sha256sum "$(basename "$out")" > "$(basename "$out").sha256"
    echo "$out"

# Flash the SD bring-up probe (--no-default-features --features sd-probe) to PreenFM3 via DFU
flash-sd-probe:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features --features sd-probe
    just bank2-split target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-sd-probe
    dfu-util -a0 -d 0x0483:0xdf11 -D target/chimera-sd-probe.bin -s 0x8020000:leave

# Coverage-guided fuzz of the Sound and project decoders, the library peek, seeded with the
# v1 fixtures and NEW, so ORBIT's records are in the corpus from the start
# (and both map records, 0x001E and 0x001F, in the Sound seeds),
# and of the TAPE importer's WAV reader, seeded with small WAVs; the TX81Z
# bank parser and map first, from an empty corpus (never a real bank)
# (needs nightly and cargo-fuzz)
fuzz:
    cd chimera-desktop/fuzz && cargo +nightly fuzz run parse_tx -- -max_total_time=300
    mkdir -p chimera-core/fuzz/corpus/decode_sound chimera-core/fuzz/corpus/decode_project
    cp chimera-core/tests/fixtures/v1/*.snd chimera-core/fuzz/seeds/decode_sound/*.snd chimera-core/fuzz/corpus/decode_sound/
    cp chimera-core/tests/fixtures/v1/project.prj chimera-core/fuzz/corpus/decode_project/
    cp chimera-core/tests/fixtures/v1/orbit.prj chimera-core/fuzz/seeds/decode_project/*.prj chimera-core/fuzz/corpus/decode_project/
    cd chimera-core/fuzz && for t in decode_sound decode_project peek_library; do cargo +nightly fuzz run $t -- -max_total_time=300; done
    mkdir -p chimera-desktop/fuzz/corpus/read_wav
    cp chimera-desktop/fuzz/seeds/read_wav/*.wav chimera-desktop/fuzz/corpus/read_wav/
    cd chimera-desktop/fuzz && cargo +nightly fuzz run read_wav -- -max_total_time=300

# chimera-utils (ADR 0098): Chimera's DSP against external references, on
# the host, built in the `utils` profile. Paths from the environment
# (chimera-utils/README.md); outputs in target/mv. Flags such as --jobs N go
# after the arguments.

# REF (a program on the contract, or midiverb) vs a Chimera DEVICE
ab ref device *knobs:
    {{utils}} ab {{ref}} {{device}} {{knobs}}

# MidiVerb II references (default programs 1-49)
mv-refs *progs:
    {{utils}} mv-refs {{progs}}

# Fit a reverb TYPE's knobs to MidiVerb II programs
mv-fit type +progs:
    {{utils}} mv-fit {{type}} {{progs}}

# Re-measure a TYPE's fits: residuals and PASS/FAIL per preset
mv-verify type *flags:
    {{utils}} mv-verify {{type}} {{flags}}

# RMS-matched A/B WAVs of a TYPE's fits on the listening set
mv-ab type:
    {{utils}} mv-ab {{type}}

# A MidiVerb II program's structure from the decompiled listing
mv-structure +progs:
    {{utils}} mv-structure {{progs}}

# The chorus slot's TYPEs on a stab and a sine, stereo WAVs into dir, and each TYPE's host cost (#794)
chorus-listen dir:
    {{utils}} chorus-listen {{dir}}

# DRUM's host prototypes (0.4, #214): each machine's hits and pattern, a kit, COMBINE, low-bit clips
drum-render dir="../drum-renders":
    {{utils}} drum-render {{dir}}

# HARMONIC engine host prototype (0.4): auditions of partial count, map, DENSITY, modules, morph
harmonic-render dir="../harmonic-renders":
    {{utils}} harmonic-render {{dir}}

# VA's macro oscillator KINDs, host prototypes: HARMONIC, SWARM, PARTICLE phrases and sweeps
macro-render dir="../macro-osc-renders":
    {{utils}} macro-render {{dir}}

# TAPE's Hermite read against a polyphase sinc, 24 kHz source, -12..+24 st (#698)
tape-interp-ab dir:
    {{utils}} tape-interp-ab {{dir}}

# Loudness, vibrato, drift, breath share, formants and vowel motion of WAVs
voice-stats +wavs:
    {{utils}} voice-stats {{wavs}}

# Waveforms, spectra and envelope-peak tracks of WAVs side by side, as an SVG
voice-plot out +wavs:
    {{utils}} voice-plot {{out}} {{wavs}}
