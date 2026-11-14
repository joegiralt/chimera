# Bank 2 into the bootloader slots: Design

- **Status:** Revised after review (B1–B6 and should-fix 1–8 applied, 2026-11-14). Design only: no firmware change yet
- **Date:** 2026-11-14
- **Owner decision (2026-11-14):** open bank-2 sectors 4–7 (0x08180000–0x081FFFFF, 4 × 128 KB = 512 KB; the earlier "256 KB" was wrong) to Chimera's code, so 0.4 keeps all its features.
- **ADR:** [0272](../../adr/0272-bank-2-extends-into-the-bootloader-slots.md) (Proposed), superseding in part [0137](../../adr/0137-bank-2-holds-sealed-code-behind-one-door.md)
- **Builds on:** ADR 0137 (one link, one door, one seal), ADR 0251 (bank 1 below 8 KB of bank 2), spec `2026-10-11-bank2-design.md` (its decision 6, "the slots stay untouched for good", is what this reverses), research (private notes) (Ixox/preenfm3 at 65cb963)

## Decisions taken (controller, 2026-11-14)

- **The refusal wording stays** as in § 3.4.
- **No DFU pre-check in 0.4.** `chimera-usb.py update` doesn't read the slots before flashing bank 1; the guard on the unit is the only check.
- **Both stubs go in every image** (§ 2.3). Every image is now exactly 1 MB (§ 1.1).
- **DRUM stays in bank 1.** Only new 0.4 work goes into S4–S7.

## Where we are

| Area | Range | Free today |
|---|---|---|
| Bank 1 firmware | 0x08020000–0x080FFFFF (917,504 B; SD-flash limit) | ~43 KB |
| Bank 2 S0–S3 (`.bank2`) | 0x08100000–0x0817FFFF (512 KB) | ~8 KB, at ADR 0251's floor |
| Bank 2 S4–S7 (the slots) | 0x08180000–0x081FFFFF (512 KB) | erased, unused, MPU no-access |

The facts the design rests on (research report, upstream source; § 7 lists what is not verified on the unit):
- **Buttons at power-on.** Bootloader 1.09 reads the buttons once. Held MIXER (index 9) goes to `bootJumpToApplication(0x08180000)` and held EDIT (10) to 0x081C0000. MENU (6) stays in the bootloader, and anything else goes to 0x08020000 (`bootloader/Src/main.c:116-129`). All three jumps call `MX_Deinit()` first and use the same `bootJumpToApplication`.
- **The jump.** It loads MSP from word 0 and branches to word 1, with no validity check and without setting VTOR (`bootloader.cpp:304-310`). The normal path doesn't set VTOR either: the firmware sets its own (Chimera through cortex-m-rt's `set-vtor`).
- **An erased slot hangs.** It faults into the bootloader's `while(1)` HardFault handler until power is cycled. This is inferred from the source and has not been tried on the unit.
- **Nothing else writes bank 2.** The bootloader never erases or writes it, and stock firmware has no flash write code.
- **No firmware, fork or documented user path has ever used the slots.**

## Summary

- **One image of exactly 1 MB:** one link, one output section `.bank2`, one header, door, seal and `.bk2`. The body runs from 0x08100000 to 0x081FFFFF. Fixed points sit at 0x08180000 (MIXER's slot reserve), 0x081BFFE0 (S5's marker), 0x081C0000 (EDIT's reserve) and 0x081FFFE0 (S7's marker). The section is filled with 0xFF.
- **Slot stubs.** Each 1 KB reserve holds a 64-byte stub, two flash words. The **code word** (0x00) is a vector table whose reset handler sets VTOR to 0x08020000 and jumps into bank 1, stock or Chimera. The **tag word** (0x20) holds the stub's literals and the tag `CHIMERA-SLOT`.
- **Ownership marks.** A slot is ours if S4's tag word or S5's end marker reads cleanly, and the same for S6 and S7. The install writes them in an order that keeps at least one of them readable after a power cut at any instant.
- **`slot_guard()`** runs on its own, after `BANK2_INSTALLING` is marked and before anything is drawn or erased. Its reads are ECC-safe. A slot that isn't ours must be fully erased; otherwise the guard refuses, nothing is written, and a screen says so.
- **The update stays one command:** `chimera-usb.py` takes a 1 MB `.bk2`, and from 0.3 it flashes first and stages after.
- **MPU:** region 2 grows to 1 MB; region 1 is unchanged. **Header format 2.**

