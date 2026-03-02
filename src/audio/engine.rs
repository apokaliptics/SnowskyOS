// ═══════════════════════════════════════════════════════════════════════════════
// audio/engine.rs — DMA-driven, zero-copy audio engine (RKNanoD)
//
// The audio pipeline:
//   Decoder (FAT32/exFAT → PCM frames)
//     → CPU fills inactive half of DmaRingBuffer
//       → RKNanoD DMAC streams active half to I2S0 TX FIFO
//         → I2S → CS43131 DACs → headphone output
//
// Uses DmaLli (Linked-List Item) descriptors for hardware ping-pong.
// The digital bitstream is **never** manipulated in the digital domain
// — achieving brick-level bit-perfect output.
// ═══════════════════════════════════════════════════════════════════════════════

use core::sync::atomic::{AtomicBool, Ordering};
use crate::hal::{dma, i2s, interrupt};
use crate::mem::dma_buffer::{self, HALF_BUFFER_BYTES};

// ── Engine state ────────────────────────────────────────────────────────────

/// Set by the DMA-complete ISR; cleared by `refill_from_decoder()`.
static NEEDS_REFILL: AtomicBool = AtomicBool::new(false);

/// Set to `true` when playback is active.
static PLAYING: AtomicBool = AtomicBool::new(false);

// ── DMA Linked-List Items (ping-pong pair) ──────────────────────────────────
//
// Two LLIs that point at each other, forming an infinite ring.
// Placed in the .dma_buffers section for DMA-coherent access.

#[link_section = ".dma_buffers"]
static mut LLI_A: dma::DmaLli = dma::DmaLli {
    src_addr:  0,
    dst_addr:  0,
    next_lli:  0,
    control:   0,
    size:      0,
};

#[link_section = ".dma_buffers"]
static mut LLI_B: dma::DmaLli = dma::DmaLli {
    src_addr:  0,
    dst_addr:  0,
    next_lli:  0,
    control:   0,
    size:      0,
};

// ═════════════════════════════════════════════════════════════════════════════
// Initialisation
// ═════════════════════════════════════════════════════════════════════════════

/// Initialise the audio engine: configure I2S, DMA LLIs, and ISR.
pub fn init() {
    // ── I2S setup: 32-bit stereo, master mode ───────────────────────────
    let cfg = i2s::I2sConfig {
        sample_width: i2s::SampleWidth::Bits32,
        mclk_div: 1,  // MCLK divider (PLL output / mclk_div = MCLK)
        bclk_div: 4,  // MCLK / (2 * (4+1)) = MCLK/10 → BCLK
    };
    i2s::init(&cfg);
    i2s::flush_tx();

    // ── DMA controller global init ──────────────────────────────────────
    dma::init();

    // ── Build linked DMA LLIs (ping-pong) ───────────────────────────────
    let buf = unsafe { dma_buffer::audio_buffer() };
    let i2s_dr = i2s::data_register_addr() as u32;
    let transfer_count = (HALF_BUFFER_BYTES / 4) as u32; // 32-bit words

    // CTL word: src increment, dst fixed (FIFO), 32-bit width, int enable
    let ctl = build_ctl_word();

    unsafe {
        // LLI A → plays half-A, chains to B
        LLI_A.src_addr = buf.half_a_phys() as u32;
        LLI_A.dst_addr = i2s_dr;
        LLI_A.size     = transfer_count;
        LLI_A.control  = ctl;
        LLI_A.next_lli = &LLI_B as *const _ as u32;

        // LLI B → plays half-B, chains back to A
        LLI_B.src_addr = buf.half_b_phys() as u32;
        LLI_B.dst_addr = i2s_dr;
        LLI_B.size     = transfer_count;
        LLI_B.control  = ctl;
        LLI_B.next_lli = &LLI_A as *const _ as u32;
    }

    // ── Register the DMA IRQ handler ────────────────────────────────────
    interrupt::register(interrupt::IRQ_DMA, audio_dma_isr);
}

