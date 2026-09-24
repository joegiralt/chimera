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
# type-check, firmware build + link into its flash/RAM regions.
# The desktop needs ALSA's pkg-config file; point PKG_CONFIG_PATH at it if it
# is not installed system-wide (cargo inherits the variable).
check:
    cargo test -p chimera-core -p chimera-hal
    cargo check -p chimera-desktop
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf

# Run tests
test:
    cargo test -p chimera-core -p chimera-hal

# Clippy
clippy:
    cargo clippy -p chimera-core -p chimera-hal -p chimera-desktop -- -D warnings

# Flash firmware to PreenFM3 via DFU
flash:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
    rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin
    dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000:leave
