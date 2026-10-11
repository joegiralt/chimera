# Justfile for Chimera

# The bench's builds, an area each (all of it no longer fits FLASH), and
# the filter probes `tools/loop_count.py` counts. Each is built with
# `--no-default-features` (the `bench` feature brings back all of the
# shipping set but the console's `put`; `filter-probes`, `default`).
bench_sets := "fault-inject bench-algo bench-algo,master-tape bench-routing bench-memory bench-stress bench-stress,orbit bench-orbit bench-tape filter-probes"
bench_all := "bench-algo,bench-routing,bench-memory,bench-stress,bench-orbit,bench-tape"

# Build desktop simulator
desktop:
    cargo run -p chimera-desktop

# TAPE's listening rig (spec § 9): each model on each source into
# target/listen/ (WAVs, report.txt, and index.html once every render passes
# QA). `just tape-listen loop --src chimera --src <file.wav>`; a bare file
# name is looked for in $CHIMERA_TAPES_SRC (~/chimera-tapes-src)
tape-listen *args:
    cargo run -q -p chimera-desktop --bin tape-listen -- {{args}}

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

# The factory set onto the unit's card: each source through `put` (ADR
# 0106; a re-run answers OK PRESENT for each). `dir` is `just
# factory-tapes`' output, or a downloaded `tapes-vN` release. The factory
# TAPE Sounds (S9xxxxxx.A) stay off the card: no firmware reads
# /CHIMERA/SOUNDS until the card library (#317).
put-factory-tapes dir="target/tapes-v1":
    #!/usr/bin/env bash
    set -euo pipefail
    shopt -s nullglob
    srcs=("{{dir}}"/*.SRC)
    [ ${#srcs[@]} -gt 0 ] || { echo "no .SRC in {{dir}}: just factory-tapes" >&2; exit 1; }
    for f in "${srcs[@]}"; do python3 tools/chimera-usb.py put "$f" /CHIMERA/TAPES/; done

# Build STM32 firmware (release)
firmware:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf

# Build STM32 firmware with ORBIT (release)
firmware-orbit:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features orbit

# Release binaries: the shipping firmware and its ORBIT variant with every build
# path remapped (#413), so no home directory, user name or registry path
# survives in the .bin. `--config` arrays append to .cargo/config.toml's
# target rustflags (RUSTFLAGS would replace them).
# Panic locations keep their relative in-crate paths (removing them needs
# nightly's -Zlocation-detail). Output: target/release-bin/chimera{,-orbit}.bin
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
        rust-objcopy -O binary target/$T/release/chimera-stm32 target/release-bin/$out.bin
        just check-bin-paths target/release-bin/$out.bin
    done

# Draft a release of the path-clean firmware on the PUBLIC chimera-releases
# repo (#413): `just release 0.1.0 [--dry-run]`. Refuses a dirty tree or a
# version already tagged or released; builds `release-bin`; names the files
# chimera-<version>{,-orbit}.bin; adds tools/chimera-usb.py (version-stamped,
# leak-checked); writes SHA256SUMS and the notes from
# docs/alpha/RELEASE-NOTES.md.tmpl; asserts the assets are exactly those
# four and none is a private file's bytes. A draft only: the owner
# publishes. --dry-run does everything but `gh release create`.
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
    out=target/release-assets/v$V
    rm -rf "$out"; mkdir -p "$out"
    cp target/release-bin/chimera.bin "$out/chimera-$V.bin"
    cp target/release-bin/chimera-orbit.bin "$out/chimera-$V-orbit.bin"
    for b in "$out"/*.bin; do just check-bin-paths "$b"; done
    # The console tool, stamped with the release version.
    sed "s/^VERSION = \"dev\"/VERSION = \"$V\"/" tools/chimera-usb.py > "$out/chimera-usb.py"
    grep -q "^VERSION = \"$V\"" "$out/chimera-usb.py" || { echo "version not stamped in chimera-usb.py" >&2; exit 1; }
    python3 -I -m py_compile "$out/chimera-usb.py"; rm -rf "$out/__pycache__"
    # It is public: no home path, user name, e-mail, URL or repo-internal reference.
    if grep -n -E -e '/home/|/Users/|[A-Za-z0-9._-]+@[A-Za-z0-9-]+\.|https?://|ADR|docs/|chimera-(core|board|engine|desktop|stm32)|\.rs\b|joegiralt' -e "$(id -un)" "$out/chimera-usb.py"; then
        echo "chimera-usb.py leaks a private reference" >&2; exit 1
    fi
    (cd "$out" && sha256sum chimera-$V.bin chimera-$V-orbit.bin chimera-usb.py > SHA256SUMS)
    # Nothing private ships: exactly these assets, and none is a private
    # file (the stock bootloader backup, the bootloader, a dev chimera.bin).
    [ "$(ls "$out" | LC_ALL=C sort | tr '\n' ' ')" = "SHA256SUMS chimera-$V-orbit.bin chimera-$V.bin chimera-usb.py " ] || { echo "unexpected assets in $out" >&2; exit 1; }
    for p in preenfm3-bootloader-backup.bin bootloader.bin chimera.bin; do
        [ -f "$p" ] || continue
        h=$(sha256sum "$p" | cut -d' ' -f1)
        if grep -q "$h" "$out/SHA256SUMS"; then echo "asset is $p: refusing" >&2; exit 1; fi
    done
    sed -e "s/@VERSION@/$V/g" -e "s/@COMMIT@/$(git rev-parse --short HEAD)/g" -e "s/@DATE@/$(date -u +%F)/g" \
        docs/alpha/RELEASE-NOTES.md.tmpl > "$out/RELEASE-NOTES.md"
    ! grep -q '@[A-Z]*@' "$out/RELEASE-NOTES.md" || { echo "unfilled template field" >&2; exit 1; }
    assets=("$out/chimera-$V.bin" "$out/chimera-$V-orbit.bin" "$out/chimera-usb.py" "$out/SHA256SUMS")
    echo "assets:"; (cd "$out" && ls -l chimera-$V.bin chimera-$V-orbit.bin chimera-usb.py SHA256SUMS && cat SHA256SUMS)
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
    cargo build -p chimera-core -p chimera-engine -p chimera-hal -p chimera-desktop

# Everything must pass before a commit (ADR 0013): core + hal tests (and core
# with the master tape back on, ADR 0055, and the whole suite without ORBIT), desktop
# tests and its no-MIDI build, the dosfstools cross-checks, the firmware built and linked with default
# features, with none, with ORBIT, each bench area (ALGO also with the master
# tape, STRESS also with ORBIT), the filter probes and the SD probe, clippy on host
# and firmware (every feature set), rustfmt and the stack check. The desktop needs ALSA's
# pkg-config file; point PKG_CONFIG_PATH at it if it is not installed
# system-wide. The host tool's tests run first, against a fake unit.
check:
    python3 -m unittest discover -s tools -p 'test_*.py'
    cargo test -p chimera-core -p chimera-engine -p chimera-hal -p chimera-waves -p chimera-fat -p chimera-theory -p chimera-utils --features chimera-hal/testkit
    cargo test -p chimera-core -p chimera-engine --features master-tape
    cargo test -p chimera-core -p chimera-engine --no-default-features
    cargo test -p chimera-core --features fault-inject --test console_fault_test --test console_answer_test
    just test-fat-tools
    cargo test -p chimera-desktop
    cargo build -p chimera-desktop --no-default-features
    # The default build in release too: with TAPE its dev build passes FLASH (#303).
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release
    # Every other feature set in release: a build check, not a debug flash;
    # their dev builds sit at or past FLASH
    # (private issue #303).
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --no-default-features
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --features orbit
    # The bench, an area a build: all of it no longer fits FLASH.
    for f in {{bench_sets}}; do cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --no-default-features --features "$f" || exit 1; done
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release --no-default-features --features sd-probe
    cargo build -p chimera-bootloader --target thumbv7em-none-eabihf
    cargo build -p chimera-theory --target thumbv7em-none-eabihf
    just clippy
    just loom
    cargo fmt --all -- --check
    just release-bin
    just stack-check

# The stack is the 63.75 KB of DTCM below the wave tables (ADR 0025): fail if any release function moves
# SP by 8 KB or more in one step, or by a register (a large value built on
# the stack instead of in a static); then `tools/stack_depth.py` sums nested
# frames from `main` and from each interrupt entry (the audio's DMA1_STR0,
# render_half and the FX bus under it, MIDI's USART1, SysTick, HardFault),
# all nested at once, against the stack, each call through a pointer
# charged the deepest address-taken function, three deep; and that the audio
# interrupt reaches no small-profile code (ADR 0108). Checked for the default,
# no-default, orbit, every bench set (`bench_sets`) and sd-probe feature
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
    sets=("" "--no-default-features" "--features orbit")
    for f in {{bench_sets}}; do sets+=("--no-default-features --features $f"); done
    sets+=("--no-default-features --features sd-probe")
    for features in "${sets[@]}"; do
        cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf $features
        test -r "$elf"
        dis="$("$objdump" -d --mcpu=cortex-m7 --no-show-raw-insn -C "$elf")"
        data="$("$objdump" -s -j .rodata -j .data "$elf")"
        if grep -E -e '^[0-9a-f]{8} <' -e 'sub(\.w)?[[:space:]]+sp, (sp, )?(#0x([2-9a-f][0-9a-f]{3}|[0-9a-f]{5,})$|r[0-9])' <<<"$dis" \
            | grep -B1 -E '^[[:space:]]' \
            | grep -v -e '^--$'; then
            echo "stack-check: large stack frame (features: ${features:-default})" >&2
            exit 1
        fi
        if [[ "$features" != *sd-probe* ]]; then
            addr() { "$nm" "$elf" | awk -v s="$1" '$3 == s { print strtonum("0x" $1) }'; }
            echo "stack-depth (features: ${features:-default})"
            python3 tools/stack_depth.py "$(( $(addr _stack_start) - $(addr _stack_end) ))" <<<"$dis"$'\n'"$data"
        fi
    done

# The source table's gate under loom (ADR 0087): every interleaving of
# one audio block loop and one UI close, wait, move and open. Its own
# target dir: `--cfg loom` would rebuild everything else.
loom:
    RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p chimera-core --features test-support --test loom_source_table --release

# Run tests
test:
    cargo test -p chimera-core -p chimera-engine -p chimera-hal -p chimera-waves -p chimera-fat -p chimera-theory -p chimera-utils --features chimera-hal/testkit
    cargo test -p chimera-desktop --lib --tests
    just test-fat-tools

# The FAT layer checked by dosfstools (ADR 0048): mkfs.fat images must read
# back and fsck.fat -n must pass, after the suite and after console puts
# that a device error stopped, and a stock PreenFM3 tree kept bit-identical (#405). The tests are #[ignore]d so a plain
# `cargo test` stays hermetic; without dosfstools this fails with an install
# message, it never skips.
test-fat-tools:
    cargo test -p chimera-fat --features chimera-hal/testkit --test dosfstools_test --test console_put_test --test stock_files_test -- --ignored

# Clippy on every target and feature set `check` builds, test targets included
clippy:
    cargo clippy -p chimera-core -p chimera-engine -p chimera-hal -p chimera-desktop -p chimera-waves -p chimera-fat -p chimera-theory -p chimera-utils --features chimera-hal/testkit --all-targets -- -D warnings
    cargo clippy -p chimera-core -p chimera-engine --features master-tape --all-targets -- -D warnings
    cargo clippy -p chimera-core -p chimera-engine --no-default-features --all-targets -- -D warnings
    cargo clippy -p chimera-desktop --no-default-features --all-targets -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --no-default-features -- -D warnings
    cargo clippy -p chimera-stm32 -p chimera-board --target thumbv7em-none-eabihf --features orbit -- -D warnings
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
# The card's read speed beside audio and the streams it holds (any `just flash-bench <area>` without ORBIT or master-tape first)
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
# The project files on the card and their sizes; `just ls tapes`: the sources
ls *args:
    python3 tools/chimera-usb.py ls {{args}}
# The lost-cluster sweep at the next boot (ADR 0094): the unit restarts into it; `just status` reads it
sweep:
    python3 tools/chimera-usb.py sweep

# Flash firmware to PreenFM3 via DFU. With the console up, `to-dfu` sends
# `dfu` and waits for the ROM loader; without it, bridge BOOT0 and re-plug.
# dfu-util exits 74 when `:leave` resets the unit under it; that counts only
# if the download completed and the console comes back within 10 s.
flash:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
    rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera.bin
    python3 tools/chimera-usb.py to-dfu
    log=$(mktemp) && trap 'rm -f "$log"' EXIT
    rc=0 && dfu-util -a0 -d 0x0483:0xdf11 -D target/chimera.bin -s 0x8020000:leave 2>&1 | tee "$log" || rc=$?
    if [ "$rc" -eq 74 ] && grep -q 'File downloaded successfully' "$log"; then
        python3 tools/chimera-usb.py wait-console 10
    elif [ "$rc" -ne 0 ]; then exit "$rc"; fi

# Flash one bench area to PreenFM3 via DFU: `just flash-bench <area> [extra]`,
# area one of algo, routing, memory, stress, orbit, tape; extra more
# features (`just flash-bench stress orbit`: STRESS plays ORBIT;
# `just flash-bench algo master-tape`: the REEL row). All of the bench no
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
    rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-bench.bin
    python3 tools/chimera-usb.py to-dfu
    log=$(mktemp) && trap 'rm -f "$log"' EXIT
    rc=0 && dfu-util -a0 -d 0x0483:0xdf11 -D target/chimera-bench.bin -s 0x8020000:leave 2>&1 | tee "$log" || rc=$?
    if [ "$rc" -eq 74 ] && grep -q 'File downloaded successfully' "$log"; then
        echo "bench running ~4 min; console appears after"
    elif [ "$rc" -ne 0 ]; then exit "$rc"; fi

# Flash the fault-injection build (console `fault dma <1|2>`, for #25) via
# DFU: the shipping set plus `fault-inject`, less `put`. A test build only.
flash-fault:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features --features fault-inject
    rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-fault.bin
    python3 tools/chimera-usb.py to-dfu
    log=$(mktemp) && trap 'rm -f "$log"' EXIT
    rc=0 && dfu-util -a0 -d 0x0483:0xdf11 -D target/chimera-fault.bin -s 0x8020000:leave 2>&1 | tee "$log" || rc=$?
    if [ "$rc" -eq 74 ] && grep -q 'File downloaded successfully' "$log"; then
        python3 tools/chimera-usb.py wait-console 10
    elif [ "$rc" -ne 0 ]; then exit "$rc"; fi

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
    rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-sd-probe.bin
    dfu-util -a0 -d 0x0483:0xdf11 -D target/chimera-sd-probe.bin -s 0x8020000:leave

# Coverage-guided fuzz of the Sound and project decoders, the library peek, seeded with the
# v1 fixtures and NEW, so ORBIT's records are in the corpus from the start,
# and of the TAPE importer's WAV reader, seeded with small WAVs
# (needs nightly and cargo-fuzz)
fuzz:
    mkdir -p chimera-core/fuzz/corpus/decode_sound chimera-core/fuzz/corpus/decode_project
    cp chimera-core/tests/fixtures/v1/*.snd chimera-core/fuzz/corpus/decode_sound/
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
utils := "cargo run -q --profile utils -p chimera-utils --"

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
