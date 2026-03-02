// ═══════════════════════════════════════════════════════════════════════════════
// display/framebuffer.rs — 16-bit RGB565 framebuffer with DMA-backed SPI flush
//
// Resolution: 170 × 320 pixels (1.99" IPS)
// Colour depth: RGB565 (16-bit) = 108,800 bytes per frame
// ═══════════════════════════════════════════════════════════════════════════════

use core::sync::atomic::Ordering;
use crate::hal::{dma, spi};

/// Display width in pixels.
pub const WIDTH: usize = 170;
/// Display height in pixels.
pub const HEIGHT: usize = 320;
/// Bytes per pixel (RGB565).
pub const BPP: usize = 2;
/// Total framebuffer size in bytes.
pub const FB_SIZE: usize = WIDTH * HEIGHT * BPP;

// ═════════════════════════════════════════════════════════════════════════════
// Framebuffer storage — placed in the DMA section (uncached) so SPI DMA can
// stream it without cache-flush overhead.
// ═════════════════════════════════════════════════════════════════════════════

#[link_section = ".dma_buffers"]
static mut FB_DATA: [u8; FB_SIZE] = [0u8; FB_SIZE];

/// Pixel-level access to the framebuffer (RGB565).
pub struct FrameBuffer;

impl FrameBuffer {
    /// Get a mutable byte slice of the entire framebuffer.
    pub fn as_mut_bytes() -> &'static mut [u8] {
        unsafe { &mut *(&raw mut FB_DATA) }
    }

    /// Get a read-only byte slice of the framebuffer.
    pub fn as_bytes() -> &'static [u8] {
        unsafe { &*(&raw const FB_DATA) }
    }

    /// Set a single pixel (x, y) to an RGB565 colour.
    #[inline]
    pub fn set_pixel(x: usize, y: usize, color: u16) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        let offset = (y * WIDTH + x) * BPP;
        let bytes = color.to_le_bytes();
        unsafe {
            FB_DATA[offset] = bytes[0];
            FB_DATA[offset + 1] = bytes[1];
        }
    }

    /// Read a single pixel.
    #[inline]
    pub fn get_pixel(x: usize, y: usize) -> u16 {
        if x >= WIDTH || y >= HEIGHT {
            return 0;
        }
        let offset = (y * WIDTH + x) * BPP;
        unsafe { u16::from_le_bytes([FB_DATA[offset], FB_DATA[offset + 1]]) }
    }

    /// Fill the entire framebuffer with a single colour.
    pub fn fill(color: u16) {
        let bytes = color.to_le_bytes();
        let fb = unsafe { &mut *(&raw mut FB_DATA) };
        for chunk in fb.chunks_exact_mut(2) {
            chunk[0] = bytes[0];
            chunk[1] = bytes[1];
        }
    }

    /// Fill a rectangular region.
    pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, color: u16) {
        for row in y..(y + h).min(HEIGHT) {
            for col in x..(x + w).min(WIDTH) {
                Self::set_pixel(col, row, color);
            }
        }
    }

    /// Flush the entire framebuffer to the LCD via DMA-backed SPI.
    /// This is non-blocking — returns immediately after starting DMA.
    pub fn flush_dma() {
        let src = (&raw const FB_DATA) as *const u8 as usize;
        let dst = spi::data_register_addr();

        dma::setup_single(
            dma::CH_LCD,
            src,
            dst,
            (FB_SIZE / 2) as u32, // 16-bit transfers
            dma::HS_SPI0_TX,
            dma::TransferWidth::HalfWord,
            dma::TransferWidth::HalfWord,
        );
    }

    /// Blocking flush: start DMA and wait for completion.
    pub fn flush_blocking() {
        Self::flush_dma();
        while !dma::LCD_DMA_DONE.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        dma::LCD_DMA_DONE.store(false, Ordering::Release);
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// embedded-graphics DrawTarget implementation
// ═════════════════════════════════════════════════════════════════════════════

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::Pixel;

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl DrawTarget for FrameBuffer {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            if coord.x >= 0 && coord.y >= 0 {
                let raw: RawU16 = color.into();
                Self::set_pixel(coord.x as usize, coord.y as usize, raw.into_inner());
            }
        }
        Ok(())
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// RGB565 colour helpers
// ═════════════════════════════════════════════════════════════════════════════

/// Convert 8-bit-per-channel RGB to RGB565.
#[inline]
pub const fn rgb888_to_565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | ((b as u16) >> 3)
}

// Predefined colours
pub const COLOR_BLACK: u16   = 0x0000;
pub const COLOR_WHITE: u16   = 0xFFFF;
pub const COLOR_RED: u16     = rgb888_to_565(255, 0, 0);
pub const COLOR_GREEN: u16   = rgb888_to_565(0, 255, 0);
pub const COLOR_BLUE: u16    = rgb888_to_565(0, 0, 255);
pub const COLOR_AMBER: u16   = rgb888_to_565(255, 191, 0);  // retro cassette amber
pub const COLOR_DARK_BG: u16 = rgb888_to_565(24, 24, 32);   // dark OS background
