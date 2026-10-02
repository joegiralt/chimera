# Recovery: the stock bootloader and a clean reflash

The PreenFM3 runs two programs from flash: the stock bootloader at
`0x08000000` (128 KB), which runs first on every reset, and Chimera at
`0x08020000`. `just flash` only ever writes `0x08020000`. The bootloader is
never touched, but if it were ever lost or damaged, this page puts it back.

## The bootloader is Ixox's

The bootloader is Xavier Hosxe's (Ixox), from the stock PreenFM3 firmware:
github.com/Ixox/preenfm3 (`bootloader/`). It is not Chimera's code, and
Chimera never writes it.

There are two copies:

- the repo keeps one, `preenfm3-bootloader-backup.bin` at the root, pinned
  to a known version;
- `just backup-bootloader` takes a local one, read off your own unit, into
  `~/chimera-backups/`.

Whether the repo keeps its copy, and how the bootloader is licensed with
Chimera, the owner decides both at V1. Until then, leave both as they are.

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
- It refuses to overwrite a backup that already exists. A failed or
  interrupted read leaves no partial file.
- Afterwards the unit is still in DFU: power-cycle it (jumper off) to play.

The local backup lives in `~/chimera-backups/`.

## Restoring

Both steps need the BOOT0 jumper bridged and the unit re-plugged, so it
enumerates as `0483:df11` (`dfu-util -l` lists it). A broken bootloader
means Chimera's `dfu` cannot be reached, so the jumper is the way in.

1. Check the backup:

   ```
   cd ~/chimera-backups && sha256sum -c preenfm3-bootloader-<date>.bin.sha256
   ```

2. Write the bootloader back at `0x08000000` (the repo's
   `preenfm3-bootloader-backup.bin` works in place of a local backup):

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
