# Flash bank 2: room to grow: Design

- **Status:** Approved 2026-10-11 (owner: "lets go with all you suggestions"); decisions below
- **Date:** 2026-10-11
- **Issue:** private issue #460 (post-alpha)
- **Builds on:** ADR 0092 (console `put`), ADR 0108 (fast audio, small rest), ADR 0122 (WAVES at boot), ADR 0132 (TAPE out of the firmware for alpha), (private notes) (the 2026-10-07 ROM DFU hang), (private notes) (bootloader 1.09)
- **Chip:** STM32H753VI (stock `bootloader/STM32H753VITX_FLASH.ld`), rev V. It has 2 MB of flash in two 1 MB banks of eight 128 KB sectors. CLAUDE.md's "H750, 128KB" line is stale.

## Owner decisions (2026-10-11)

1. **Two files per release:** `chimera-<v>.bin` through the usual DFU route, plus `chimera-<v>.bk2`, put over USB by one tool command, which the unit then installs itself.
2. **Auto-install:** a staged bank-2 file that exactly matches the running firmware installs at boot without asking, behind a `DON'T POWER OFF` screen.
3. **Audio during a write:** ALGO, MODAL and VA keep playing (read-while-write); only TAPE is silent. Phase 0b must show no overruns; if it does, the writer mutes everything instead.
4. **TAPE Parts while bank 2 is closed:** kept silent and labelled, never converted, so a stale bank 2 never costs data. ADR 0132's lossy load is superseded when bank 2 lands.
5. **Phase 0 go:** 0a (read-only option bytes) first, then 0b (Chimera erases and programs bank-2 sector 0, with a timeout and an SR2 dump).
6. **The slots at 0x08180000 / 0x081C0000 (bank-2 sectors 4–7) stay untouched for good.** Bank 2 is 0x08100000–0x0817FFFF.
7. **SWD:** not needed. The design works without it.
8. **The label:** `NEEDS UPDATE` (full words, no "bank" jargon), with the release notes saying which file to install.

## Open questions for the owner

1. **Two files a release.** A single .bin can't install both banks safely today (§ 3). Is this flow acceptable: `chimera-<v>.bin` through the usual DFU route, plus `chimera-<v>.bk2`, which one tool command puts over USB and the unit then installs itself?
2. **Auto-install.** When the card holds a staged bank-2 file that exactly matches the running firmware, should the boot install it without asking, on a "DON'T POWER OFF" screen of about 10 s? Or should it ask first?
3. **Audio during a write.** Read-while-write means ALGO, MODAL and VA can keep playing while bank 2 is written, with only TAPE silenced. Or should everything be muted?
4. **TAPE Parts while bank 2 is closed.** Keep ADR 0132's lossy load, where a TAPE Part becomes INIT VA, or keep the Part silent and labelled so a stale bank 2 never costs data? (Recommended: keep it silent and labelled; see § 2.)
5. **Phase 0b go.** Proving that code runs from bank 2 needs one erase and program of bank-2 sector 0, done by Chimera's own code (not ROM DFU). Go or no-go?
6. **Slots at 0x08180000 / 0x081C0000** (bank-2 sectors 4–7, 512 KB). Do we stay clear of them for good? (Recommended: yes.)
7. **SWD.** Does the unit have a reachable SWD header and an ST-Link? It would give a developer-only write route and a way to see fault addresses. It is not needed.
8. **The label.** Testers don't know what a bank is. Should the cell read `UPDATE BANK 2`, `NEEDS UPDATE` or something else (full-word labels rule)?

## Summary of the recommendation

