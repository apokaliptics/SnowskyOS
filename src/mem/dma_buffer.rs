// ═══════════════════════════════════════════════════════════════════════════════
// mem/dma_buffer.rs — Zero-copy DMA ring buffer for the audio pipeline
// ═══════════════════════════════════════════════════════════════════════════════
//! Double-buffer / ring-buffer placed in the `.dma_buffers` linker section
//! (uncached SRAM region on the RKNanoD) so that the DMA controller can read
//! it without cache-coherency issues.
//!
//! The audio engine fills one half while the DMA drains the other — true
//! zero-copy, bit-perfect streaming.

use core::sync::atomic::{AtomicU8, Ordering};

/// Number of audio frames per half-buffer.
/// 1024 frames × 2 channels × 4 bytes (32-bit) = 8 KB per half.
pub const FRAMES_PER_HALF: usize = 1024;

/// Total size of one half-buffer in bytes (stereo, 32-bit samples).
pub const HALF_BUFFER_BYTES: usize = FRAMES_PER_HALF * 2 * 4;

/// Double-buffer state: which half the DMA is currently reading from.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HalfSelect {
    First  = 0,
    Second = 1,
}

// ═════════════════════════════════════════════════════════════════════════════
// DmaRingBuffer — placed in the `.dma_buffers` linker section (uncached)
// ═════════════════════════════════════════════════════════════════════════════

/// A ping-pong (double) DMA buffer for bit-perfect audio streaming.
///
/// Memory layout:
/// ```text
/// [ ---- Half A (8 KB) ---- | ---- Half B (8 KB) ---- ]
/// ```
/// The DMA reads from one half while the CPU writes next samples into the other.
#[repr(C, align(32))]
pub struct DmaRingBuffer {
    /// Raw sample data — two halves back-to-back.
    data: [u8; HALF_BUFFER_BYTES * 2],
    /// Which half the DMA is currently consuming.
    active_half: AtomicU8,
}

/// Place the singleton in the uncached DMA section.
#[link_section = ".dma_buffers"]
static mut DMA_AUDIO_BUF: DmaRingBuffer = DmaRingBuffer::new();

impl DmaRingBuffer {
    pub const fn new() -> Self {
        Self {
            data: [0u8; HALF_BUFFER_BYTES * 2],
            active_half: AtomicU8::new(0),
        }
    }

    /// Get a mutable slice to the half that the CPU should fill next (the
    /// *inactive* half — the one NOT being read by DMA).
    pub fn writable_half(&mut self) -> &mut [u8] {
        let active = self.active_half.load(Ordering::Acquire);
        let offset = if active == 0 {
            HALF_BUFFER_BYTES // DMA is on half 0 → CPU writes to half 1
        } else {
            0                 // DMA is on half 1 → CPU writes to half 0
        };
        &mut self.data[offset..offset + HALF_BUFFER_BYTES]
    }

    /// Get a read-only slice to the half being consumed by DMA (for debug).
    pub fn active_half_slice(&self) -> &[u8] {
        let active = self.active_half.load(Ordering::Acquire);
        let offset = (active as usize) * HALF_BUFFER_BYTES;
        &self.data[offset..offset + HALF_BUFFER_BYTES]
    }

    /// Swap the active half (called from the DMA-complete ISR).
    pub fn swap(&self) {
        let prev = self.active_half.load(Ordering::Acquire);
        let next = if prev == 0 { 1 } else { 0 };
        self.active_half.store(next, Ordering::Release);
    }

    /// Physical base address of half A (for DMA descriptor setup).
    pub fn half_a_phys(&self) -> usize {
        self.data.as_ptr() as usize
    }

    /// Physical base address of half B.
    pub fn half_b_phys(&self) -> usize {
        self.data.as_ptr() as usize + HALF_BUFFER_BYTES
    }

    /// Which half is the DMA currently consuming?
    pub fn current_active(&self) -> HalfSelect {
        if self.active_half.load(Ordering::Acquire) == 0 {
            HalfSelect::First
        } else {
            HalfSelect::Second
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Public accessors for the global DMA buffer singleton
// ═════════════════════════════════════════════════════════════════════════════

/// Get a reference to the global audio DMA buffer.
///
/// # Safety
/// The caller must ensure exclusive mutable access is properly synchronised
/// (only one writer at a time — guaranteed by the double-buffer protocol).
pub unsafe fn audio_buffer() -> &'static mut DmaRingBuffer {
    &mut *(&raw mut DMA_AUDIO_BUF)
}

/// Physical address of the entire buffer (for DMA descriptor `src_addr`).
pub fn audio_buffer_phys() -> usize {
    (&raw const DMA_AUDIO_BUF) as *const u8 as usize
}
