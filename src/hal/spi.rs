// ═══════════════════════════════════════════════════════════════════════════════
// hal/spi.rs — SPI master driver for Rockchip RKNanoD
// Used to drive the 1.99" IPS LCD (170×320) via 8080-style SPI.
// ═══════════════════════════════════════════════════════════════════════════════
use crate::hal::mmio::{self, SPI0_BASE};

// ── SPI register offsets (RKNanoD SPI master) ───────────────────────────────
const SPI_CTRLR0: usize = SPI0_BASE + 0x00; // Control register 0
const SPI_CTRLR1: usize = SPI0_BASE + 0x04; // Control register 1 (slave only)
const SPI_ENR:    usize = SPI0_BASE + 0x08; // Enable register
const SPI_SER:    usize = SPI0_BASE + 0x0C; // Slave enable (chip select)
const SPI_BAUDR:  usize = SPI0_BASE + 0x10; // Baud rate divisor
const SPI_TXFTLR: usize = SPI0_BASE + 0x14; // TX FIFO threshold
const SPI_RXFTLR: usize = SPI0_BASE + 0x18; // RX FIFO threshold
const SPI_TXFLR:  usize = SPI0_BASE + 0x1C; // TX FIFO level
const SPI_RXFLR:  usize = SPI0_BASE + 0x20; // RX FIFO level
const SPI_SR:     usize = SPI0_BASE + 0x24; // Status register
const SPI_DR:     usize = SPI0_BASE + 0x60; // Data register (FIFO)

// ── Status bits ─────────────────────────────────────────────────────────────
const SR_TFE: u32   = 1 << 2;  // TX FIFO empty
const SR_BUSY: u32  = 1 << 0;  // Transfer in progress
const SR_TFNF: u32  = 1 << 1;  // TX FIFO not full

/// SPI chip select lines.
#[derive(Clone, Copy)]
pub enum CsPin {
    Cs0 = 0,
}

/// Initialise SPI0 in master mode (mode 0: CPOL=0, CPHA=0, 8-bit frame).
pub fn init(divider: u32) {
    // Disable SPI first
    mmio::write32(SPI_ENR, 0);

    // CTRLR0: SPI mode 0, 8-bit frame, transmit-only
    let ctrlr0 = (0x07 << 0)   // DFS: 8-bit frame (7 = 8-1)
               | (0 << 6)      // SCPH = 0 (CPHA=0)
               | (0 << 7)      // SCPOL = 0 (CPOL=0)
               | (0 << 8);     // TMOD: TX & RX
    mmio::write32(SPI_CTRLR0, ctrlr0);

    // Baud rate (SPI_CLK = APB_CLK / divider)
    mmio::write32(SPI_BAUDR, divider);

    // TX FIFO threshold
    mmio::write32(SPI_TXFTLR, 0);

    // Enable SPI
    mmio::write32(SPI_ENR, 1);
}

/// Assert chip-select (active low via GPIO — we control it manually).
pub fn cs_assert(_pin: CsPin) {
    crate::hal::gpio::write_pin(gpio_cs_bank(), gpio_cs_pin(), false);
}

/// De-assert chip-select.
pub fn cs_deassert(_pin: CsPin) {
    crate::hal::gpio::write_pin(gpio_cs_bank(), gpio_cs_pin(), true);
}

/// Blocking: send a single byte.
pub fn write_byte(b: u8) {
    // Wait for TX FIFO not full
    while mmio::read32(SPI_SR) & SR_TFNF == 0 {
        core::hint::spin_loop();
    }
    mmio::write32(SPI_DR, b as u32);
    // Wait until TX FIFO drains
    while mmio::read32(SPI_SR) & SR_TFE == 0 {
        core::hint::spin_loop();
    }
    while mmio::read32(SPI_SR) & SR_BUSY != 0 {
        core::hint::spin_loop();
    }
}

/// Blocking: send a command byte (D/C=0) then data bytes (D/C=1) for
/// LCD 8080-style SPI interface.
pub fn lcd_write_cmd(cmd: u8) {
    crate::hal::gpio::write_pin(gpio_dc_bank(), gpio_dc_pin(), false); // D/C = command
    write_byte(cmd);
    crate::hal::gpio::write_pin(gpio_dc_bank(), gpio_dc_pin(), true);  // D/C = data
}

/// Blocking: send a slice of data bytes.
pub fn write_data(data: &[u8]) {
    for &b in data {
        write_byte(b);
    }
}

/// Physical address of the SPI data register (for DMA transfers).
pub const fn data_register_addr() -> usize {
    SPI_DR
}

// ── GPIO pin assignments for LCD control signals (RKNanoD board) ────────────
fn gpio_cs_bank() -> usize { 0 }   // GPIO bank 0
fn gpio_cs_pin() -> u8 { 20 }
fn gpio_dc_bank() -> usize { 0 }   // GPIO bank 0
fn gpio_dc_pin() -> u8 { 21 }
