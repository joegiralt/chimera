MEMORY
{
    /* Bootloader at 0x08000000, firmware starts at 0x08020000 */
    FLASH  (rx)  : ORIGIN = 0x08020000, LENGTH = 896K
    DTCM   (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
    RAM    (rwx) : ORIGIN = 0x24000000, LENGTH = 512K
    RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 288K
    RAM_D3 (rwx) : ORIGIN = 0x38000000, LENGTH = 64K
}

/* Place stack in DTCM for zero-wait-state access */
_stack_start = ORIGIN(DTCM) + LENGTH(DTCM);
