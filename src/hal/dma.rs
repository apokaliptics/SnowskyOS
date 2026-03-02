// ═══════════════════════════════════════════════════════════════════════════════
// hal/dma.rs — DMA Controller (DMAC) driver for Rockchip RKNanoD
//
// Provides zero-copy transfers from memory double-buffers to I2S / SPI.
// The RKNanoD DMAC supports linked-list descriptors and per-channel interrupts.
//
// Double-buffer scheme:
//   CPU fills Buffer A while DMA streams Buffer B to I2S (then swap).
// ═══════════════════════════════════════════════════════════════════════════════
#![allow(dead_code)]
use crate::hal::mmio::{self, DMA_BASE};
use core::sync::atomic::{AtomicBool, Ordering};

// ── DMAC global registers ───────────────────────────────────────────────────
const DMAC_INTST:    usize = DMA_BASE + 0x00; // DMA interrupt status (combined)
const DMAC_INTTC_ST: usize = DMA_BASE + 0x04; // Transfer-complete interrupt status
const DMAC_INTTC_CL: usize = DMA_BASE + 0x08; // Transfer-complete clear
const DMAC_INTERR_ST:usize = DMA_BASE + 0x0C; // Error interrupt status
const DMAC_INTERR_CL:usize = DMA_BASE + 0x10; // Error interrupt clear
const DMAC_ENBLD_CH: usize = DMA_BASE + 0x1C; // Enabled channels
const DMAC_CFG:      usize = DMA_BASE + 0x30; // DMAC configuration
const DMAC_SYNC:     usize = DMA_BASE + 0x34; // Sync logic

// ── Per-channel register offset (stride = 0x20, 8 channels max) ─────────────
const CH_STRIDE: usize = 0x20;
const CH_BASE_OFFSET: usize = 0x100; // Channel 0 starts at DMA_BASE + 0x100

// Channel register offsets from channel base
const CH_SAR:  usize = 0x00; // Source address
const CH_DAR:  usize = 0x04; // Destination address
const CH_LLP:  usize = 0x08; // Linked-list pointer (next descriptor)
const CH_CTL:  usize = 0x0C; // Control register (low 32-bit)
const CH_CTLH: usize = 0x10; // Control register (high 32-bit: transfer size)
const CH_CFG:  usize = 0x14; // Configuration register (low)
const CH_CFGH: usize = 0x18; // Configuration register (high: handshake)

// ── CTL register bits ───────────────────────────────────────────────────────
const CTL_INT_EN: u32      = 1 << 0;  // Interrupt enable
const CTL_SRC_TR_WIDTH_8: u32  = 0 << 4;
const CTL_SRC_TR_WIDTH_16: u32 = 1 << 4;
const CTL_SRC_TR_WIDTH_32: u32 = 2 << 4;
const CTL_DST_TR_WIDTH_8: u32  = 0 << 1;
const CTL_DST_TR_WIDTH_16: u32 = 1 << 1;
const CTL_DST_TR_WIDTH_32: u32 = 2 << 1;
const CTL_SINC:  u32       = 0 << 9;  // Source increment: 00 = increment
const CTL_SINC_NOINC: u32  = 2 << 9;  // Source no-change
const CTL_DINC:  u32       = 0 << 7;  // Dest increment: 00 = increment
const CTL_DINC_NOINC: u32  = 2 << 7;  // Dest no-change (peripheral FIFO)
const CTL_TT_FC_M2P: u32  = 1 << 20; // Transfer type: memory-to-peripheral, DMA flow ctrl
const CTL_LLP_DST_EN: u32 = 1 << 27; // Enable linked-list for dest
const CTL_LLP_SRC_EN: u32 = 1 << 28; // Enable linked-list for source

// ── CFG register bits ───────────────────────────────────────────────────────
const CFG_CH_EN: u32       = 1 << 0;  // Channel enable
const CFG_CH_SUSP: u32    = 1 << 8;  // Channel suspend

// ── DMA handshake interface IDs (RKNanoD specific) ──────────────────────────
pub const HS_I2S0_TX: u32 = 0;  // I2S0 transmit handshake
pub const HS_SPI0_TX: u32 = 2;  // SPI0 transmit handshake
pub const HS_I2S1_TX: u32 = 4;  // I2S1 transmit handshake

/// Channel assignment: audio on ch0 (highest HW priority).
pub const CH_AUDIO: usize = 0;
/// LCD framebuffer DMA on ch1.
pub const CH_LCD: usize   = 1;

// ── Completion flags (set in ISR, polled in main loop) ──────────────────────
pub static AUDIO_DMA_DONE: AtomicBool = AtomicBool::new(false);
pub static LCD_DMA_DONE: AtomicBool   = AtomicBool::new(false);

