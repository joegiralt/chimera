/* Copyright (C) 2026 Joe Giralt */
/* SPDX-License-Identifier: GPL-3.0-or-later */

MEMORY
{
    FLASH (rx) : ORIGIN = 0x08020000, LENGTH = 896K
    /* Bank 2, S0..S7 (ADR 0137, 0272): `.bank2` only, split off into the
       .bk2. Bootloader 1.09's slots at 0x08180000 and 0x081C0000 hold
       Chimera's stubs (bank2.x). */
    BANK2 (rx) : ORIGIN = 0x08100000, LENGTH = 1M
    RAM  (rwx) : ORIGIN = 0x24000000, LENGTH = 512K
    /* D2 SRAM1 (128K) + SRAM2 (128K) + SRAM3 (32K), contiguous (RM0433 §2.3) */
    RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 288K
    /* D3 SRAM4 (64K), no clock enable needed (RM0433 §2.3) */
    RAM_D3 (rwx) : ORIGIN = 0x38000000, LENGTH = 64K
    DTCM (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
    /* ITCM (64K, CPU only, zero wait): NOLOAD state built in place (itcm.x,
       ADR 0266). From 0x400, so nothing sits at or near address 0. build.rs
       writes the region here from chimera-engine/src/itcm_geometry.rs
       (ADR 0286). */
    /* ITCM REGION */
}

/* ADR 0020, 0025: the stack lives in DTCM (zero-wait, CPU-only), not beside
   the framebuffer. The wave-table copy takes the top of DTCM (dtcm.x); the
   stack is everything below it. */
_stack_start = __sdtcm_waves;
_stack_end = ORIGIN(DTCM);
