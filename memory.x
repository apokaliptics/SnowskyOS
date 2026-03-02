/* ═══════════════════════════════════════════════════════════════════════════════
 * memory.x — Memory regions for cortex-m-rt (Rockchip RKNanoD)
 *
 * The RKNanoD has 1 MB of SRAM starting at 0x0000_0000.
 * The RKnanoFW bootloader loads the firmware into SRAM and jumps to the
 * vector table at offset 0x200 (after the 512-byte firmware header).
 *
 * cortex-m-rt convention:
 *   FLASH — code + read-only data (vector table, .text, .rodata)
 *   RAM   — read-write data (.data, .bss, heap, stack)
 * ═══════════════════════════════════════════════════════════════════════════════ */

MEMORY
{
    /* Code region: starts after 512-byte RKnanoFW header */
    FLASH (rx)  : ORIGIN = 0x00000200, LENGTH = 768K

    /* Data region: upper 256 KB of SRAM for .data/.bss/heap/stack */
    RAM   (rwx) : ORIGIN = 0x000C0000, LENGTH = 256K
}

/* Stack starts at top of RAM (grows downward) — cortex-m-rt uses this */
_stack_start = ORIGIN(RAM) + LENGTH(RAM);
