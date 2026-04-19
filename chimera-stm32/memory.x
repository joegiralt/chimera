MEMORY
{
    /* Bootloader at 0x08000000, firmware starts at 0x08020000 */
    FLASH  (rx)  : ORIGIN = 0x08020000, LENGTH = 896K
    /* Use AXI-SRAM as main RAM (512K, fits all BSS including delay buffers) */
    RAM    (rwx) : ORIGIN = 0x24000000, LENGTH = 512K
    DTCM   (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
    RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 288K
    RAM_D3 (rwx) : ORIGIN = 0x38000000, LENGTH = 64K
}

/* Stack in AXI-SRAM (top of RAM) — cortex-m-rt requires stack above data */
_stack_start = ORIGIN(RAM) + LENGTH(RAM);