## 1. Layout

### 1.1 The address map

```
0x08100000  S0  header (256 B) | door 0x08100100 | fixed tables after it | low groups ...
0x08120000  S1  low groups ...
0x08140000  S2  low groups ...
0x08160000  S3  low groups ... __bank2_lo_end (<= 0x08180000) | 0xFF
0x08180000  S4  MIXER reserve 1 KB: code word, tag word, 0xFF x 960 | S4 groups ...
0x081A0000  S5  S4 groups ... __bank2_s4_end (<= 0x081BFFE0) | 0xFF | S5 marker @ 0x081BFFE0
0x081C0000  S6  EDIT reserve 1 KB | S6 groups ...
0x081E0000  S7  S6 groups ... __bank2_s6_end (<= 0x081FFFE0) | 0xFF | S7 marker @ 0x081FFFE0
```

The room for code and data is 512 KB + 2 × (256 KB − 1 KB − 32 B), which is **1,046,400 B**. That is ~510 KB more than today, and ~518 KB free in all. Every image is exactly 1 MB: `body_len` is 1 MB − 256, the `.bk2` is 1,048,576 B, and every install erases S0–S7. The S7 marker, on the bank's last word, is what fixes the length. The cost is all eight erases on every seal change, and a 1 MB CRC at each boot (§ 6.3 measures both). In return the image never varies in size or in the sectors an install touches, and S7 never holds stale bytes.

### 1.2 One output section with fixed points (review B3, B4, should-fix 1)

There is still **one image**, for ADR 0137's reasons: same compiler, same link, no ABI, and the seal as an exact compatibility key. It is also **one output section**. The first draft used three sections. That failed twice. First, `door_hits` exempts only the section named `.bank2` (`chimera-utils/src/bank2.rs:145-151, 181`), so a door entry into `.bank2_s4` would have failed the door check. Second, lld fills the gaps between output sections with zeros or trap fill, not 0xFF, so the flash's erased gaps would not have matched the file, and the read-back CRC would have failed. With one section, fixed points are just assignments to the location counter inside it:

- `memory.x`: `BANK2 (rx) : ORIGIN = 0x08100000, LENGTH = 1M`.
- `bank2.x`:
  ```
  .bank2 : ALIGN(32) {
    __bank2_base = .;
    KEEP(*(.bank2.header));  . = ALIGN(32); __bank2_door = .;  KEEP(*(.bank2.door));
    /* SLOTS ... */  /* low groups (today's list) */
    . = ALIGN(32); __bank2_lo_end = .;
    . = __bank2_base + 0x80000;  __slot_mixer = .;  KEEP(*(.bank2.slot.mixer));
    . = __bank2_base + 0x80400;
    /* S4 groups */
    . = ALIGN(32); __bank2_s4_end = .;
    . = __bank2_base + 0xBFFE0;  KEEP(*(.bank2.slot.mixer_end));
    . = __bank2_base + 0xC0000;  __slot_edit = .;  KEEP(*(.bank2.slot.edit));
    . = __bank2_base + 0xC0400;
    /* S6 groups */
    . = ALIGN(32); __bank2_s6_end = .;
    . = __bank2_base + 0xFFFE0;  KEEP(*(.bank2.slot.edit_end));
    __bank2_end = .;
  } > BANK2 =0xFF
  ```
  The asserts:
  - `__bank2_lo_end <= 0x08180000`, `__bank2_s4_end <= 0x081BFFE0` and `__bank2_s6_end <= 0x081FFFE0`, each with a message naming its group. lld also refuses to move the location counter backwards.
  - `__slot_mixer == 0x08180000`, `__slot_edit == 0x081C0000` and `__bank2_end == 0x08200000`.
  - The existing door asserts are unchanged.
