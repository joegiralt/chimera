# Justfile for Chimera

# Build desktop simulator
desktop:
    cargo run -p chimera-desktop

# Build STM32 firmware (release)
firmware:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf

# Build all (desktop targets only)
build:
    cargo build -p chimera-core -p chimera-hal -p chimera-desktop

# Everything must pass before a commit (ADR 0013): core + hal tests, desktop
# build + unit tests, firmware build + link into its flash/RAM regions,
# clippy (including test targets) and rustfmt, both clean.
# The desktop needs ALSA's pkg-config file; point PKG_CONFIG_PATH at it if it
# is not installed system-wide (cargo inherits the variable).
check:
    cargo test -p chimera-core -p chimera-hal
    cargo test -p chimera-desktop
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf
    cargo clippy -p chimera-core -p chimera-hal -p chimera-desktop --all-targets -- -D warnings
    cargo fmt --all -- --check
    just stack-check

# The stack is 128 KB of DTCM (ADR 0020): fail if any release function moves
# SP by 8 KB or more in one step, or by a register (a large value built on
# the stack instead of in a static). Needs the llvm-tools rustup component.
stack-check:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
    ! "$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-objdump" -d --no-show-raw-insn -C target/thumbv7em-none-eabihf/release/chimera-stm32 | grep -E -e '^[0-9a-f]{8} <' -e 'sub(\.w)?[[:space:]]+sp, (sp, )?(#0x([2-9a-f][0-9a-f]{3}|[0-9a-f]{5,})$|r[0-9])' | grep -B1 -E '^[[:space:]]' | grep -v -e '^--$'

# Run tests
test:
    cargo test -p chimera-core -p chimera-hal

# Clippy (including test targets)
clippy:
    cargo clippy -p chimera-core -p chimera-hal -p chimera-desktop --all-targets -- -D warnings

# Render every screen-golden case with the real renderer and write
# docs/screens/<case>.png at 2x (nearest neighbour). Needs ImageMagick (`magick`).
# SCREEN_DUMP must be absolute: cargo runs the test binary with its CWD set
# to chimera-core/, not the workspace root, so a relative path lands there.
screens:
    rm -rf target/screens && mkdir -p target/screens docs/screens
    SCREEN_DUMP="$(pwd)/target/screens" cargo test -p chimera-core --test screen_golden_test -q
    for f in target/screens/*.ppm; do magick "$f" -filter point -resize 200% "docs/screens/$(basename "$f" .ppm).png"; done

# Flash firmware to PreenFM3 via DFU
flash:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
    rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin
    dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000:leave