- **What goes in bank 2:** code first, then data. It is **one link spanning two regions**. Bank 1 enters bank 2 through **one door**, a table of function pointers at a fixed address that bank 1 reads only after the header checks out. A post-link check fails the build if bank 1 holds any other reference into bank 2.
- **Image:** a header with magic, format, length, CRC-32, a 64-bit **seal** (a hash of bank 2's bytes, patched into bank 1 after the link) and a version string. A commit word is programmed last. Bank 1 opens the door only on an exact seal match. Otherwise TAPE (and later DRUM) reads `UPDATE BANK 2` and nothing in bank 2 is touched.
- **Writer:** Chimera writes bank 2 itself, from a file staged on the card by the console's `put`. It runs from the UI loop, uses bank-2 registers only, keeps the audio running (read-while-write) and writes the commit word last. ROM DFU and the stock bootloader are both rejected as writers.
- **First move:** TAPE's DSP. On the ORBIT build TAPE costs **46,476 B** in bank 1. Through the door it should cost about 2–4 KB there.

## Facts gathered

| Fact | Source |
|---|---|
| Firmware area 0x08020000–0x080FFFFF, 917,504 B | `chimera-stm32/memory.x` |
| Sizes today (vector + text + rodata + data). Shipping: 827,796 B, so **89,708 free**. ORBIT: 846,936, so **70,568 free**. ORBIT+TAPE: 893,412, so **24,092 free** | built at 46eb6489, 2026-10-11 |
| TAPE-named symbols are 40,928 B with `tape`, 8,146 B without (params, UI, `off`). The other ~13.7 KB of the 46.5 KB delta is TAPE code inlined into `EngineSlot`/`Instrument` render | `rust-nm -S`, same builds |
| Cold rodata is small. The five u8g2 fonts are 10.3 KB, `factory.bin` 2 KB, the splash 1 KB. WAVES is already gone (ADR 0122) | file sizes |
| Bootloader 1.09 jumps to 0x08180000 if button index 9 is held at power-on, to 0x081C0000 for index 10, and otherwise (unless MENU) to 0x08020000. It never reads bank 2 for anything else | ixox/preenfm3 `bootloader/Src/main.c:116-127` |
| The stock bootloader **never erases or writes bank 2**. SD flash erases sectors `1..size/128K+1` with `Banks = FLASH_BANK_1` in every public version (2e309043, 34a7ab17, c5be5695). `FLASH_If_Erase`, which mass-erases bank 2, is never called | `bootloader.cpp formatFlash`, `flash_if.c:55-104` |
| The stock *firmware* never programs flash. Only the bootloader references the slots | GitHub code search |
| Stock firmware links FLASH as 1920 K from 0x08020000. One image spanning both banks is how ST intends the part to be used | `firmware/preenfm3_2MB.ld:50` |
| ROM DFU, 2026-10-07: a 64 KB read at 0x08100000, 0x08180000 and 0x081E0000 read all 0xFF with no errors. `dfu-util -D` of 128 KB at 0x08100000 hung in erase (dfuDNBUSY for more than 6 min) and needed a power cycle. Bank 1 was untouched | `flash-diet.md` lines 3–4 |
| Option bytes, WRP and SWAP_BANK have **never been read** on this unit | none found |

**The 2026-10-07 hang, what is known.** The erase was ROM DFU's own, entered through Chimera's software jump (`to-dfu`, with the clocks at 480 MHz, the backup domain set and SysTick stopped) rather than through BOOT0. Bank-1 writes through the same route work on every `just flash`. Nothing was logged from FLASH_SR2. The candidates, none confirmed:
- (a) bank-2 sectors are write-protected in the option bytes (WPSN_CUR2R), and the ROM loops on WRPERR;
- (b) the ROM's bank-2 erase misbehaves after a software jump, given the clock, voltage-scaling or flash-latency state the jump leaves;
- (c) a ROM bootloader limitation for dual-bank H74x/75x (check AN2606's known-limitations table for this ROM version).

Phase 0a reads WPSN_CUR2R, OPTSR_CUR and SR2, which settles (a) without writing anything. Phase 0b's Chimera-driven erase, with a timeout and an SR2 dump, separates (b) and (c) from a hardware problem.

**A hazard found in the stock bootloader (source reading only; never test it).** `formatFlash` erases `Sector = k+1` for `k < size/131072+1`. For a .bin of **917,504 B or more**, `k+1` reaches 8. With `USE_FULL_ASSERT` off (its `hal_conf.h:209`), `FLASH_Erase_Sector` ORs `8 << SNB_Pos` (bit 11, reserved per RM0433 FLASH_CR1) into CR1 and leaves SNB at **0**. That erases the bootloader's own sector, and only the BOOT0 jumper recovers from it. So:
- a combined two-bank .bin must never go to the card root;
- `just release-bin` should assert every bank-1 .bin is **strictly under** 917,504 B (today the linker allows exactly 917,504);
- the bank-2 file must not be a `.bin`, because the bootloader lists every root `*.bin` as flashable.

*Verify: RM0433 FLASH_CR1 bit 11, and that a 917,504 B image cannot link at exactly the limit.*

## 1. What lives in bank 2

| Option | Gain now | Cost / risk |
|---|---|---|
| (a) Data only | About 13 KB today (fonts, `factory.bin`, splash). Fonts can't move: the UI needs a font to say `UPDATE BANK 2`. The INITs in `factory.bin` must work without bank 2 | Simplest: no code crosses. **Little to move today.** Valuable later for big tables (VA macro-oscillator waves, DRUM tables) |
| (b) Code | TAPE 46.5 KB now, DRUM later; the reverb TYPEs (~65 KB, flash audit) if ever needed | Calls cross the boundary; the gating has to be total |
| **(c) Both, code first (recommended)** | As (b), and data uses the same header and door when a big table arrives | Only one mechanism, because data is just `&'static [u8]` behind the door |

**How code in bank 2 is linked.**

| | One image, two regions (**recommended**) | Two images, stable ABI |
|---|---|---|
| Build | One `cargo build`; the linker places `.bank2` in a second MEMORY region; objcopy splits it into two files | Two binaries and two link scripts |
| Crossing | Rust calls through a door table (same compiler, same link, so no ABI question). Bank-2 code calls `core`, `libm` and engine helpers in bank 1 directly | `extern "C"` and `#[repr(C)]` for every shared type. Bank 2 needs its own `core`, `libm` and compiler-builtins (duplicated KB). Rust has no stable ABI for `TapeEngine`'s state |
| Coupling | Bank 2 matches exactly one bank 1, by seal. A firmware change that moves anything bank 2 calls means a bank-2 rewrite | Bank 2 survives firmware updates while its ABI version holds |
| Verdict | Simple and exact. The cost is that releases usually ship both files | Revisit only if testers find the two-step update a real burden |

**Placement.** The TAPE DSP moves into its own crate, `chimera-tape`, built at opt 2 for the audio (ADR 0108). A linker script that `build.rs` writes, `bank2.x`, holds a `.bank2` output section `> BANK2`, with `BANK2 (rx) : ORIGIN = 0x08100000, LENGTH = 512K` added to `memory.x`:
- `KEEP(*(.bank2.header))` at offset 0, then `KEEP(*(.bank2.door))`;
- then every `.text.*` and `.rodata.*` input section whose mangled name holds the crate (`*12chimera_tape*`; this build uses v0 mangling, `_RNv…14chimera_engine…`);
- then explicit `#[link_section = ".bank2.text.*"/".bank2.rodata.*"]` items.

rustc emits one section per function on ELF targets even under fat LTO with one codegen unit, so name globs still work after LTO. *Verify on the first spike with `rust-nm`: monomorphs, closures and `<T as Trait>` impls of the crate all land in bank 2.*
- **Script order.** An input section goes to the *first* output statement that matches it. cortex-m-rt's `.text : { *(.text .text.*) }` would grab ours, so `bank2.x` uses `INSERT BEFORE .text`. *Check rust-lld's behaviour.*
- **Asserts.** `ADDR(.bank2) == 0x08100000` and `ADDR(.bank2) + SIZEOF(.bank2) <= 0x08180000` ("clear of bootloader 1.09's slots").

**Rust codegen.**
- **No veneers or long calls.** Thumb-2 `BL` reaches ±16 MB, and bank 1 to bank 2 spans at most 1.5 MB, so direct calls from bank 2 into bank 1 are plain `BL`. rust-lld inserts no thunks, and no `-mlong-calls` equivalent is needed.
- **Inlining.** Fat LTO may inline bank-1 helpers into bank-2 code (fine: copies in bank 2). The reverse would put bank-2 code into bank 1, and the door prevents it. Bank 1 reads the door table from the **raw address** `0x0810_0000 + door_offset` with `read_volatile`, never through the Rust static, so LLVM can't devirtualise or inline a door function into a bank-1 caller. The door static is `#[used]`, and KEEP covers `--gc-sections`.
- **The door check.** Link with `--emit-relocs`. A host check (chimera-utils, Rust) fails the build if any relocation in a bank-1 section resolves into 0x08100000–0x0817FFFF. It catches `#[inline]` generics from `chimera-tape` instantiated in bank 1, a stray `Clone` and the like. The door read is a literal address, not a relocation, so the rule is simply zero.
- **Proxy.** `TapeEngine` in bank 1 becomes a proxy with today's surface. When closed it behaves as `off.rs` does now. When open it forwards each call through the door's `TapeDoor` table (`fn` pointers: `init_in_place`, `note_on`, `run`, `strip`/`restore`, …). The state struct's type comes from `chimera-tape`; its layout needs no code. That is one indirect call per voice per block, which is noise.
- **MPU.** Region 1 makes 0x08100000–0x081FFFFF (1 MB) no-access. Region 2 covers 0x08100000 for 512 KB: read-only XN for the header check, then read-only executable once the door opens, and no-access again before an erase. A stray access then faults with a clear address instead of reading erased or torn flash, and Cortex-M7 speculative fetches can't reach a bank that is mid-erase. The firmware then never touches the bootloader slots.
- **Caches.** After programming: clean and invalidate the D-cache by address over the written range, invalidate the whole I-cache, then DSB and ISB.

## 2. Image format and versioning

The header sits at 0x08100000, 256 B (eight 32-byte flash words), little-endian:

| Off | Field | Notes |
|---|---|---|
| 0x00 | `magic: [u8; 8]` | `CHIMERA2` |
| 0x08 | `format: u16`, `flags: u16` | header layout version; flags reserved, 0 |
| 0x0C | `body_len: u32` | bytes after the header, ≤ 512 K − 256 |
| 0x10 | `body_crc: u32` | CRC-32/ISO-HDLC, the table `storage::crc` already has |
| 0x14 | `door_off: u32` | from 0x08100000, 32-byte aligned, inside the body |
| 0x18 | `seal: [u8; 8]` | first 8 bytes of SHA-256 over the body, computed on the host |
| 0x20 | `version: [u8; 32]` | `0.2.0 02e440e2`, the build stamp the Sound library already shows |
| 0xE0 | `commit: [u32; 8]` | its own flash word: `0xB2C0_FFEE` ×8 means committed. Erased is 0xFF…, and anything else is torn |

**Seal.** Bank-2 bytes come from one link and hold absolute and PC-relative references to bank 1. If two links produce the same bank-2 bytes, every reference in them is right for the bank 1 of either link. Hashing bank 2's bytes is therefore an exact compatibility key.
- After the link, `chimera-utils bank2-seal` fills `body_len`, `body_crc`, `seal` and `door_off`.
- It patches the same seal into bank 1's `#[used] static BANK2_SEAL: [u8; 8]`, found by symbol in the ELF and read with `read_volatile` so the placeholder is never constant-folded.
- A firmware change that leaves bank 2's bytes alone keeps the seal, so **no bank-2 rewrite is needed**. That also saves wear and spares testers a step.

**The judgement** is a pure function in a small `no_std` crate, `chimera-image`, shared by the firmware, the console and the host tool:

```rust
pub enum Bank2 { Empty, Torn, Foreign, Format(u16), Stale { have: Version }, Corrupt, Open(Door) }
pub fn judge(read: impl Fn(u32) -> u32, want: Seal) -> Bank2
```

It reads in order:
1. the commit word, so that erased gives `Empty` and anything else not committed gives `Torn`;
2. magic and format;
3. `body_len` and `door_off` bounds;
4. seal equality, so a mismatch gives `Stale` and the UI can show bank 2's version;
5. the CRC over the body, about 1 ms for TAPE and about 5 ms for a full 512 KB.

`Door` can be built only by `judge` (a private field). Proxies, data accessors and `EngineType::Tape` being playable all take a `&Door` or something derived from it. That is the type-level rule that bank 1 never calls a stale or missing bank 2.

**The closed UX.**
- The TAPE row reads `UPDATE BANK 2` where alpha reads `COMING SOON`; it is the same `browser::Soon` path from ADR 0132.
- The console's `status` adds a line: `bank2 open 0.2.0 seal=…`, or `stale have 0.1.9` / `empty` / `torn` / `corrupt`.
- SETTINGS › SYSTEM shows the same.
- For TAPE Parts in a project, Q4 decides. The recommendation is a silent Part showing `UPDATE BANK 2`, with its Sound kept. ADR 0132 rejected a silent Part only because a guard is needed on every way onto its pages; with a door token those guards are the type's.

**ECC on read.** RM0433 says a double-bit ECC error on a flash read raises a bus error (*verify*). A torn commit word could then fault at boot, every boot.
- The HardFault and BusFault handlers (`panic.rs`) check FLASH_SR2.DBECCERR and FLASH_ECC_FA2R.
- If the failing address is in bank 2, the handler writes `BANK2_BAD` to a free RTC backup register (BKP14R) and resets through `Marker::reset`.
- The next boot sees `BANK2_BAD` and erases bank-2 sector 0 before any bank-2 read. Erase clears the ECC.
- It boots with bank 2 `Empty`, then reinstalls from the card if Q2 allows.

One fault and one extra reset, never a loop. Power loss clears the backup domain on this unit (`boot0-jumper-normal`), but a power-on re-faults and re-marks, so it converges in one cycle.

## 3. How bank 2 gets written

| Route | Verdict |
|---|---|
| **Chimera writes it itself from a card-staged file (recommended)** | It uses the existing robust `put` (CRC-checked and deadline-bounded, ADR 0092) to stage `/CHIMERA/SYSTEM/BANK2.BK2`. The unit judges the staged file with the same `judge` against its own seal, then programs it. A torn write leaves the staged file in place, so the next boot reinstalls it. The bootloader ignores subfolders and non-`.bin` files |
| Chimera streams it from USB straight to flash | Saves the card step, but a torn write needs the host again. Kept as a fallback if the card is absent or full |
| Copied in the bootloader's SD-access mode | Works as a staging route, since the unit picks the file up at boot. But MENU at power-on did not work on the owner's unit (2026-10-03, re-test pending), so it is not the main route |
| Stock bootloader SD flash or DFU | **No.** SD flash writes only at 0x08020000 and erases only bank 1. A combined file triggers the sector-0 hazard above. Its "DFU" is ROM DFU |
| ROM DFU (`dfu-util -s 0x08100000`) | **No.** It hung on 2026-10-07 and no fix is known. Never use alt 1 (option bytes) |

**The writer** (`chimera-board::bank2`, behind the `bank2` feature).
- **Registers.** It uses only `FLASH_KEYR2`, `CR2`, `SR2`, `CCR2` and the ECC address register. `KEYR1`, `CR1`, `OPTKEYR` and `OPTCR` are unreachable: no binding to them exists in the module, and a host test greps for it.
- **Addresses.** `Bank2Sector` is an enum of S0–S3 (0x08100000, 0x08120000, 0x08140000, 0x08160000); there is no S4–S7. `Bank2Word` is a 32-byte-aligned address that can only be built inside S0..=S3.
- **Order** (power-loss safe without A/B):
  1. Close the door. Release TAPE voices, set the MPU to no-access, and close the TAPE row.
  2. Judge the staged file fully, CRC and seal, before touching flash.
  3. Erase the sectors the image spans, **S0 first**: that removes the commit word before anything else changes.
  4. Program the body from offset 0x100 upward, 256-bit words, one card block (512 B, 16 words) at a time through the existing put buffer.
  5. Program the header words 0x00–0xDF.
  6. Read everything back through an invalidated D-cache and check the CRC and seal from flash.
  7. Program the **commit word last**, then run `judge` again.
  8. Open the door.
- **Error flags.** Every step checks the SR2 errors (WRPERR, PGSERR, STRBERR, INCERR, OPERR, RDSERR, SNECCERR, DBECCERR) and waits for QW2/BSY2 with a timeout of a few seconds per sector erase (*datasheet DS12117: t_ERASE for 128 KB, PSIZE x64*). A failure leaves bank 2 `Empty`/`Torn`, never `Open`.
- **No A/B.** Bank 1 boots alone, and the staged file is the second copy. A/B would halve the 512 KB for a benefit, TAPE staying playable during a failed update, that nobody needs.

**Read-while-write.** Bank 2 is erased and programmed while code, vectors and the audio ISR run from bank 1. RM0433 describes two independent banks with separate controllers and RWW across them (*verify the section that guarantees bank-1 fetches never stall during a bank-2 erase*). So:
- the audio interrupt and DMA keep running and the watchdog (ADR 0034) keeps its heartbeat, with nothing new to arrange;
- the writer runs in the UI loop like `put`, polling QW2;
- ALGO, MODAL and VA can keep sounding (Q3), and TAPE is silent because its door is closed.

The fallback, if the bench shows overruns, is a muted master during the write, not a stopped ISR.

**Wear.** Endurance is 10 k cycles a sector (*datasheet*). Bank-1 sectors 1–7 already take one cycle per `just flash`. Bank 2 takes one only when its seal changes, so it is never worse than bank 1.

## 4. Release and install flow for testers

The release keeps `chimera-<v>.bin` (bank 1, the existing DFU route) and adds `chimera-<v>.bk2` (bank 2). `SHA256SUMS` covers both, and the `just release` asset assertion grows from four to five (six with ORBIT's own `.bk2`).

The tester does this:
1. Plug in USB. On the unit, SETTINGS › SYSTEM › OS UPGRADE (or the tool's `to-dfu`), then the same `dfu-util … -s 0x8020000:leave` line as today.
2. `python3 chimera-usb.py bank2 chimera-<v>.bk2`. The tool waits for the console, puts the file to `/CHIMERA/SYSTEM/BANK2.BK2`, and the unit installs it (about 10 s on screen) and answers `OK bank2 open 0.2.0`.

**The order does not matter.** If step 2 runs first, the unit stages the file, answers `staged: needs firmware 0.2.0`, and the boot after step 1 installs it (Q2). Skipping step 2 never breaks anything: TAPE reads `UPDATE BANK 2`.

When the seal didn't change between releases, the release notes say "bank 2 unchanged" and step 2 answers `OK already installed`.

There is also a convenience: `chimera-usb.py update chimera-<v>.bin chimera-<v>.bk2` does both, as `just flash` does. Testers never write 0x08000000, never use alt 1, never move BOOT0, and never put a `.bk2` or a large `.bin` in the card root.

## 5. Developer flow and tests

**`just flash`:**
1. Build.
2. `objcopy -R .bank2` into `chimera.bin` and `-j .bank2` into `chimera.bk2`.
3. `chimera-utils bank2-seal` both files.
4. Run the door check.
5. `to-dfu`, `dfu-util` bank 1, then `wait-console`.
6. `chimera-usb.py bank2 target/chimera.bk2`, which asks `status` first and skips if the seal is already installed.

`release-bin` does steps 2–4 per variant and asserts bank-1 size < 917,504.

**Host tests** (`chimera-image`, `chimera-utils`, `chimera-board` with a simulated flash):
- **`judge`.** Each state from fixtures. Every single-byte and single-word corruption of a sealed image is never `Open`. Bounds (`body_len` over the slot, `door_off` outside the body or misaligned) give `Corrupt`.
- **Writer state machine against `SimFlash`.** It models 32-byte words, erase to 0xFF, program-once (a second program is an error) and a write that tears to random bits. It panics on any address outside S0–S3 or on a bank-1 or option register.
- **Power cut.** At every step and every word: restart, judge, reinstall from the staged file. The result is always `Open(new)` or not `Open`, and never `Open` with the wrong bytes.
- **Seal round trip.** The seal tool's output parses, and its seal equals the patched bank-1 static.
- **Door check.** A fixture ELF holding a forbidden relocation fails it.
- **Desktop.** The simulator links TAPE in-line (no banks). A test feature makes the door "closed" so the `UPDATE BANK 2` UX runs in QA.

**Hardware bring-up checklist** (owner or Claude over the USB console; nothing writes 0x08000000, option bytes or alt 1):
1. **0a, read-only.**
   - `bank2 diag` prints FLASH_OPTSR_CUR (SWAP_BANK = 0, RDP = AA, BOOT_ADD0/1), WPSN_CUR1R/2R, SR1/SR2 and the ECC address registers.
   - Bank-2 S0–S3 read as erased and `judge` says `Empty`.
   - Pass: SWAP_BANK = 0, no WRP on bank-2 S0–S3, SR2 clean.
2. **0b, owner go.**
   - `bank2 spike` erases S0 only and programs a 4 KB pattern plus a 16-byte Thumb function (`adds r0, r0, r1; bx lr`), with a commit word.
   - It reads back and checks, calls the function through the MPU-opened region, times a 4 KB read from bank 2 against bank 1 (equal within 5%), then erases S0 again.
   - It runs during ALGO playback and reports `overruns` and watchdog resets.
   - Pass: erase under 4 s, no SR2 errors, `add(2,3) == 5`, no overrun, no reset.
3. **Phase 2:** install and reinstall a test `.bk2`, then a **power-pull** test by the owner. Pull power mid-erase, mid-body and mid-header, three times each; each time the unit must boot, show `UPDATE BANK 2` or reinstall, and end `open`.
4. **Phase 3:** the `bench-tape` TAPE COLD rows from bank 2 within ±2% of the same build with TAPE in bank 1, and a stale `.bk2` (the previous build's) showing `UPDATE BANK 2` with no fault.

## 6. Risks

| Risk | Mitigation |
|---|---|
| **Bricking** | The writer can't address bank 1, sector 0 or the option registers (types plus a host test). ROM DFU through BOOT0 always recovers, because RDP, BOOT_ADD and SWAP_BANK are never written. Bank 1 boots without bank 2. A torn bank 2 costs at most one extra reset (§ 2, ECC) |
| **Stock bootloader and bank 2** | 1.09 and every earlier public version leave bank 2 alone, apart from the jumps to the slots on buttons 9 and 10 |
| **The sector-0 hazard in bootloader SD flash** | No combined .bin, ever. The bank-1 size is asserted below 917,504. The bank-2 file is `.bk2` and staged in `/CHIMERA/SYSTEM` |
| **1.09's slots** | The linker assert and the MPU no-access region keep Chimera out of 0x08180000+. Holding button 9 or 10 at power-on jumps into erased flash, as it does today (a power cycle recovers) |
| **ECC errors and retention** | The commit word comes last, the CRC is checked every boot, a DBECC fault marks bank 2 bad and erases it, and SNECCERR (corrected) counts go in `status` |
| **BOOT0 jumper** | Untouched. It stays in NORMAL (`boot0-jumper-normal`) |
| **Option bytes** | Read-only in 0a, never written. Never DFU alt 1 |
| **Flash wear** | Bank 2 is rewritten only on a seal change, never worse than bank 1 |
| **Testers on another bootloader** | Bank-2 writes don't involve the bootloader. Before 1.09 there were no slots, which is harmless. Testers stay on 1.09 (`bootloader-hands-off`) |
| **A seal mismatch in a shipped pair** | `just release` refuses assets whose seals differ, and the door check runs in `just check` |
| **The ROM-DFU hang is a chip-level problem** | Phase 0a/0b finds out before any design commitment, and the fallback is the flash diet |

## 7. Migration

| Order | Move | Bank 1 freed (ORBIT build) | Note |
|---|---|---|---|
| 1 | **TAPE DSP** (`chimera-tape` crate, door, proxy) | TAPE re-enabled costs ~2–4 KB instead of **46,476 B**. Measured delta: ORBIT 846,936 vs ORBIT+TAPE 893,412 | Undoes ADR 0132 points 1–4 behind `Door` (superseding ADR, #476) |
| 2 | DRUM, built for bank 2 from day one | all of it (not yet written) | Its own door table |
| 3 | Cold data, as it arrives | today ≤ 3 KB (`factory.bin` and splash). Fonts stay | The door gains `fn table(id) -> &'static [u8]` |
| later | Non-default reverb TYPEs (ROOM, HALL, GATE/REVERSE, BLOOM ≈ 65 KB, flash audit) | up to ~65 KB | Only if bank 1 runs short again; a closed door falls back to a bank-1 TYPE |

Bank 2's 512 KB holds TAPE about eleven times over. The free bank-1 figures before any move are shipping 89,708 and ORBIT 70,568.

## 8. Phased plan

- **Phase 0a: read-only diagnostics.** A `bank2 diag` console command, flashed in a normal build. It needs no bank-2 write. Pass: § 5 item 1. If WRP is set on bank 2, stop and bring it to the owner; we never clear it ourselves.
- **Phase 0b: execute-and-read spike (owner go, Q5).** A strictly read-only proof that code runs from bank 2 is impossible, because something has to write it first. 0b is the smallest write: one sector, by Chimera's own code, erased again afterwards. Pass: § 5 item 2. This decides between the bank-2 plan and diet-only.
- **Phase 1: header and version check.**
  - `chimera-image` (`judge`, header), the seal tool, the door check, `memory.x` BANK2, `bank2.x`, the MPU regions and the `UPDATE BANK 2` UX.
  - Bank 2 is still empty, so every build shows `Empty` and nothing changes for users.
  - ADR "Bank 2 holds sealed code behind one door".
  - Pass: host tests, `just check`, and the unit showing `empty`.
- **Phase 2: self-programming.**
  - The writer, `put` to `/CHIMERA/SYSTEM/BANK2.BK2`, the boot install, the ECC fault path, `chimera-usb.py bank2` and `update`, and the `just flash` and `release` changes.
  - A test `.bk2` that holds only a data table.
  - Pass: § 5 item 3, the power-pull test included.
- **Phase 3: TAPE moves.**
  - `chimera-tape` crate, door and proxy, `tape` on in the firmware, the ADR 0132 supersession (#476).
  - Pass: § 5 item 4, the door check clean, and bank-1 cost ≤ 4 KB.

## RM0433 / datasheet claims to check

- the dual-bank layout, eight 128 KB sectors per bank, and bank 2 at 0x08100000 with SWAP_BANK = 0;
- RWW across banks, with no stall of bank-1 fetches during a bank-2 erase;
- 256-bit flash words: a word can be programmed only once after an erase, partial words need FW, and PSIZE goes with VOS;
- erased words read 0xFF without an ECC error;
- whether a double ECC error raises a precise bus error, and what FLASH_ECC_FA2R latches;
- the WPSN_CUR2R and OPTSR_CUR fields;
- FLASH_CR1 bit 11 being reserved (the bootloader hazard);
- sector erase time and the 10 k-cycle endurance (DS12117);
- AN2606: the H74x/75x ROM bootloader's known limitations around bank-2 erase.

## Phase 0a result (2026-10-11, unit on 83a6ea1f)

Read-only, over the console's `flash` command (raw log in (private notes)):
- **No write protection on either bank** (WPSN_CUR1R and WPSN_CUR2R are both 0xFF, so nothing is protected). There is no PCROP or secure area (PRAR and SCAR are empty), RDP is 0, SWAP_BANK is 0 and BOOT_CUR is 0x08000000.
- **Bank 2 is erased** at 0x08100000 and at 0x08180000. SR1 and SR2 show no error flags before or after reading it, so reading bank 2 raised no ECC faults.
- **Both CRs are locked, with different parallelism:** CR1 has PSIZE 2 (×32), CR2 has PSIZE 3 (×64). Bank 2's last writer, probably the 2026-10-07 ROM DFU attempt, left ×64. A ×64 erase needs VOS0/VOS1 at the right supply. That is a lead to check against RM0433 in Phase 0b: the writer sets PSIZE explicitly and dumps SR2 on timeout.
- **So candidate (a), write protection, is ruled out** for the 2026-10-07 hang. What remains: ROM DFU's assumptions after a software jump at 480 MHz (clocks, voltage scaling, PSIZE), or a hardware or erase-timing fault. Phase 0b tests Chimera's own erase with explicit settings.
