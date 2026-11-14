# Bank 2 into the bootloader slots: Design

- **Status:** Draft for the owner (design only: no firmware change yet)
- **Date:** 2026-11-14
- **Owner decision (2026-11-14):** open bank-2 sectors 4–7 (0x08180000–0x081FFFFF, 4 × 128 KB = 512 KB; the earlier "256 KB" was wrong) to Chimera's code, so 0.4 keeps all its features.
- **ADR:** [0272](../../adr/0272-bank-2-extends-into-the-bootloader-slots.md) (Proposed), superseding in part [0137](../../adr/0137-bank-2-holds-sealed-code-behind-one-door.md)
- **Builds on:** ADR 0137 (one link, one door, one seal), ADR 0251 (bank 1 below 8 KB of bank 2), spec `2026-10-11-bank2-design.md` (its decision 6, "the slots stay untouched for good", is what this reverses), research (private notes) (Ixox/preenfm3 at 65cb963)

## Where we are

| Area | Range | Free today |
|---|---|---|
| Bank 1 firmware | 0x08020000–0x080FFFFF (917,504 B; SD-flash limit) | ~43 KB |
| Bank 2 S0–S3 (`.bank2`) | 0x08100000–0x0817FFFF (512 KB) | ~8 KB, at ADR 0251's floor |
| Bank 2 S4–S7 (the slots) | 0x08180000–0x081FFFFF (512 KB) | erased, unused, MPU no-access |

The facts the design rests on (research report, upstream source; see § 7 for what is not verified on the unit):
- Bootloader 1.09 reads the buttons once at power-on. Held MIXER (index 9) goes to `bootJumpToApplication(0x08180000)`, held EDIT (index 10) to 0x081C0000, MENU (6) stays in the bootloader, and anything else goes to 0x08020000 (`bootloader/Src/main.c:116-129`). All three jumps call `MX_Deinit()` first and use the same `bootJumpToApplication`.
- The jump loads MSP from word 0 and branches to word 1. There is no validity check, and VTOR is not set (`bootloader.cpp:304-310`). The normal path to 0x08020000 doesn't set VTOR either: the firmware sets its own (Chimera through cortex-m-rt's `set-vtor`).
- Today an erased slot loads SP 0xFFFFFFFF and branches to 0xFFFFFFFF. That faults into the bootloader's `while(1)` HardFault handler, and the unit hangs until power is cycled. This is inferred from the source and has not been tried on the unit.
- The bootloader never erases or writes bank 2. `formatFlash` erases bank-1 sectors only, and `FLASH_If_Erase` (the bank-2 mass erase) is dead code. Stock firmware has no flash write code at all.
- No firmware, fork or documented user path has ever put anything in the slots.

## Summary

- **One link, one door, one seal, one header, one `.bk2`.** The image's body still runs contiguously from 0x08100000. It can now reach 0x081FFFFF instead of stopping at 0x0817FFFF.
- **Three output sections in that one link:** `.bank2` (S0–S3, as today), `.bank2_s4` (from 0x08180000) and `.bank2_s6` (from 0x081C0000). Each high section opens with a 1 KB **slot reserve**. The reserve holds a 64-byte **slot stub**, a valid vector table whose reset handler sets VTOR to 0x08020000 and jumps into whatever firmware is in bank 1. Holding MIXER or EDIT at power-on then boots normally, into stock or Chimera.
- **Every image from now on holds both stubs.** They are fixed bytes the same in every release, so any Chimera install leaves the slots safe, and so does a later revert to stock.
- **The installer guard** runs before anything is erased. Each slot (S4+S5, S6+S7) must be erased or carry Chimera's stub tag; otherwise the install refuses, writes nothing anywhere and says so on screen. The install is still one package, one command and one install screen.
- **MPU:** region 2 grows from 512 KB to 1 MB in its Read, Write and Open states. Region 1 (1 MB, no access) stays for Closed.
- **Header format 2:** the same 256-byte layout, but `body_len` may run to 1 MB − 256. Older firmware then reports `format 2` rather than `corrupt`.