/// Base address for a given DMA channel's registers.
#[inline]
const fn ch_base(ch: usize) -> usize {
    DMA_BASE + CH_BASE_OFFSET + ch * CH_STRIDE
}

// ═════════════════════════════════════════════════════════════════════════════
// DMA Linked-List Item (LLI) — for chained/ping-pong transfers
// ═════════════════════════════════════════════════════════════════════════════

/// A DMA linked-list item for chained transfers (ping-pong double buffer).
///
/// The DMAC reads these from memory to set up the next transfer automatically.
#[repr(C, align(16))]
pub struct DmaLli {
    pub src_addr: u32,   // Source address
    pub dst_addr: u32,   // Destination address
    pub next_lli: u32,   // Pointer to next LLI (0 = end of chain)
    pub control:  u32,   // CTL register value for this transfer
    pub size:     u32,   // Transfer size (number of source-width units)
}

// ═════════════════════════════════════════════════════════════════════════════
// Public API
// ═════════════════════════════════════════════════════════════════════════════

/// Enable the DMAC globally.
pub fn init() {
    // Enable the DMA controller
    mmio::write32(DMAC_CFG, 1); // DMAC_EN

    // Clear all pending interrupts
    mmio::write32(DMAC_INTTC_CL, 0xFF);
    mmio::write32(DMAC_INTERR_CL, 0xFF);
}

/// Configure a channel for a single (non-linked-list) transfer.
pub fn setup_single(
    ch: usize,
    src: usize,
    dst: usize,
    count: u32,
    handshake: u32,
    src_width: TransferWidth,
    dst_width: TransferWidth,
) {
    let base = ch_base(ch);

    // Source / dest
    mmio::write32(base + CH_SAR, src as u32);
    mmio::write32(base + CH_DAR, dst as u32);
    mmio::write32(base + CH_LLP, 0); // No linked list

    // Control: M2P, source increment, dest fixed, interrupt enable
    let ctl = CTL_INT_EN
        | (src_width as u32) << 4   // SRC_TR_WIDTH
        | (dst_width as u32) << 1   // DST_TR_WIDTH
        | CTL_SINC                   // Source increment
        | CTL_DINC_NOINC             // Dest fixed (peripheral)
        | CTL_TT_FC_M2P;            // Memory → Peripheral
    mmio::write32(base + CH_CTL, ctl);

    // Transfer size (high word)
    mmio::write32(base + CH_CTLH, count);

    // Config: handshake interface, enable
    let cfg_h = (handshake & 0xF) << 7; // DST_PER: destination handshake
    mmio::write32(base + CH_CFGH, cfg_h);
    mmio::write32(base + CH_CFG, CFG_CH_EN);
}

/// Start a linked-list transfer on a channel (for audio ping-pong).
pub fn start_linked(ch: usize, first_lli: usize, handshake: u32) {
    let base = ch_base(ch);

    // Point to first LLI
    mmio::write32(base + CH_LLP, first_lli as u32);

    // Control: linked-list enabled (src + dst), M2P, increment src, fixed dst
    let ctl = CTL_INT_EN
        | CTL_SRC_TR_WIDTH_32
        | CTL_DST_TR_WIDTH_32
        | CTL_SINC
        | CTL_DINC_NOINC
        | CTL_TT_FC_M2P
        | CTL_LLP_SRC_EN;
    mmio::write32(base + CH_CTL, ctl);

    // Config: handshake, enable
    let cfg_h = (handshake & 0xF) << 7;
    mmio::write32(base + CH_CFGH, cfg_h);
    mmio::write32(base + CH_CFG, CFG_CH_EN);
}

/// DMA IRQ handler — called from the interrupt dispatcher.
///
/// Checks transfer-complete status and sets the appropriate completion flag.
pub fn irq_handler() {
    let tc_status = mmio::read32(DMAC_INTTC_ST);

    if tc_status & (1 << CH_AUDIO) != 0 {
        mmio::write32(DMAC_INTTC_CL, 1 << CH_AUDIO);
        AUDIO_DMA_DONE.store(true, Ordering::Release);
    }

    if tc_status & (1 << CH_LCD) != 0 {
        mmio::write32(DMAC_INTTC_CL, 1 << CH_LCD);
        LCD_DMA_DONE.store(true, Ordering::Release);
    }

    // Clear any error flags
    let err_status = mmio::read32(DMAC_INTERR_ST);
    if err_status != 0 {
        mmio::write32(DMAC_INTERR_CL, err_status);
    }
}

/// Supported DMA transfer widths.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum TransferWidth {
    Byte     = 0,
    HalfWord = 1, // 16-bit
    Word     = 2, // 32-bit
}
