/* ═══════════════════════════════════════════════════════════════════════════════
 * linker.ld — Supplementary linker script for Echo Mini OS
 * Target: Rockchip RKNanoD (Dual-Core ARM Cortex-M3)
 *
 * This script is loaded AFTER cortex-m-rt's link.x (via memory.x).
 * It adds sections specific to the Echo Mini firmware:
 *   - RKnanoFW header at the very start of SRAM (before FLASH origin)
 *   - DMA buffer section (32-byte aligned, no-init)
 *   - Heap boundaries for the buddy allocator
 *
 * Memory layout (1 MB SRAM at 0x0000_0000):
 *   0x0000_0000 – 0x0000_01FF : RKnanoFW header (512 bytes)
 *   0x0000_0200 – 0x000B_FFFF : FLASH region (code + rodata)  ~768 KB
 *   0x000C_0000 – 0x000F_FFFF : RAM region (data/bss/heap/stack) 256 KB
 * ═══════════════════════════════════════════════════════════════════════════════ */

SECTIONS
{
    /* ── RKnanoFW firmware header (0x200 = 512 bytes) ──────────────────────
     * Placed at SRAM origin BEFORE the vector table.  The Python packaging
     * tool fills this; here we just reserve the space so the linker doesn't
     * overlap code into it.
     */
    .fw_header 0x00000000 (NOLOAD) :
    {
        . = . + 0x200;
    }

    /* ── DMA buffers (uncached, 32-byte aligned, no-init) ─────────────────
     * Audio ring-buffers and framebuffer live here.  Placed at the end of
     * the RAM region, growing downward from the stack.  We carve a 32 KB
     * region between .bss end and the heap start.
     */
    .dma_buffers (NOLOAD) :
    {
        . = ALIGN(32);
        __dma_start = .;
        *(.dma_buffers .dma_buffers.*)
        . = ALIGN(32);
        __dma_end = .;
    } > RAM

    /* ── Heap: from end of DMA buffers to bottom of stack ─────────────────
     * The buddy allocator uses these symbols.
     */
    . = ALIGN(16);
    __heap_start = .;
    __heap_end   = _stack_start - 4096;  /* Leave 4 KB guard below stack */
}