- **Everything else is unchanged:** `-R .bank2` and `-j .bank2` in `bank2-split`, the stack check's `objdump -j .bank2` (`Justfile:476`), `door_hits`, the seal tool, and `check-door-placed`'s section. Only its address bound moves, from 0x08180000 to 0x08200000.
- **Which code goes high:** whole glob groups, placed by hand, the way `bank2.x` already places subsystems. The S4 and S6 groups are new 0.4 work only (decided); DRUM stays in bank 1. Fixed door tables stay in S0.
- **The door doesn't change.** It is still one table at 0x08100100. Entries may point anywhere in `.bank2`, and `door_hits` already bounds bank 2 by `BANK2_BASE + BANK2_LEN`.

*Verify on the first spike:* that lld's `=0xFF` fill covers gaps made by `. =` inside the section. `check-slots` (§ 6.1) asserts it from the bytes.

**Rejected:**
- *A second image or door for S4–S7:* this is ADR 0137's rejected stable-ABI option.
- *Stubs written once and kept outside the image:* that leaves 256 KB of code room, half the gain.
- *`--enable-non-contiguous-regions`:* not verified for rust-lld, and not needed with hand-placed groups.

## 2. Slot stubs and markers

### 2.1 The stub (64 B, two flash words)

Bootloader 1.09 does the same thing on every jump: `MX_Deinit()`, MSP ← word 0, branch to word 1. The stub turns a slot jump into the normal-path jump, plus VTOR:

```
code word (0x00-0x1F):
  0x00  .word 0x2002_0000      initial SP: DTCM's top (the stub uses no stack)
  0x04  .word base + 0x09      reset, Thumb: the code at base + 8
  0x08  ldr  r0, [pc, ...]     0x08020000, bank 1's vector table   (literal at 0x20)
        ldr  r1, [pc, ...]     0xE000ED08, SCB->VTOR              (literal at 0x24)
        str  r0, [r1]
        dsb
        isb
        ldr  r1, [r0]          bank 1's SP
        ldr  r2, [r0, #4]      bank 1's reset
        msr  msp, r1
        bx   r2                (24 B of code: 0x08-0x1F)
tag word (0x20-0x3F):
  0x20  .word 0x08020000, 0xE000ED08
  0x28  0xFF x 8
  0x30  "CHIMERA-SLOT" (12 B), version u8 = 1, slot u8 (0 MIXER, 1 EDIT), 0xFF 0xFF
```

- **No validity check of bank 1.** The stub does exactly what the bootloader's normal path does, so a held button is never worse than a plain power-on.
- **It works for stock and Chimera alike.** It reads bank 1's vectors at run time and carries no build's addresses.
- **Same state as a normal boot,** because `MX_Deinit` is the same call. VTOR is set before the jump, so a pending interrupt taken after it goes through bank 1's table.
- **A 1 KB reserve** holds a full H7 vector table (664 B) at VTOR's 1 KB alignment, should a later stub need one. Check-slots asserts that the 960 B after the stub are 0xFF.

### 2.2 The end markers

S5's last flash word (0x081BFFE0) and S7's (0x081FFFE0) hold `CHIMERA-SLOT-END` (16 B), the version, the slot, and 0xFF to 32 B. They are part of the image (`.bank2.slot.mixer_end`/`.edit_end`), so the CRC covers them.

### 2.3 One source of bytes

`chimera_image::slot::{code_word, tag_word, end_marker}(slot) -> [u8; 32]` are hand-encoded `const fn`s in the host-tested crate. The firmware places them as `#[used] #[link_section = ".bank2.slot.mixer"] static SLOT_MIXER: [u8; 64]`, and the same way for the rest. They are data, so the outliner, LTO and the door check never see them. The installer, `check-slots` and `bank2-seal --check` compare the bytes against the same functions. The guard reads the tag (`CHIMERA-SLOT`) and the marker (`CHIMERA-SLOT-END`) by their text, not their version, so a later stub version is still ours.

Every image from 0.4 on carries both stubs and both markers (decided).

## 3. The slot guard and the install

### 3.1 Install order (review B1)

For each slot, MIXER (S4 + S5) then EDIT (S6 + S7):

1. **Erase S5**, unless the guard's scan found it fully erased. Erasing an erased sector buys nothing, and a torn erase there could leave ECC garbage where there was none.
2. **Program S5's end marker.**
3. **Erase S4.**
4. **Program the tag word** (0x20).
5. **Program the code word** (0x00).

