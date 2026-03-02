// ═══════════════════════════════════════════════════════════════════════════════
// sim/src/framebuffer.rs — Host-side RGB565 framebuffer
//
// Mirrors the firmware's display/framebuffer.rs but backed by a plain Vec
// instead of a DMA-mapped static.  Provides the same DrawTarget impl so that
// all embedded-graphics rendering is pixel-identical.
// ═══════════════════════════════════════════════════════════════════════════════

use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::Pixel;

/// Display width in pixels (matches the real 1.99″ IPS panel).
pub const WIDTH: usize = 170;
/// Display height in pixels.
pub const HEIGHT: usize = 320;
/// Bytes per pixel (RGB565).
pub const BPP: usize = 2;
/// Total framebuffer size in bytes.
pub const FB_SIZE: usize = WIDTH * HEIGHT * BPP;

/// Convert 8-bit-per-channel RGB to RGB565.
#[inline]
pub const fn rgb888_to_565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | ((b as u16) >> 3)
}

// Predefined colours (same as firmware)
#[allow(dead_code)]
pub const COLOR_BLACK: u16   = 0x0000;
#[allow(dead_code)]
pub const COLOR_WHITE: u16   = 0xFFFF;
#[allow(dead_code)]
pub const COLOR_AMBER: u16   = rgb888_to_565(255, 191, 0);
pub const COLOR_DARK_BG: u16 = rgb888_to_565(24, 24, 32);

/// Host-side framebuffer backed by heap memory.
pub struct FrameBuffer {
    pub data: Vec<u8>,
}

impl FrameBuffer {
    pub fn new() -> Self {
        Self {
            data: vec![0u8; FB_SIZE],
        }
    }

    /// Set a single pixel (x, y) to an RGB565 colour.
    #[inline]
    pub fn set_pixel(&mut self, x: usize, y: usize, color: u16) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        let offset = (y * WIDTH + x) * BPP;
        let bytes = color.to_le_bytes();
        self.data[offset] = bytes[0];
        self.data[offset + 1] = bytes[1];
    }

    /// Read a single pixel.
    #[inline]
    #[allow(dead_code)]
    pub fn get_pixel(&self, x: usize, y: usize) -> u16 {
        if x >= WIDTH || y >= HEIGHT {
            return 0;
        }
        let offset = (y * WIDTH + x) * BPP;
        u16::from_le_bytes([self.data[offset], self.data[offset + 1]])
    }

    /// Fill the entire framebuffer with a single colour.
    pub fn fill_color(&mut self, color: u16) {
        let bytes = color.to_le_bytes();
        for chunk in self.data.chunks_exact_mut(2) {
            chunk[0] = bytes[0];
            chunk[1] = bytes[1];
        }
    }

    /// Convert the RGB565 framebuffer to a u32 ARGB buffer for minifb.
    pub fn to_argb32(&self) -> Vec<u32> {
        let mut buf = Vec::with_capacity(WIDTH * HEIGHT);
        for chunk in self.data.chunks_exact(2) {
            let rgb565 = u16::from_le_bytes([chunk[0], chunk[1]]);
            let r = ((rgb565 >> 11) & 0x1F) as u32;
            let g = ((rgb565 >> 5) & 0x3F) as u32;
            let b = (rgb565 & 0x1F) as u32;
            // Expand to 8-bit per channel
            let r8 = (r << 3) | (r >> 2);
            let g8 = (g << 2) | (g >> 4);
            let b8 = (b << 3) | (b >> 2);
            buf.push(0xFF00_0000 | (r8 << 16) | (g8 << 8) | b8);
        }
        buf
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// embedded-graphics DrawTarget — identical behaviour to the firmware impl
// ═════════════════════════════════════════════════════════════════════════════

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
                self.set_pixel(coord.x as usize, coord.y as usize, raw.into_inner());
            }
        }
        Ok(())
    }
}
