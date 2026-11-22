# Recovery: the stock bootloader and a clean reflash

The PreenFM3 runs two programs from flash: the stock bootloader at
`0x08000000` (128 KB), which runs first on every reset, and Chimera at
`0x08020000`. `just flash` only ever writes `0x08020000`. The bootloader is
never touched, but if it were ever lost or damaged, this page puts it back.

## The bootloader is Ixox's

The bootloader is Xavier Hosxe's (Ixox), from the stock PreenFM3 firmware:
github.com/Ixox/preenfm3 (`bootloader/`). It is not Chimera's code, and
Chimera never writes it.

Chimera doesn't carry a copy. The bootloader is Ixox's software, with
Ixox's own update mechanism and instructions, and Chimera rides on it (ADR
0314):

- **Where to get it:** the stock PreenFM3 firmware releases,
  https://github.com/Ixox/preenfm3/releases, and the source in that repo's
  `bootloader/`. Follow Ixox's instructions there to install or restore it.
- **The version Chimera expects:** bootloader 1.09, `p3_boot_1_09.bin`,
  inside the stock v1.03 release's `pfm3_firmware_1_03.zip`
  (https://github.com/Ixox/preenfm3/releases/tag/v1.03).
- `just backup-bootloader` takes a local copy, read off your own unit, into
  `~/chimera-backups/`.

If Ixox ships a new bootloader (a PreenFM3 2.0, say), Chimera follows it:
check it against Chimera on a unit, then update this page.

## Taking a backup

```
just backup-bootloader
```

- It needs the unit in ROM DFU (`0483:df11`). With Chimera running and the
  console up, the recipe gets there itself (`tools/chimera-usb.py to-dfu`
  sends `dfu`). Otherwise bridge BOOT0 on the back and re-plug first.
  On macOS and Windows the tool finds the unit with pyserial
  (`pip install pyserial`) and `dfu-util -l`; Linux needs neither.
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

2. Write the bootloader back at `0x08000000` from your backup (without one,
   use Ixox's release and instructions, above):

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

## Back in DFU after a flash with the jumper on

With BOOT0 bridged, a `to-dfu` (or OS UPGRADE's `ENTER DFU`) leaves
Chimera's DFU marker set: the reset it asks for goes straight to the ROM
loader, so nothing clears the marker. After the flash, `:leave` starts
the new image, which sees the marker and goes back into DFU. Leave DFU a
second time, flashing nothing; the first trip cleared the marker:

```
dfu-util -a0 -d 0483:df11 -s 0x8020000:leave
```

Without the jumper this never happens
(private issue #305).

## No console after a cold power-on

Known issue: private issue #331. From a cold
power-on the USB console doesn't come up: the OTG core's reset never
completes, and about a second after PART 1 the screen toasts
`USB OFF: csrst`. The synth plays normally. Once the unit has been
through the ROM loader, the console works until the next power-off.

Once per session, after powering on:

1. SETTINGS › SYSTEM › OS UPGRADE, then `ENTER DFU`. The unit enumerates
   as `0483:df11`.
2. Leave DFU, flashing nothing:

   ```
   dfu-util -a0 -d 0483:df11 -s 0x8020000:leave
   ```

   dfu-util then reports an error and exits 74: `:leave` resets the unit
   under it, so the final status request fails. That is expected (the
   `Justfile`'s `flash` recipe accepts it the same way). `just flash`
   flashes and leaves the same way.

Chimera restarts with the console up (`0483:5740`), and `just status`,
`just flash` and the rest work hands-free until the unit is powered off.

## Never write the option bytes

The ROM loader offers a second DFU alternate setting, alt 1, `@Option
Bytes`. Never write to it: no `-a1` with `-D`. The option bytes hold the
read protection level and the boot addresses, and a bad write can lock
the chip or stop the ROM loader from starting, which no jumper undoes.
Every command here and in the `Justfile` uses `-a0`, internal flash, only.

The ROM loader itself is in system memory and cannot be written, so with
the jumper and a bootloader backup the unit can always be brought back.