These are separate writes, tag word first; the first draft's "two words at once" is gone.

**The invariant: one clean piece of evidence survives at every instant.** The order above keeps S4's tag while S5 is rewritten, then S5's marker while S4 is. It is the order for a slot the guard took by its tag or by its scan (§ 3.2). A slot taken by its marker alone has no clean tag to keep, so erasing S5 first would leave nothing: a cut in reinstall 1 between S4's erase and the tag, then a cut in reinstall 2 during S5's erase or marker, would read foreign for good. For that slot, and for one taken from the boot's slot mark, the order is: **erase S4, program the tag word, program the code word, erase S5, program S5's marker.** The marker stays until the tag is clean. The whole install is then:
- `slot_guard()` (§ 3.2);
- erase S0 (the commit word goes first, as now), then S1–S3;
- the MIXER sequence, then the EDIT sequence;
- the body from 0x100 up, skipping the six words already programmed (the bytes match the file, and the read-back CRC checks them);
- the header, the uncommitted judge, the commit word last, and `Door::open`.

A cut at any instant leaves the slot ours by the guard's rule:

| Cut during / after | S4 tag word | S5 marker | Guard reads | Held MIXER at power-on |
|---|---|---|---|---|
| S5 erase (torn) | old, clean | torn / gone | ours (tag) | old code word → boots |
| S5 marker write (torn), old S4 | old, clean | torn | ours (tag) | boots |
| S5 marker write (torn), first install (S4 erased) | erased | torn | ours (§ 3.2 rule c) | hangs, as today |
| S4 erase (torn or done) | torn / erased | clean | ours (marker) | **hangs**, up to the erase time |
| tag word write (torn) | torn | clean | ours (marker) | hangs |
| code word write (torn) | clean | clean | ours | hangs, or garbage: a power cycle recovers |
| after the code word | clean | clean | ours | stub → boots |

The table is for the S5-first order. In the S4-first order the marker covers S4's erase and both S4 writes, and the tag covers S5's erase and the marker write.

The held-button window lasts from S4's erase until its code word is programmed. Each erase is typically ~1 s and at most 4 s (DS12117 Table 51, quoted at `chimera-image/src/regs.rs:78-80`; the timeout is 6 s). So the window is **~1 s typical and up to ~4 s per slot**, twice per install. Only a power cut inside it followed by a held button at the next power-on hits it, and a plain power-on reinstalls.

### 3.2 `slot_guard()` (review B1, B2, should-fix 6)

`chimera_image::slot::guard(&mut impl SlotRead) -> Result<SlotPlan, SlotForeign>` is pure, and `InstallFail::SlotForeign` comes from it, not from `install_with`. `SlotPlan` records, per slot, the evidence the guard accepted: `Tag`, `Marker`, `Scan { erased }` (rule c; `erased` when both sectors read 0xFF, step 1's skip), or `Mark` (taken from the boot's slot mark, unread). The install picks the slot's order from it (§ 3.1). For each slot it reads, in this order:

