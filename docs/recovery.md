# Recovery: the stock bootloader and a clean reflash

The PreenFM3 runs two programs from flash: the stock bootloader at
`0x08000000` (128 KB), which runs first on every reset, and Chimera at
`0x08020000`. `just flash` only ever writes `0x08020000`. The bootloader is
never touched, but if it were ever lost or damaged, this page puts it back.

## The bootloader is Ixox's, and it stays out of this repo

The bootloader is Xavier Hosxe's (Ixox), from the stock PreenFM3 firmware:
github.com/Ixox/preenfm3 (`bootloader/`). It is not Chimera's code, so it
is deliberately not in this repo: neither its source nor a binary read off
a unit. A backup is personal and stays on your machine. Never `git add` it.

## Taking a backup

```
just backup-bootloader
```

- It needs the unit in ROM DFU (`0483:df11`). With Chimera running and the
  console up, the recipe gets there itself (`tools/chimera-usb.py to-dfu`
  sends `dfu`). Otherwise bridge BOOT0 on the back and re-plug first.
- It reads (`dfu-util -U`, upload only) 131072 bytes from `0x08000000` into
  `~/chimera-backups/preenfm3-bootloader-<YYYY-MM-DD>.bin`, and writes a
  `.sha256` beside it. It never writes to the unit.
- It refuses to overwrite a backup that already exists. A failed read leaves
  no partial file.
- Afterwards the unit is still in DFU: power-cycle it (jumper off) to play.

The backup lives in `~/chimera-backups/`. Keep a second copy somewhere off
the machine too.

## Restoring

Both steps need the BOOT0 jumper bridged and the unit re-plugged, so it
enumerates as `0483:df11` (`dfu-util -l` lists it). A broken bootloader
means Chimera's `dfu` cannot be reached, so the jumper is the way in.

1. Check the backup:

   ```
   cd ~/chimera-backups && sha256sum -c preenfm3-bootloader-<date>.bin.sha256
   ```

2. Write the bootloader back at `0x08000000`:

   ```
   dfu-util -a0 -d 0483:df11 -s 0x08000000 -D ~/chimera-backups/preenfm3-bootloader-<date>.bin
   ```

3. Write Chimera at `0x08020000`, and leave DFU:

   ```
   cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
   rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera.bin
   dfu-util -a0 -d 0483:df11 -D target/chimera.bin -s 0x8020000:leave
   ```

   (`just flash` does the same with the jumper bridged.)

4. Take the jumper off and power-cycle. The bootloader runs and jumps to
   Chimera.

## Never write the option bytes

The ROM loader offers a second DFU alternate setting, alt 1, `@Option
Bytes`. Never write to it: no `-a1` with `-D`. The option bytes hold the
read protection level and the boot addresses, and a bad write can lock
the chip or stop the ROM loader from starting, which no jumper undoes.
Every command here and in the `Justfile` uses `-a0`, internal flash, only.

The ROM loader itself is in system memory and cannot be written, so with
the jumper and a bootloader backup the unit can always be brought back.
