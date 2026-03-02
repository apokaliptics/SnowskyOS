// ═══════════════════════════════════════════════════════════════════════════════
// hal/mmio.rs — Memory-Mapped I/O helpers & SoC register base addresses
// Target: Rockchip RKNanoD (Dual-Core ARM Cortex-M3)
// ═══════════════════════════════════════════════════════════════════════════════
//! Volatile, race-free register access for the RKNanoD APB/AHB peripheral bus.

use core::ptr;

// ── Rockchip RKNanoD peripheral base addresses ───────────────────────────────
// APB peripherals (0x4000_0000 region)
pub const CRU_BASE: usize   = 0x4000_0000; // Clock and Reset Unit
pub const TIMER_BASE: usize  = 0x4000_0400; // Timer0 / Timer1
pub const WDT_BASE: usize   = 0x4000_0800; // Watchdog timer
pub const GPIO0_BASE: usize  = 0x4000_1000; // GPIO bank 0
pub const GPIO1_BASE: usize  = 0x4000_1400; // GPIO bank 1
pub const I2C0_BASE: usize  = 0x4000_2000; // I2C controller 0 (DAC left, addr 0x30)
pub const I2C1_BASE: usize  = 0x4000_2400; // I2C controller 1 (DAC right, addr 0x32)
pub const SPI0_BASE: usize  = 0x4000_3000; // SPI master 0 (LCD)
pub const UART0_BASE: usize = 0x4000_3400; // UART0 (debug)
pub const PWM_BASE: usize   = 0x4000_3800; // PWM controller

// AHB peripherals (0x4004_0000 region)
pub const I2S0_BASE: usize  = 0x4004_0000; // I2S master 0 (audio output)
pub const I2S1_BASE: usize  = 0x4004_0400; // I2S master 1
pub const DMA_BASE: usize   = 0x4004_1000; // DMA controller (DMAC)
pub const EMMC_BASE: usize  = 0x4004_2000; // eMMC / SD controller

// Cortex-M system peripherals (standard ARM addresses)
pub const SCB_BASE: usize   = 0xE000_ED00; // System Control Block
pub const NVIC_BASE: usize  = 0xE000_E100; // Nested Vectored Interrupt Controller
pub const SYSTICK_BASE: usize = 0xE000_E010; // SysTick timer

// ── Safe volatile register access ────────────────────────────────────────────

/// Read a 32-bit MMIO register.
#[inline(always)]
pub fn read32(addr: usize) -> u32 {
    unsafe { ptr::read_volatile(addr as *const u32) }
}

/// Write a 32-bit MMIO register.
#[inline(always)]
pub fn write32(addr: usize, val: u32) {
    unsafe { ptr::write_volatile(addr as *mut u32, val) }
}

/// Set specific bits in a 32-bit MMIO register (read-modify-write).
#[inline(always)]
pub fn set_bits(addr: usize, mask: u32) {
    let v = read32(addr);
    write32(addr, v | mask);
}

/// Clear specific bits in a 32-bit MMIO register (read-modify-write).
#[inline(always)]
pub fn clear_bits(addr: usize, mask: u32) {
    let v = read32(addr);
    write32(addr, v & !mask);
}

/// Modify specific bits: clears then sets.
#[inline(always)]
pub fn modify_bits(addr: usize, clear_mask: u32, set_mask: u32) {
    let v = read32(addr);
    write32(addr, (v & !clear_mask) | set_mask);
}

/// Spin until `(read32(addr) & mask) == expected`.
#[inline]
pub fn wait_for(addr: usize, mask: u32, expected: u32) {
    while (read32(addr) & mask) != expected {
        core::hint::spin_loop();
    }
}

// ── Platform-level init ──────────────────────────────────────────────────────

/// Called once from `main()` to gate clocks and enable peripherals.
pub fn init_platform() {
    // Disable watchdog timer first
    write32(WDT_BASE + 0x00, 0); // WDT_CR: disable

    // ── Ungate peripheral clocks via CRU_CLKGATE register ───────────────
    // CRU_CLKGATE_CON: writing 1 to upper 16 bits enables write, lower 16 = gate
    // Clear gate bits to enable clock for each peripheral
    let clkgate = CRU_BASE + 0x80; // CRU_CLKGATE_CON0
    let current = read32(clkgate);
    // Enable: I2C0, I2C1, I2S0, SPI0, DMA, GPIO0, GPIO1, TIMER
    let enable_mask = (1 << 0)   // I2C0
                    | (1 << 1)   // I2C1
                    | (1 << 2)   // I2S0
                    | (1 << 3)   // SPI0
                    | (1 << 5)   // DMA
                    | (1 << 8)   // GPIO0
                    | (1 << 9)   // GPIO1
                    | (1 << 12); // TIMER
    // Write-enable bits in upper half, clear gate bits in lower half
    write32(clkgate, (enable_mask << 16) | (current & !enable_mask));
}
