/* Copyright (C) 2026 Joe Giralt */
/* SPDX-License-Identifier: GPL-3.0-or-later */

MEMORY
{
    FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 128K
    RAM  (rwx) : ORIGIN = 0x24000000, LENGTH = 512K
}