- **(a) S4's tag word**, then **(b) S5's marker**. If the tag word reads cleanly (no ECC error, exact text), the slot is ours and nothing else is read. A clean marker makes the slot ours only if S4's code word also passes: erased, ECC-flagged, or `(w & e) == e` against `code_word`. A marker alone would survive a revert that flashed other code into S4 only. This relies on the stub's code bytes staying the same across versions.
- **(c) Otherwise** it scans both sectors. The slot is ours if every word is erased, except possibly S4's code word, S4's tag word and S5's marker word. Only Chimera writes those three positions, in this order, so this covers a torn first install. Any other word not 0xFF makes the slot foreign. *Where this differs from the review's rule ("otherwise it must be fully erased"):* that rule locks Chimera out after a cut during the marker write on a first install, because S4 is erased and the marker is torn. Rule (c) closes that case. A foreign application in slot 2 would need its vector table at 0x08180000, which is S4's code word, but rule (c) only accepts that word when everything else is erased. One word of foreign code with nothing behind it is not an application. The bitwise test does pass a code word that is a bit-superset of ours, but its reset target then carries 0x0818_0009's (or 0x081C_0009's) bits. That puts it in S4–S7, which must hold ours, or in memory that is unmapped or not set up at reset, so no working trampoline into system memory, RAM or bank 1 can pass.

**ECC-safe reads.** Every guard read can meet a torn word with a double ECC error. Each read runs with FAULTMASK set and SCB->CCR.BFHFNMIGN = 1. Under those, a precise data bus fault is ignored and the load returns. After each read the guard checks FLASH_SR2.DBECCERR (and FA2R's word) and clears it. A flagged word counts as "not clean" in (a) and (b), and as "not erased" in (c), except at the three positions (c) allows.
- *Verify on a spike gate* (§ 6.3, gate 1b) that a flash double-ECC error is a **precise** bus error that BFHFNMIGN masks. RM0433 doesn't say so plainly.
- *Fallback if it isn't:* a reset mark, `BANK2_SLOT_TORN | slot` in RTC_BKP14R, set before each slot's reads. The HardFault handler keeps it, and the next boot's guard treats that slot as ours-torn, which the install then erases. The cost: a foreign slot with an ECC error would be erased instead of refused. That is accepted only in the fallback, because no foreign slot is known.

**Where it runs.** The boot path is: `find_staged` → `bank2_mark(BANK2_INSTALLING)` → **`slot_guard()`** → on `Ok`, `installing::draw` and `install_with`; on `Err`, `bank2_mark(0)` and the refusal screen. The console's `run_install` gains the same mark around guard and install; today it sets none. A fault anywhere after the mark, whether an unmasked guard read or the install, keeps `BANK2_INSTALLING` (`fault_mark`). The next boot is `EraseS0NoInstall`, so it can't loop through `BANK2_BAD` → erase S0 → install → fault, which the first draft allowed.

### 3.3 What a refusal leaves

Bank 2 is untouched, S0–S3 included (a partial image is no use), and so is the staged file. The boot continues with bank 2 closed, since the old image is never `Good` for a new build's seal. Each later power-on repeats the refusal: the staged file's full CRC (`find_staged`, ~1 MB from the card) plus the guard's scan, a cost § 6.3 measures. It never decides to write.

### 3.4 What the user sees (wording kept)

The boot shows this for 5 s or until any button, instead of DON'T POWER OFF:

```
NOT INSTALLED
THE MIXER SLOT HOLDS
OTHER CODE.
NOTHING WAS WRITTEN.
```

- `status` reads `bank2 slot foreign mixer|edit`, ABOUT reads `SLOT IN USE`, and `Bank2Status` gains `SlotForeign(Slot)`.
- `chimera-usb.py update` and `bank2` exit 2 with `the unit's MIXER slot (0x08180000) holds other code; bank 2 not installed`.

### 3.5 Recovery (should-fix 4)

- **A foreign or refused slot:** `chimera-usb.py update` with the **0.3 package**. 0.3 never touches S4–S7, and its bank 2 fits S0–S3, so the unit plays again.
- **Clearing a foreign slot:** owner only, with the gate build's `bank2 erase slots` (never in a shipping build; § 6.3). Testers are never given an erase command.
- **Never ROM DFU into 0x0818xxxx.** It hung erasing bank 2 on 2026-10-07, and the sanctioned DFU recovery stays at 0x08020000 only.
- **Back from stock:** stock has no console, so the tool can't stage under it. The way back is the tester route: the bootloader's SD flash (or DFU at 0x08020000) of `CHIMERA.BIN`, with `CHIMERA.BK2` in the card root. The first boot installs it, and the guard reads the slots as ours because the stubs are still there. *Not verified:* whether `update` works from a unit already in DFU.

## 4. The MPU

| Region | ADR 0137 | Now |
|---|---|---|
| 0 | DMA, D2's first 16 KB | unchanged |
| 1 | 0x08100000, 1 MB, no access, XN, strongly-ordered, always on | **unchanged**: the floor while region 2 is `Closed` |
| 2 | 0x08100000, **512 KB**: Closed / Read (RO, XN, Normal NC) / Write (RW, XN, SO) / Open (RO, exec, Normal WT) | 0x08100000, **1 MB**, the same states and attributes |

- **The RASR words** become `BANK2_READ_RASR` 0x1608_0027, `BANK2_WRITE_RASR` 0x1300_0027 and `BANK2_OPEN_RASR` 0x0602_0027 (`SIZE_1M`), with their `const` asserts in `chimera-board/src/cache.rs`.
- **Cache maintenance is unchanged,** and no operation names a bank-2 address.
- **The guard reads in `Write`,** which is strongly-ordered and uncached, so each read reaches the flash and its ECC flag belongs to that read.
- **No subregions.** The door check already proves bank 1 can't reach a stale byte.
- **ECC:** `bank2_ecc_fault`'s bound becomes the whole bank. A double-ECC fault outside the guard marks `BANK2_BAD` as today, unless `BANK2_INSTALLING` is set (§ 3.2). The judge reads only S0's header until the commit word is good, so a torn install's stub words are read only by the guard and by the bootloader on a held button.
- The console's `flash` still peeks only at S0.

## 5. Reversibility

| State of the unit | Normal power-on | Held MIXER | Held EDIT |
|---|---|---|---|
| Stock, or Chimera ≤ 0.3 (slots erased) | that firmware | **hangs** until power is cycled (inferred) | **hangs** |
| Chimera 0.4+, installed | Chimera | stub → Chimera | stub → Chimera |
| 0.4 install cut by power | Chimera, which reinstalls | boots, except in its slot's window (§ 3.1) | same for S6 |
| Reverted to stock after 0.4 | stock | stub → stock | stub → stock |
| Reverted to Chimera ≤ 0.3 after 0.4 | 0.3 | stub → 0.3 | stub → 0.3 |
| A foreign slot, guard refused | new Chimera, bank 2 closed | whatever the foreign code does | same |

- **To stock:** flash stock's `.bin` at 0x08020000 as today. The stubs stay, so held buttons boot stock instead of hanging. Nothing needs cleaning.
- **To Chimera ≤ 0.3:** 0.3 reports the 0.4 header as `format 2` and installs its own image into S0–S3 only. S4–S7 keep 0.4's stubs, markers and stale code, which 0.3's region 1 keeps no-access.
- **A later release that needs no S4–S7 code** still links both stubs and markers, so nothing changes. Handing the slots back would take a new ADR.
- **Upstream:** a bootloader that validates SP and reset passes the stubs. One that writes the slots would be refused by the guard.

## 6. Budget, checks and gates

### 6.1 `just check` and the build

- **`just check-bank2 <set>`** (new, run after `check-bank1` for `default`, `orbit` and `bank2-gate`). It reads the three end symbols and prints:
  ```
  bank 2 S0-S3   free = 0x08180000 - __bank2_lo_end
  bank 2 S4-S5   free = 0x081BFFE0 - __bank2_s4_end   (1 KB slot, 32 B marker)
  bank 2 S6-S7   free = 0x081FFFE0 - __bank2_s6_end   (1 KB slot, 32 B marker)
  bank 2 total
  ```
  It fails under ADR 0251's 8 KB floor, which now applies to the total, and holds each set to `tools/bank2-budget.txt` under bank 1's rules.
- **`just check-slots <elf> <bk2>`** (new, run by `bank2-split`) checks that:
  - the `.bk2` is exactly 1,048,576 B;
  - the bytes at file offsets 0x80000, 0x80020, 0xBFFE0, 0xC0000, 0xC0020 and 0xFFFE0 equal `chimera_image::slot`'s;
  - the 960 B after each stub and every gap byte in its region are 0xFF (the `=0xFF` fill);
  - `rust-objdump -d` of each code word decodes to § 2.1's sequence;
  - no symbol lies in a reserve or a marker word.
- **`check-door-placed`:** the bound `a >= 0x08180000` becomes `>= 0x08200000`, and it also fails on a symbol in a reserve or marker.
- **`chimera-image`:**
  - `BANK2_LEN` 1 MB and `FORMAT` 2;
  - `Bank2Sector` S0–S7, with `assert!(8 * SECTOR_LEN == BANK2_LEN)`;
  - `Bank2Word` bounded to 1 MB;
  - a new `slot` module: the words, `Slot`, `guard`, `SlotPlan` and `SlotForeign`;
  - `bank2_registers_test.rs` changes from "no literal ≥ S4" to "no literal ≥ 0x08200000, and no bank-1 or option register".
- **`chimera-usb.py` (review B5):**
  - `BK2_MAX` becomes `1 << 20`. Today's `512 * 1024` (`:107`, checked at `:858`) rejects every 0.4 package.
  - 0.2.0's and 0.3.0's exact `put` usage lines join `OLD_PUT_REFUSALS`. 0.3 refuses a BANK2 `put` over 512 KB with its usage line (`put.rs:270`), and that line isn't in the list today. A refusal sends `update` down its existing flash-first path: flash the 0.4 `.bin`, wait for the console, `put` and `bank2 install`. That is still one command and one install screen, the console's.
  - `BANK2_WAIT_S` (60 s) and `UPDATE_BOOT_WAIT_S` (90 s) rise to 120 s, to cover an install's worst case (§ 6.3).
- **`check-bin-size` is unchanged.**

### 6.2 Host tests

- **Stub and marker bytes** (`slot_test.rs`): word 0 is SP 8-aligned in DTCM, word 1 is `base + 9`, the code decodes by a table of expected Thumb-2 encodings to § 2.1, the literals are 0x08020000 and 0xE000ED08, and the tag and marker sit at their offsets.
- **Layout** (`install_test.rs`):
  - `plan()` always orders S0–S3, then S5 (skipped when erased), S4, then S7 (skipped when erased), S6;
  - `Bank2Sector::addr` holds for S0–S7, and `Bank2Word` takes 0x081FFFE0 and refuses 0x08200000;
  - `judge` gives `Format(1)` for an old header and `Corrupt` for a `body_len` ≠ 1 MB − 256 (format 2 fixes the length);
  - the body copy never programs one of the six pre-programmed words, since `SimFlash` refuses a second program.
- **Guard against `SimFlash`:**
  - Erased, tagged and marked slots pass.
  - One foreign word anywhere in S4–S7 outside the three positions, with no clean tag or marker, gives `SlotForeign`, and **SimFlash records no erase and no program**.
  - Words flagged as ECC-faulting at each position read as not clean and never panic the guard.
- **Tears (review B1):** `install_test.rs` already models torn erases, torn programs and ECC words. A tear is injected at every erase and every word write of steps 1–5, for both slots, on a first install (erased slots) and on a reinstall (ours). After each tear, the test asserts that `guard` reads **ours** for both slots, never foreign, and that a reinstall ends `Open`.
- **The mark sequence (review B2):** a host model of `boot_action`, `fault_mark` and the boot's order (`find_staged` → mark → guard → install) runs over a series of boots, with faults injected at each guard read and each install step. The test asserts that the marks never cycle `BANK2_BAD` → install → fault, and that a clean install follows within a bounded number of boots once the fault stops.
- **`test_chimera_usb.py`:**
  - A 1 MB package passes `check_pair`.
  - 0.2.0's and 0.3.0's usage lines each take the flash-first path, and the result is one install.
  - `bank2 slot foreign mixer` gives exit 2 with the message.
- **Desktop:** the refusal screen and ABOUT's `SLOT IN USE` get screen goldens.

### 6.3 Hardware gates (owner's unit, only after the owner OKs flashing)

These are run over the USB console with `chimera-usb.py update`, never inside a timeout. The held-button power-ons need the owner's hands. A known-good 0.3 `.bin` + `.bk2` stays ready for the DFU recovery at 0x08020000.

0. **Read-only, before any write:**
   - a `dfu-util -U` upload of 0x08000000 (128 KB), to confirm the 1.09 binary holds 0x08180000 and 0x081C0000 and compares with 9 and 10;
   - 0x08180000–0x081FFFFF read as all 0xFF;
   - bank 2's write-protect bits read through the console's `flash` (`WPSN_CUR2R`: expected 0xFF, nothing protected, as Phase 0a found on 2026-10-11).
1. **Flash** the 0.4 package from 0.3, which exercises B5's flash-first path. `status` must read `bank2 open … seal=<the package's>`. Log the install time (expected ~10 s, worst ~32 s of erase plus programming), each slot's held-button window (S4's and S6's erase times), and the boot time with flash `Good` (the 1 MB CRC).
   - **1b. ECC spike** (gate build): create a torn word with gate 6's word pull, or the gate build's deliberate double program if the controller allows it. Read it through the guard's masked read. It must return with DBECCERR set and no fault. If it faults, adopt § 3.2's fallback mark.
2. **Held MIXER at power-on** (owner): Chimera boots to its normal screen, `status` reads open, and audio plays. A stray MIXER press at boot is noted (§ 7).
3. **Held EDIT at power-on** (owner): the same.
4. **Cold boot** (power pulled, no button): Chimera, open.
5. **Update twice:** the same package (`OK already installed`, nothing erased), then a different seal (a full reinstall). Repeat gates 2–3 after each.
6. **Power pulls that tear (review B6),** in the `bank2-gate` build:
   - **Torn S4 erase:** the gate build starts S4's erase and shows PULL NOW. It keeps re-erasing S4 back to back for 3 s, so a pull lands inside an erase almost surely. Pull three times.
   - **Between the tag word and the code word:** hold with PULL NOW after the tag word. Pull three times.
   - **The same two for S6.**
   - **The old marks** (after S0's erase, half the body, before the commit word).

   After each pull a normal power-on must reinstall and end open, and the guard must read ours. Once per point, the owner also does a held MIXER (or EDIT) power-on before the normal one; it may hang, and a power cycle recovers.

   A word program (≤ 260 µs) can't be torn by hand; the host tear tests cover it.
7. **Revert to stock:** stock v1.03's `.bin` by DFU at 0x08020000. A normal power-on, a held MIXER and a held EDIT must all boot stock. Then return by § 3.5's card route. It must install (the slots are ours) and open.
8. **Guard refusal** (gate build only): a gate-only console command programs one foreign word into S5's middle with S4 erased. Both `bank2 <file>` and the boot must refuse with S0–S7 unchanged (read back by CRC). Clear it with the gate's `bank2 erase slots`. `check-no-gate` keeps both commands out of shipping builds with a new marker.
9. **Audio during the install:** 0 overruns.
10. **A refused boot's time**, measured in gate 8: the staged file's CRC plus the guard's scan.

## 7. Risks and what could not be verified

| Risk | Mitigation / status |
|---|---|
| **The unit's 1.09 binary may not match the source** | Gate 0, read-only, before any write |
| **The erased-slot hang is inferred** | Only the "before" row of § 5 rests on it |
| **A flash double-ECC error may not be a precise, maskable bus error** | Gate 1b; § 3.2's fallback mark if it isn't |
| **lld's `=0xFF` fill over `. =` gaps** | First spike; `check-slots` checks the bytes in every build |
| **A foreign slot on a tester's unit** | Refused, nothing written; recovery is the 0.3 package (§ 3.5). None known |
| **A refused unit already has the new bank 1** and runs with bank 2 closed, mostly silent | No DFU pre-check in 0.4 (decided); recovery as above |
| **A stray MIXER/EDIT press at boot** (the button still down) | Not checked in code; gates 2–3 look for it. The fix, if needed: ignore buttons already down at the first scan |
| **Install time:** 8 erases per seal change, ~1 s each and up to 4 s, so ~32 s worst before programming | Tool waits raised to 120 s; gate 1 logs it |
| **Boot cost:** a 1 MB CRC at every boot; a refused boot adds a ~1 MB card CRC and the guard's scan | Gates 1 and 10 measure them |
| **A 1 MB `put`** under the unit's 30 s `PUT_DEADLINE_MS` | Unmeasured. The boot's install from the card root doesn't use it. Raise the deadline if gate 1's flash-first path runs out |
| **An older Chimera reading format 2** | Reports `format 2` and reinstalls into S0–S3 (by reading `judge` and `plan`; not run) |
| **An upstream bootloader that writes the slots** | None exists; the guard refuses to overwrite one |
| **Whether `update` works on a unit already in DFU** (the way back from stock) | Not checked; the card route is documented instead |

## Open questions for the owner

1. **Every image is exactly 1 MB,** so every install erases all eight sectors (§ 1.1). It is simpler than an image that ends where its code does, at a few seconds more per install. Agreed?
2. **The fallback mark (§ 3.2),** if gate 1b shows ECC faults can't be masked: a foreign slot with an ECC error would then be erased rather than refused. Acceptable, given no foreign slot is known?