## 1. Layout

### 1.1 The address map

```
0x08100000  S0  header (256 B) | door 0x08100100 | fixed tables after it | .bank2 ...
0x08120000  S1  .bank2 ...
0x08140000  S2  .bank2 ...
0x08160000  S3  .bank2 ... __bank2_end (<= 0x08180000)
0x08180000  S4  slot reserve MIXER (1 KB: stub 64 B, rest 0xFF) | .bank2_s4 ...
0x081A0000  S5  .bank2_s4 ... __bank2_s4_end (<= 0x081C0000)
0x081C0000  S6  slot reserve EDIT (1 KB) | .bank2_s6 ...
0x081E0000  S7  .bank2_s6 ... __bank2_s6_end (<= 0x08200000)
```

Room: 512 KB + 2 × 255 KB = **1,046,528 B** of bank 2 for code and data. That is ~510 KB more than today, ~518 KB free in all.

### 1.2 Why one link, but three sections

There is still **one image**, for the reasons ADR 0137 gives: same compiler, same link, no ABI, and the seal as an exact compatibility key. The header, door, seal, CRC, commit word and judge are unchanged in kind. The `.bk2` is one file: `rust-objcopy -j .bank2 -j .bank2_s4 -j .bank2_s6 --gap-fill 0xff -O binary`. Its body is contiguous from 0x08100000 to `__bank2_s6_end`, with the gaps between sections as 0xFF. The CRC and seal cover all of it, stubs and gaps included, and an erased gap reads back as the 0xFF the file holds. The installer already skips all-0xFF flash words.

The link is three output sections rather than one because the two stubs are **fixed points in the middle of the bank**, and rust-lld won't flow input sections around a fixed address inside one output section. Once the location counter passes 0x08180000 it can't go back. So:

- `memory.x`: `BANK2 (rx) : ORIGIN = 0x08100000, LENGTH = 512K` stays. Two regions are added, `BANK2_S4 (rx) : ORIGIN = 0x08180000, LENGTH = 256K` and `BANK2_S6 (rx) : ORIGIN = 0x081C0000, LENGTH = 256K`, so an overflow is the linker's own error, region by region.
- `bank2.x` (written by `chimera-stm32/build.rs`) gains two output sections after `.bank2`:
  ```
  .bank2_s4 : {
    __slot_mixer = .;  KEEP(*(.bank2.slot.mixer));  . = __slot_mixer + 1K;
    /* S4 GLOBS */
    . = ALIGN(32); __bank2_s4_end = .;
  } > BANK2_S4
  .bank2_s6 : { __slot_edit = .; KEEP(*(.bank2.slot.edit)); . = __slot_edit + 1K;
    /* S6 GLOBS */  . = ALIGN(32); __bank2_s6_end = .; } > BANK2_S6
  ```
  The new asserts are `__slot_mixer == 0x08180000`, `__slot_edit == 0x081C0000`, `SIZEOF(.bank2_s4) >= 1K`, `SIZEOF(.bank2_s6) >= 1K`, `__bank2_s4_end <= 0x081C0000` and `__bank2_s6_end <= 0x08200000`. `__bank2_end <= 0x08180000` stays, with its message changed to "S0–S3 end before the MIXER slot".
- **Which code goes high:** whole glob groups, chosen by hand, the way `bank2.x` already places subsystems. Input sections go to the first output statement that matches, and the groups are disjoint, so moving a group is a cut and paste from one section's list to another's. Good candidates are whole subsystems with no fixed table: 0.4's DRUM kernels (ADR 0251 put ~50 KB of them in bank 1), the reverb TYPEs, ORBIT's runner, and new 0.4 features. Fixed door tables stay in S0, as now.
- The **door doesn't change.** It is still one table at 0x08100100 with its fixed slots after it. Door entries may point into `.bank2_s4`/`.bank2_s6`, because those are bank-2 addresses in the same link. `door-check` already bounds its range by `BANK2_BASE + BANK2_LEN`, so it covers 1 MB once `BANK2_LEN` grows.

