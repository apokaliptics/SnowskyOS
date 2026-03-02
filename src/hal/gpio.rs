// ═══════════════════════════════════════════════════════════════════════════════
// hal/gpio.rs — GPIO driver for Rockchip RKNanoD (GPIO banks 0–1)
//
// The RKNanoD has a standard Synopsys DesignWare GPIO controller with
// 2 banks, each supporting up to 32 pins.
// ═══════════════════════════════════════════════════════════════════════════════
use crate::hal::mmio::{self, GPIO0_BASE};

/// GPIO bank stride (0x400 between banks).
const BANK_STRIDE: usize = 0x400;

// Register offsets within each GPIO bank (DesignWare GPIO)
const SWPORTA_DR:  usize = 0x00; // Port A data register (output level)
const SWPORTA_DDR: usize = 0x04; // Port A data direction (1=output, 0=input)
const INTEN:       usize = 0x30; // Interrupt enable
const INTMASK:     usize = 0x34; // Interrupt mask
const INTTYPE:     usize = 0x38; // Interrupt type (0=level, 1=edge)
const INTPOL:      usize = 0x3C; // Interrupt polarity (0=low/falling, 1=high/rising)
const INTSTATUS:   usize = 0x40; // Interrupt status (read)
const EOI:         usize = 0x4C; // End-of-interrupt (write-1-to-clear, edge only)
const EXT_PORTA:   usize = 0x50; // External port A (read pin level)

/// Bank base address.
#[inline]
fn bank_base(bank: usize) -> usize {
    GPIO0_BASE + bank * BANK_STRIDE
}

/// Configure a pin as GPIO output.
pub fn set_output(bank: usize, pin: u8) {
    let base = bank_base(bank);
    let bit = 1u32 << pin;

    // Set direction to output
    mmio::set_bits(base + SWPORTA_DDR, bit);

    // Disable interrupt on this pin
    mmio::clear_bits(base + INTEN, bit);
}

/// Configure a pin as GPIO input.
pub fn set_input(bank: usize, pin: u8) {
    let base = bank_base(bank);
    let bit = 1u32 << pin;

    // Set direction to input
    mmio::clear_bits(base + SWPORTA_DDR, bit);

    // Disable interrupt
    mmio::clear_bits(base + INTEN, bit);
}

/// Configure a pin as GPIO input with interrupt on falling edge.
pub fn set_input_irq_falling(bank: usize, pin: u8) {
    let base = bank_base(bank);
    let bit = 1u32 << pin;

    // Direction: input
    mmio::clear_bits(base + SWPORTA_DDR, bit);

    // Edge-triggered
    mmio::set_bits(base + INTTYPE, bit);

    // Polarity: falling edge (active-low → polarity bit = 0)
    mmio::clear_bits(base + INTPOL, bit);

    // Unmask
    mmio::clear_bits(base + INTMASK, bit);

    // Enable interrupt
    mmio::set_bits(base + INTEN, bit);

    // Clear any stale flags
    mmio::write32(base + EOI, bit);
}

/// Write a logic level to an output pin.
pub fn write_pin(bank: usize, pin: u8, high: bool) {
    let base = bank_base(bank);
    let bit = 1u32 << pin;

    if high {
        mmio::set_bits(base + SWPORTA_DR, bit);
    } else {
        mmio::clear_bits(base + SWPORTA_DR, bit);
    }
}

/// Read the current level of a pin.
pub fn read_pin(bank: usize, pin: u8) -> bool {
    let base = bank_base(bank);
    mmio::read32(base + EXT_PORTA) & (1u32 << pin) != 0
}

/// Clear the interrupt flag for a pin (edge-triggered).
pub fn clear_irq(bank: usize, pin: u8) {
    let base = bank_base(bank);
    mmio::write32(base + EOI, 1u32 << pin);
}

/// Read pending interrupt flags for a bank.
pub fn pending_irqs(bank: usize) -> u32 {
    mmio::read32(bank_base(bank) + INTSTATUS)
}

/// Configure a pin for an alternate function via IOMUX.
///
/// The RKNanoD uses CRU-based IOMUX registers.  `func` selects the mux option
/// (0 = GPIO, 1–3 = alternate functions).
pub fn set_function(bank: usize, pin: u8, func: u8) {
    // RKNanoD IOMUX is in the CRU region.  Each bank has a 32-bit mux register
    // with 2 bits per pin (4 functions per pin).
    let iomux_base = mmio::CRU_BASE + 0xC0 + bank * 4;
    let shift = (pin as u32) * 2;
    let mask = 0x3 << shift;
    let val = ((func as u32) & 0x3) << shift;
    mmio::modify_bits(iomux_base, mask, val);
}