/// Start playback: kick the DMA linked-list ring and enable I2S TX.
pub fn start() {
    let lli_a_addr = unsafe { &LLI_A as *const _ as usize };
    dma::start_linked(dma::CH_AUDIO, lli_a_addr, dma::HS_I2S0_TX);
    i2s::start_tx();
    PLAYING.store(true, Ordering::Release);
}

/// Stop playback gracefully.
pub fn stop() {
    PLAYING.store(false, Ordering::Release);
    i2s::stop_tx();
    i2s::flush_tx();
}

/// Returns `true` when the main loop should call `refill_from_decoder()`.
pub fn needs_refill() -> bool {
    NEEDS_REFILL.load(Ordering::Acquire)
}

/// Fill the inactive buffer half with the next decoded PCM frames.
///
/// In a full implementation this would call the FAT32/exFAT file-system reader
/// and the audio decoder (FLAC/WAV/ALAC/DSD).  Here we provide the structural
/// skeleton — the actual decoder is plugged in via a trait object or function
/// pointer.
pub fn refill_from_decoder() {
    if !NEEDS_REFILL.load(Ordering::Acquire) {
        return;
    }

    let buf = unsafe { dma_buffer::audio_buffer() };
    let writable = buf.writable_half();

    // ── Decode next HALF_BUFFER_BYTES of PCM into `writable` ────────
    // For now: silence (zeros).  Replace with the real decoder call.
    decode_into(writable);

    // Clear the flag — buffer is now ready for DMA
    NEEDS_REFILL.store(false, Ordering::Release);
}

// ═════════════════════════════════════════════════════════════════════════════
// DMA-complete ISR (runs in interrupt context — must be fast)
// ═════════════════════════════════════════════════════════════════════════════

/// Called when the DMA finishes one half-buffer transfer.
fn audio_dma_isr() {
    // Acknowledge the DMA transfer-complete interrupt
    dma::irq_handler();

    // Swap the ring-buffer halves
    let buf = unsafe { dma_buffer::audio_buffer() };
    buf.swap();

    // Signal the main loop to fill the now-inactive half
    NEEDS_REFILL.store(true, Ordering::Release);
}

// ═════════════════════════════════════════════════════════════════════════════
// Decoder stub (replace with real codec)
// ═════════════════════════════════════════════════════════════════════════════

/// Placeholder: fill buffer with silence.  Real implementation reads from
/// the SD card via the file-system driver and decodes the audio format.
fn decode_into(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        *b = 0;
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Helpers
// ═════════════════════════════════════════════════════════════════════════════

/// Build the DMA CTL (control) word for an audio LLI.
///
/// RKNanoD DMAC CTL register encoding:
///   [2:0]   TT_FC    = 01 (memory-to-peripheral, DMAC flow control)
///   [6:4]   SRC_MSIZE = 010 (burst length 4)
///   [10:8]  DST_MSIZE = 010 (burst length 4)
///   [11]    SINC      = 1 (source address increment)
///   [12]    DINC      = 0 (dest address fixed — I2S FIFO)
///   [14:13] SRC_TR_WIDTH = 10 (32-bit)
///   [16:15] DST_TR_WIDTH = 10 (32-bit)
///   [18]    INT_EN    = 1 (interrupt on transfer complete)
///   [27]    LLP_SRC_EN = 1 (linked-list enabled for source)
///   [28]    LLP_DST_EN = 0 (no linked-list for dest — fixed FIFO addr)
fn build_ctl_word() -> u32 {
    let tt_fc        = 0b001;         // mem-to-periph, DMAC flow ctrl
    let src_msize    = 0b010 << 4;    // burst 4
    let dst_msize    = 0b010 << 8;    // burst 4
    let sinc         = 1 << 11;       // source increment
    let dinc         = 0 << 12;       // dest fixed
    let src_width    = 0b10 << 13;    // 32-bit source
    let dst_width    = 0b10 << 15;    // 32-bit dest
    let int_en       = 1 << 18;       // interrupt enable
    let llp_src_en   = 1 << 27;       // linked-list for source

    tt_fc | src_msize | dst_msize | sinc | dinc
        | src_width | dst_width | int_en | llp_src_en
}