**Rejected, a second image or door for S4–S7:** this is ADR 0137's "two images, stable ABI" again, with a second `core`/`libm`, `extern "C"` everywhere, and a second seal and install. It costs more than hand-placing groups.

**Rejected, the stubs written once and kept outside the image:** S4 and S6 would never be erased, so only S5 and S7 (256 KB) would hold code. That halves the gain to save a few seconds of install.

**Deferred, `--enable-non-contiguous-regions`:** GNU ld 2.33+, and lld from LLVM 19 as far as I know (*not verified for the toolchain's rust-lld*). It would let one glob list spill across the three regions on its own. Revisit only if hand-packing groups becomes a chore. Either way, placement stays checked by `check-door-placed` and the asserts.

### 1.3 Keeping the two jump targets safe

- **Only the stub may sit at the entry words.** The `KEEP` of `.bank2.slot.*` comes first in its section, and the asserts pin each stub symbol to its address. A `.bank2_s4` that is empty apart from its reserve is still emitted, because the stubs are always linked (§ 2.3).
- **Nothing else may land in a reserve.** `just check-slots` (§ 6) fails the build if any symbol other than the stub sits in [0x08180000, 0x08180400) or [0x081C0000, 0x081C0400). It also fails if the `.bk2`'s bytes at file offsets 0x80000 and 0xC0000 (the file starts at 0x08100000, header included) aren't `chimera_image::slot::stub(base)`.
- **The installer writes each stub straight after erasing its sector**, and before any body word (§ 3.2). That keeps short the window in which a held button would find an erased entry.

## 2. Slot entry stubs

### 2.1 What the stub does

Bootloader 1.09 does the same thing on every jump: `MX_Deinit()`, MSP ← word 0, branch to word 1. The stub only has to turn a slot jump into the normal-path jump to 0x08020000, plus setting VTOR:

```
word 0   0x2002_0000          initial SP: DTCM's top, 8-byte aligned (the stub uses no stack)
word 1   base + 0x09          reset, Thumb bit set: the code at base + 8
base+8:  ldr  r0, =0x08020000 bank 1's vector table
         ldr  r1, =0xE000ED08 SCB->VTOR
         str  r0, [r1]
         dsb
         isb
         ldr  r1, [r0]        bank 1's initial SP
         ldr  r2, [r0, #4]    bank 1's reset vector
         msr  msp, r1
         bx   r2
         (literal pool: 0x08020000, 0xE000ED08)
base+0x30: tag "CHIMERA-SLOT" (12 B), version u8 = 1, slot u8 (0 MIXER, 1 EDIT), 0xFFFF
```

That is about 48 bytes of vectors, code and literals, plus 16 bytes of tag: **64 B**, two flash words. The rest of the 1 KB reserve stays 0xFF.

- **No validity check of bank 1.** The stub does exactly what the bootloader's normal path does. If bank 1 is erased, a held button fails the same way a plain power-on does, and the stub is no worse. A check would add code and a second failure mode for nothing.
- **It works for stock and Chimera alike.** It reads bank 1's vectors at run time, so it carries no build's addresses. Stock's `SystemInit` sets VTOR again (`system_stm32h7xx.c:108,229`), and so does Chimera's cortex-m-rt (`set-vtor`).
- **Same state as a normal boot.** Clocks, GPIO, SysTick and NVIC are what `MX_Deinit` left, because that is the same call as on the normal path. VTOR is set before the jump, so a pending interrupt taken after it goes through bank 1's table, not the bootloader's.
- **Why a 1 KB reserve for 64 bytes:** a full H7 vector table (16 + 150 vectors, 664 B) fits in it at the 1 KB alignment VTOR wants, so a later stub could own its vectors without moving any code. The cost is 2 × 960 B.

### 2.2 Where the bytes come from

There is one source: `chimera_image::slot::stub(base: u32) -> [u8; 64]`, a `const fn` in the host-tested crate. It is hand-encoded Thumb-2, and its only parameter is the reset word.
- The firmware places it as `#[used] #[link_section = ".bank2.slot.mixer"] static SLOT_MIXER: [u8; 64] = stub(0x0818_0000);`, and `.edit` the same way. It is data, not compiled code, so the outliner, LTO and the door check never see it.
- `chimera-utils bank2-seal --check` and `just check-slots` compare the `.bk2`'s bytes with `stub(base)`. A host test decodes the instructions (§ 6.2), and `check-slots` also disassembles the ELF's two reserves with `rust-objdump` and matches the listing above.
- The installer guard reads the tag (`CHIMERA-SLOT`) only, not the whole stub. A future stub version (the version byte) is still recognised as Chimera's.

### 2.3 Every image holds both stubs

From 0.4 on, every `.bk2` places both stubs, however little code sits in S4–S7. So:
- every install erases S0 through S6 at least, plus S7 when `.bank2_s6` reaches it;
- the `.bk2` is at least 0xC0400 + 256 B (~769 KB), most of it 0xFF;
- a held MIXER or EDIT boots normally on every unit that ran a 0.4+ install, and it keeps doing so after a revert to stock (§ 5).

**Rejected, stubs only when an image needs S4–S7:** the slots' state would then depend on the release, and so would the revert story and the guard. The saving is ~3 s of erase per seal change.

## 3. Installer guard

### 3.1 The rule

Before the first erase of an install (`chimera_image::install_with`, after judging the staged file `Good` and before `Phase::Erasing`), the guard checks each **slot** the plan reaches, MIXER = S4+S5 and EDIT = S6+S7. A slot passes if either:
- **it is erased:** every word of both its sectors reads 0xFFFFFFFF; or
- **it is Chimera's:** its first sector's bytes at 0x30..0x3C are `CHIMERA-SLOT`. S5's and S7's contents are then Chimera's by the slot rule: Chimera erases S5 before S4, and S7 before S6 (§ 3.2). So the tag in S4 means the whole slot was last written, or is being written, by Chimera.

Anything else is **foreign**. The install then returns `InstallFail::SlotForeign(Slot::{Mixer, Edit})` before any erase or program. Bank 2 is left exactly as it was (S0–S3 included: a partial image is no use), and so is the staged file.

The guard is pure and lives in `chimera-image`. It reads through `Bank2Memory` (live flash), and the host tests drive it against `SimFlash`. Reading 2 × 256 KB through region 2 in `Write` state (strongly-ordered) takes a few ms. A double-ECC error while reading a foreign slot is handled as any bank-2 ECC fault is (ADR 0137): one reset with the `BANK2_INSTALLING` mark, then S0 erased and nothing installed. The next boot doesn't try that file again. It is bounded, and the slot is still never written.

### 3.2 Install order (power-cut safe, guard-consistent)

1. Judge the staged file (unchanged).
2. **Guard** (§ 3.1). On a refusal: the screen (§ 3.3), nothing written.
3. Erase **S0** (the commit word goes first, as now), then S1, S2, S3.
4. MIXER slot: erase **S5**, then **S4**, then program the MIXER stub's two words at once.
5. EDIT slot: erase **S7** (when planned), then **S6**, then program the EDIT stub.
6. Program the body from 0x100 up, as now, **skipping the two reserves** (already programmed; their bytes match the file, and the read-back CRC checks them). The rest of each reserve is 0xFF and was skipped anyway.
7. Header words, uncommitted judge, commit word last, `Door::open` (unchanged).

A power cut at any point leaves every slot erased or tagged:

| Cut after | S4 | S5 | Guard next time | Held MIXER at power-on |
|---|---|---|---|---|
| step 3 | old tag | old | Chimera's | old stub → boots |
| S5 erase | old tag | erased | Chimera's | old stub → boots |
| S4 erase | erased | erased | erased | **hangs** (as today) until power is cycled |
| stub program | tag | erased | Chimera's | stub → boots |

On a unit whose slots were never touched, the first install goes from erased to tagged. There is one ~1 s window (S4's erase plus two word programs) in which a power cut and then a held MIXER at the next power-on hangs as it does today. A plain power-on then boots and reinstalls.

`plan()` names the sectors in that order. `Bank2Sector` gains S4–S7, but the writer's `erase` stays per-sector: still no bank or mass erase, no bank-1 or option-byte register.

### 3.3 What the user sees on a refusal

**The boot's install** (the one-step path: a `.bk2` in the card root or `/CHIMERA/SYSTEM/BANK2.BK2`). The guard runs before DON'T POWER OFF is drawn, so a refusal never shows the install screen. Instead it shows one screen, held 5 s or until any button:

```
NOT INSTALLED
THE MIXER SLOT HOLDS
OTHER CODE.
NOTHING WAS WRITTEN.
```

The unit then boots with bank 2 as it was: open, if the old image is still `Good` for this build (it never is after a real update), or closed. `status` reads `bank2 slot foreign mixer` (or `edit`) as its bank2 line, and SETTINGS › SYSTEM › ABOUT reads `SLOT IN USE` in place of `NEEDS UPDATE`. `Bank2Status` gains `SlotForeign(Slot)`. The boot refuses again at each power-on (one header read and a few ms of guard), which is what we want: it never decides to write.

**`chimera-usb.py update`** exits 2 with `update: the unit's MIXER slot (0x08180000) holds other code; bank 2 not installed`, from the final `status` check it already makes. `bank2 <file>` gives the same answer as its install line.

One package, one command and one install screen still hold. The refusal needs no extra file or step. It is a new outcome of the same step.

**What the guard can't prevent:** by the time the first boot runs the guard, the new bank 1 is already flashed (by DFU or the bootloader). A refused unit runs the new bank 1 with bank 2 closed, and many engines' blocks sit behind the door, so it is mostly silent until the slot is cleared or the previous release is reflashed. See open question 2 for a pre-check that refuses before bank 1 is touched.

## 4. The MPU

ADR 0137's regions, changed:

| Region | ADR 0137 | Now |
|---|---|---|
| 0 | DMA, D2's first 16 KB | unchanged |
| 1 | 0x08100000, 1 MB, no access, XN, strongly-ordered, always on | **unchanged**. It is the whole bank's floor while region 2 is `Closed` |
| 2 | 0x08100000, **512 KB**: Closed (off) / Read (RO, XN, Normal NC) / Write (RW, XN, SO) / Open (RO, exec, Normal WT) | 0x08100000, **1 MB** (`SIZE_1M` = 19 << 1), the same four states and attributes |

- **The RASR words change** to `BANK2_READ_RASR` 0x1608_0027, `BANK2_WRITE_RASR` 0x1300_0027 and `BANK2_OPEN_RASR` 0x0602_0027. Their `const` asserts in `chimera-board/src/cache.rs` change with them. In every state but Closed, region 2 now covers region 1 exactly, and the higher number wins.
- **Into `Open`:** the same whole-cache clean and invalidate, DSB and ISB. No cache operation names a bank-2 address, so none can fault.
- **Subregions:** 1 MB has eight 128 KB subregions, one per sector, and SRD could hide the sectors an image doesn't reach. *Rejected:* the door check already proves bank 1 holds no reference past the image, a stale sector's bytes are never called, and it adds a header-dependent MPU word to test.
- The console's `flash` still peeks only at S0 (`PEEK_AT`). Its comment ("the slot at 0x08180000 is never read") becomes "is not peeked".
- **ECC:** `bank2_ecc_fault`'s bound `BANK2_LEN / FLASH_WORD` becomes the whole bank (FA2R's 15 bits span it). A double-ECC fault anywhere in bank 2 marks `BANK2_BAD`. The next boot erases S0, so bank 2 is `Empty` and its other sectors aren't read that boot, and the next install erases the rest. An ECC error inside a **stub** is only ever read by the bootloader, on a held button, and then hangs as an erased slot does today.

## 5. Reversibility

| State of the unit | Normal power-on | Held MIXER | Held EDIT |
|---|---|---|---|
| Stock, or Chimera ≤ 0.3 (slots erased) | that firmware | **hangs** until power is cycled (inferred, untested) | **hangs** |
| Chimera 0.4+, installed | Chimera | stub → Chimera | stub → Chimera |
| 0.4 install torn (power cut) | Chimera, which reinstalls | stub → Chimera, except in the S4-erase window (§ 3.2) | same, for S6 |
| Reverted to stock after 0.4 | stock | stub → stock | stub → stock |
| Reverted to Chimera ≤ 0.3 after 0.4 | 0.3 | stub → 0.3 | stub → 0.3 |
| A foreign slot, guard refused | new Chimera, bank 2 closed | whatever the foreign code does | same |

- **Going back to stock:** flash stock's `.bin` at 0x08020000, by the bootloader's SD flash or DFU, as today. Stock never writes flash and the bootloader never erases bank 2, so the stubs and the dead Chimera code stay. The stubs make a held MIXER or EDIT boot stock instead of hanging, which is better than a stock unit. Nothing needs cleaning, and testers are never given an erase command (`no-destructive-commands-for-testers`).
- **Going back to Chimera ≤ 0.3:** 0.3's judge reads the 0.4 header's `format 2` and reports it. Its boot then installs its own `.bk2` into S0–S3 only (its `plan` knows nothing past S3), and S4–S7 keep 0.4's stubs and stale code. 0.3's MPU region 1 keeps that code no-access, and nothing in 0.3 references it.
- **A later release that no longer needs S4–S7 code:** it still links both stubs (§ 2.3), so nothing changes for users and the slots stay Chimera's. Handing the slots back, by erasing S4–S7 and leaving the button hang as it was, would take a new ADR. It isn't planned.
- **Upstream:** a future bootloader (nvmker's 1.10 has the same jump) could validate the slots, move them, or start using them. The stubs are ordinary vector tables, so a validity check of SP and reset would pass them. A bootloader that *writes* the slots would be a foreign writer, and our guard would then refuse to overwrite it.

## 6. Budget and checks

### 6.1 `just check` and the build

- **`just check-bank2 <set>`** (new, beside `check-bank1`): it reads `__bank2_end`, `__bank2_s4_end` and `__bank2_s6_end` from the ELF and prints one line a region plus a total:
  ```
  bank 2 S0-S3   521,920 B used   2,368 free
  bank 2 S4-S5    61,440 B used 200,704 free  (1 KB slot)
  bank 2 S6-S7     1,024 B used 261,120 free  (1 KB slot)
  bank 2 total                  464,192 free
  ```
  It fails when the total falls under ADR 0251's 8 KB floor. It also holds each set to its line in a new `tools/bank2-budget.txt` (the bank-1 budget's rules: +64 B tolerance, the line lowered after 1 KB freed, each raised line naming its ADR and feature). `just check` runs it for `default`, `orbit` and `bank2-gate`, after their `check-bank1`.
- **ADR 0251's measure** "0x08180000 − `__bank2_end`" becomes the total above. The rule ("bank 2 first while ≥ 8 KB free") is unchanged, but there is room again, so new features go to bank 2. Features ADR 0251 put in bank 1 (DRUM slice 1) may move back if bank 1 needs the room.
- **`check-door-placed`:** its awk bound `a >= 0x08180000` becomes `a >= 0x08200000`, and it also fails on any symbol inside the two reserves.
- **`just check-slots <elf> <bk2>`** (new, run by `bank2-split`): it checks the stubs' symbols at their addresses, the `.bk2` bytes at file offsets 0x80000 and 0xC0000 equal to `stub(base)`, the `rust-objdump -d` listing of each reserve's code matching § 2.1's sequence, and no other symbol in a reserve.
- **`bank2-split`:** its objcopy lines become `-R .bank2 -R .bank2_s4 -R .bank2_s6` for the `.bin`, and `-j` of all three with `--gap-fill 0xff` for the `.bk2`. `tools/test_justfile.py`'s `test_every_binary_objcopy_drops_bank2` checks all three names, and `test_bank2_x_asserts` checks for `0x081C0000` and `0x08200000` too.
- **`chimera-image`:** `BANK2_LEN` becomes 1 MB; `FORMAT` 2; `Bank2Sector` S0–S7, with `const _: () = assert!(8 * SECTOR_LEN == BANK2_LEN)`; `Bank2Word` bounded to 1 MB; and a new `slot` module (`stub`, `Slot`, `TAG`, the guard). `bank2_registers_test.rs` changes from "no literal ≥ S4" to "no literal ≥ 0x08200000, and no bank-1 or option register".
- **`stack_depth.py`** reads `.bank2` with `objdump -s -j .bank2`. It reads the two new sections as well once code lands in them.
- **`check-bin-size`** is unchanged: bank 1 stays under 917,504 B.

### 6.2 Host tests

- **Stub bytes** (`chimera-image/tests/slot_test.rs`): for both bases, word 0 is 8-aligned in DTCM and word 1 is `base + 9`. The code decodes, by a small table of the expected Thumb-2 encodings, to § 2.1's sequence. The literals are 0x08020000 and 0xE000ED08, the tag is at 0x30, and the stub is 64 B.
- **Layout** (`install_test.rs`): `plan()` for bodies ending in each sector gives the order S0, S1, S2, S3, S5, S4, then S7, S6 (S7 only when reached). `Bank2Sector::addr` holds for S0–S7. `Bank2Word` takes 0x081FFFE0 and refuses 0x08200000. `judge` returns `Corrupt` for `body_len` > 1 MB − 256, and `Format(1)` for an old header.
- **Guard against `SimFlash`:** both slots erased → install; tagged → install; one foreign word anywhere in S4–S7 with no tag → `SlotForeign`, and **SimFlash records no erase and no program**. A tag in S4 with foreign bytes in S5 counts as Chimera's (the slot rule). An erased S4 with a non-erased S5 is foreign, which the order of § 3.2 never produces.
- **Power cut:** the existing every-step, every-word cut test is extended through steps 3–6. After each cut, the guard on the resulting flash must say erased or Chimera's for both slots, and a reinstall must end `Open`. That proves Chimera never locks itself out.
- **Install skips the reserves:** no `program` of a reserve word after its stub step (`SimFlash` rejects a second program).
- **`chimera-usb.py`** (`test_chimera_usb.py`): `update` and `bank2` map `bank2 slot foreign mixer|edit` to exit 2 with the message, against the fake unit.
- **Desktop:** the refusal screen and ABOUT's `SLOT IN USE` are rendered as screen goldens.

### 6.3 Hardware gates (owner's unit, only after the owner OKs flashing)

These are run over the USB console with `chimera-usb.py update`, never inside a timeout (`no-timeout-on-update`). The held-button power-ons need the owner's hands. A known-good 0.3 `.bin` + `.bk2` stays ready for the sanctioned DFU recovery.

0. **Read-only, before any write:** a `dfu-util -U` upload of 0x08000000 (128 KB) to confirm the unit's 1.09 binary holds the literals 0x08180000 and 0x081C0000 and compares with 9 and 10. Also a read of 0x08180000 and 0x081C0000 (256 KB each), expected all 0xFF, since nobody has written them.
1. **Flash:** `update` the 0.4 package. `status` must read `bank2 open … seal=<the package's>`, and the boot's install time is logged (expected ~10–15 s, against ~5 s today).
2. **Held MIXER at power-on** (owner): Chimera boots to its normal screen, `status` reads open, and audio plays. A stray MIXER press reaching the UI at boot is noted (see § 7).
3. **Held EDIT at power-on** (owner): the same.
4. **Cold boot** (power pulled, no button): Chimera, open.
5. **Update twice:** the same package again (`OK already installed`, nothing erased), then a build with a different seal (a full reinstall through S7). After each, gates 2–3 are repeated once.
6. **Power pull during the install** (the `bank2-gate` build's marks gain one after S5's erase, one after S4's and one after the MIXER stub): three pulls at each mark. Each time a normal power-on reinstalls and ends open, and a held MIXER boots except at the "after S4's erase" mark, where it hangs and a power cycle recovers.
7. **Revert to stock:** stock v1.03's `.bin` by DFU at 0x08020000. A normal power-on, a held MIXER and a held EDIT must all boot stock. Then `update` back to 0.4, which must install (the guard reads the slots as Chimera's) and open.
8. **Guard refusal** (`bank2-gate` build only): a gate-only console command programs one foreign word into S5 with S4 erased. The next `bank2 <file>` must answer `slot foreign mixer`, and the boot must show the refusal screen with S0–S7 unchanged (a read-back of their CRCs). A gate-only `bank2 erase slots` then clears it. Neither command exists in a shipping build: `check-no-gate` covers it with a new marker.
9. **Audio during the longer install:** 0 overruns, as Phase 0b showed for S0.

## 7. Risks and what I could not verify

| Risk | Mitigation / status |
|---|---|
| **The unit's 1.09 binary may not match the source at 65cb963** (buttons, addresses, no check) | Gate 0's read-only upload before any write. Last bootloader commit is c5be56953 = 1.09, so a match is very likely |
| **The hang on an erased slot is inferred, not tested** | Only the "before" row of § 5 depends on it. The design makes the slots boot either way |
| **A foreign slot on some tester's unit** | Refused with nothing written; the unit then runs with bank 2 closed (§ 3.3). Nobody has ever used the slots (research Q2), so this is expected to be zero units |
| **Stray MIXER/EDIT press at boot:** the held button is still down when Chimera starts | Not checked in code. If the first scan reads a held button as a press, Chimera may open MIX or EDIT. Gates 2–3 look for it; the fix, if needed, is to ignore buttons already down at the first scan |
| **Interrupt state after `MX_Deinit`** | The same as the normal path, which ships today. The stub adds only VTOR |
| **Install time and wear:** 7–8 sector erases per seal change instead of ≤ 4 | ~1 s each. Wear is unchanged in kind: only on a seal change, 10 k cycles |
| **`.bk2` size:** ≥ ~769 KB (0xFF gaps), up to 1 MB | Card space is trivial. `put` over USB takes longer, unmeasured. The boot's install from the card root doesn't involve USB. Gzip or sparse `.bk2` are rejected: the judge and the installer read the file as flash |
| **Torn-install window** (S4 erased, stub not yet programmed) | ~1 s per install; a held button then hangs as today; a power cycle recovers |
| **rust-lld and three output sections with INSERT BEFORE .text** | The same mechanism as today's `.bank2`. *Verify on the first spike* that `.bank2_s4`/`.bank2_s6` keep their order and that `--gc-sections` keeps the reserves (`KEEP`) |
| **`--enable-non-contiguous-regions` in rust-lld** | Not verified, and not used (§ 1.2) |
| **Older Chimera reading a format-2 header** | Reports `format 2` and installs its own image into S0–S3 (by source reading of `judge` and `plan`; not run) |
| **An upstream bootloader that writes the slots** | None exists; the guard refuses to overwrite one |
| **Stock firmware's linker claims 0x08020000–0x08200000** | Its image is far under 1 MB, and a ≥ 917,504 B `.bin` is refused by `check-bin-size`. *Not measured:* stock v1.03's actual size |

## Open questions for the owner

1. **Refusal wording and hold:** `NOT INSTALLED / THE MIXER SLOT HOLDS OTHER CODE. / NOTHING WAS WRITTEN.` for 5 s or a button, and ABOUT `SLOT IN USE`. Is that right?
2. **Pre-check in the tool:** should `chimera-usb.py update` read the slots over DFU (read-only `dfu-util -U`) before flashing bank 1, and refuse a foreign slot before anything is written? Recommended: **no for 0.4**. No known unit has foreign slot code, and the on-unit guard is the one every route goes through.
3. **Stubs in every image** (§ 2.3), at ~3 s more install time per seal change: agreed?
4. **What goes high first:** move DRUM's kernels back from bank 1 into `.bank2_s4` (ADR 0251's bank-1 room returns), or keep bank 1 as is and put only new 0.4 work in S4–S7?
