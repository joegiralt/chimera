# Justfile for Chimera

# Build desktop simulator
desktop:
    cargo run -p chimera-desktop

# Build STM32 firmware (release)
firmware:
    cargo build --release -p chimera-stm32

# Build all (desktop targets only)
build:
    cargo build -p chimera-core -p chimera-hal -p chimera-desktop

# Check everything compiles (desktop targets)
check:
    cargo check -p chimera-core -p chimera-hal -p chimera-desktop

# Run tests
test:
    cargo test -p chimera-core -p chimera-hal

# Clippy
clippy:
    cargo clippy -p chimera-core -p chimera-hal -p chimera-desktop -- -D warnings

# Flash firmware to PreenFM3 via DFU
flash:
    cargo build --release -p chimera-stm32
    arm-none-eabi-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin
    dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000
