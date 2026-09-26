MEMORY
{
    FLASH (rx) : ORIGIN = 0x08020000, LENGTH = 896K
    RAM  (rwx) : ORIGIN = 0x24000000, LENGTH = 512K
    /* D2 SRAM1 (128K) + SRAM2 (128K) + SRAM3 (32K), contiguous (RM0433 §2.3) */
    RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 288K
    DTCM (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
}

/* ADR 0020: the stack lives in DTCM (zero-wait, CPU-only), not beside the framebuffer. */
_stack_start = ORIGIN(DTCM) + LENGTH(DTCM);
_stack_end = ORIGIN(DTCM);
