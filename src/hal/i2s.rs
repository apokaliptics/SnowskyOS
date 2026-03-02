// ═══════════════════════════════════════════════════════════════════════════════
// hal/i2s.rs — I2S master controller driver for Rockchip RKNanoD
//
// Handles PCM and DSD bitstream output to the dual CS43131 DACs.
// Configured for 32-bit width for bit-perfect transparency even with 16-bit
// source material (zero-padded in hardware — no digital manipulation).
// ═══════════════════════════════════════════════════════════════════════════════
#![allow(dead_code)]
use crate::hal::mmio::{self, I2S0_BASE};

// ── I2S register offsets (RKNanoD I2S master) ───────────────────────────────
const I2S_TXCR:    usize = I2S0_BASE + 0x00; // Transmit control register
const I2S_RXCR:    usize = I2S0_BASE + 0x04; // Receive control register
const I2S_CKR:     usize = I2S0_BASE + 0x08; // Clock generation register
const I2S_FIFOLR:  usize = I2S0_BASE + 0x0C; // FIFO level register
const I2S_DMACR:   usize = I2S0_BASE + 0x10; // DMA control register
const I2S_INTCR:   usize = I2S0_BASE + 0x14; // Interrupt control register
const I2S_INTSR:   usize = I2S0_BASE + 0x18; // Interrupt status register
const I2S_XFER:    usize = I2S0_BASE + 0x1C; // Transfer start register
const I2S_CLR:     usize = I2S0_BASE + 0x20; // FIFO clear register
const I2S_TXDR:    usize = I2S0_BASE + 0x24; // Transmit data register (FIFO)
const I2S_RXDR:    usize = I2S0_BASE + 0x28; // Receive data register

// ── TXCR bits ───────────────────────────────────────────────────────────────
const TXCR_VDW_MASK: u32   = 0x1F;       // Valid data width [4:0]
const TXCR_FMT_I2S: u32    = 0 << 5;     // I2S standard format
const TXCR_FMT_LJ: u32     = 1 << 5;     // Left-justified format
const TXCR_PBM_STEREO: u32 = 0 << 7;     // Stereo playback
const TXCR_CSR_2CH: u32    = 0 << 15;    // 2-channel
const TXCR_HWT: u32        = 0 << 14;    // Half-word transfer = off

// ── CKR bits ────────────────────────────────────────────────────────────────
const CKR_MSS_MASTER: u32  = 0 << 27;    // Master mode (generates BCLK/LRCK)
const CKR_CKP: u32         = 0 << 26;    // Clock polarity: normal
const CKR_MDIV_SHIFT: u32  = 16;         // MCLK divider [23:16]
const CKR_TSD_SHIFT: u32   = 8;          // TX serial data divider [15:8]
const CKR_RSD_SHIFT: u32   = 0;          // RX serial data divider [7:0]

// ── DMACR bits ──────────────────────────────────────────────────────────────
const DMACR_TDE: u32 = 1 << 0;   // TX DMA enable
const DMACR_TDL_SHIFT: u32 = 4;  // TX DMA watermark level [8:4]

// ── XFER bits ───────────────────────────────────────────────────────────────
const XFER_TXS: u32 = 1 << 0;    // TX start
const XFER_RXS: u32 = 1 << 1;    // RX start

// ── CLR bits ────────────────────────────────────────────────────────────────
const CLR_TXC: u32 = 1 << 0;     // Clear TX FIFO
const CLR_RXC: u32 = 1 << 1;     // Clear RX FIFO

/// Supported sample widths for the I2S transfer.
/// NOTE: We always use Bits32 for bit-perfect transparency.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum SampleWidth {
    Bits16 = 15,  // VDW = 15 (16-1)
    Bits24 = 23,  // VDW = 23 (24-1)
    Bits32 = 31,  // VDW = 31 (32-1)
}

/// I2S configuration passed to `init`.
pub struct I2sConfig {
    pub sample_width: SampleWidth,
    /// MCLK divider: MCLK_out = MCLK_in / (mdiv + 1)
    pub mclk_div: u8,
    /// BCLK = MCLK / (2 * (bclk_div + 1))
    pub bclk_div: u8,
}

/// Initialise the I2S peripheral in master / I2S-standard mode.
///
/// The I2S is **always** configured for 32-bit transfers to preserve bit-perfect
/// audio.  Even 16-bit PCM is zero-extended to 32-bit at the I2S frame level.
pub fn init(cfg: &I2sConfig) {
    // ── Stop any ongoing transfer ───────────────────────────────────────
    mmio::write32(I2S_XFER, 0);

    // ── Clear FIFOs ─────────────────────────────────────────────────────
    mmio::write32(I2S_CLR, CLR_TXC | CLR_RXC);
    for _ in 0..100 {
        core::hint::spin_loop();
    }

    // ── TX control: I2S format, 32-bit, stereo, 2-chan ──────────────────
    let txcr = (cfg.sample_width as u32 & TXCR_VDW_MASK)
             | TXCR_FMT_I2S
             | TXCR_PBM_STEREO
             | TXCR_CSR_2CH;
    mmio::write32(I2S_TXCR, txcr);

    // ── Clock: master mode, dividers ────────────────────────────────────
    let ckr = CKR_MSS_MASTER
            | CKR_CKP
            | ((cfg.mclk_div as u32) << CKR_MDIV_SHIFT)
            | ((cfg.bclk_div as u32) << CKR_TSD_SHIFT)
            | ((cfg.bclk_div as u32) << CKR_RSD_SHIFT);
    mmio::write32(I2S_CKR, ckr);

    // ── DMA: enable TX DMA, watermark at half-FIFO (8 entries) ──────────
    let dmacr = DMACR_TDE | (8 << DMACR_TDL_SHIFT);
    mmio::write32(I2S_DMACR, dmacr);
}

/// Returns the physical address of the I2S TX data register (for DMA target).
#[inline]
pub const fn data_register_addr() -> usize {
    I2S_TXDR
}

/// Flush the TX FIFO.
pub fn flush_tx() {
    mmio::write32(I2S_CLR, CLR_TXC);
    for _ in 0..64 {
        core::hint::spin_loop();
    }
}

/// Start I2S transmission.
pub fn start_tx() {
    mmio::set_bits(I2S_XFER, XFER_TXS);
}

/// Stop I2S transmission.
pub fn stop_tx() {
    mmio::clear_bits(I2S_XFER, XFER_TXS);
}

/// Enable the I2S TX DMA request line.
pub fn enable_dma() {
    mmio::set_bits(I2S_DMACR, DMACR_TDE);
}

/// Disable the I2S TX DMA request line.
pub fn disable_dma() {
    mmio::clear_bits(I2S_DMACR, DMACR_TDE);
}

/// Return true if the TX FIFO is empty.
pub fn tx_fifo_empty() -> bool {
    let level = mmio::read32(I2S_FIFOLR) & 0x3F; // TX level in [5:0]
    level == 0
}
