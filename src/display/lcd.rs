// ═══════════════════════════════════════════════════════════════════════════════
// display/lcd.rs — LCD controller driver for the 1.99" IPS (ST7789V / similar)
//
// Communication: SPI (4-wire) with D/C (Data/Command) GPIO.
// Resolution: 170 × 320, RGB565.
// ═══════════════════════════════════════════════════════════════════════════════

use crate::hal::{spi, gpio};
use crate::display::framebuffer::{self, FrameBuffer, WIDTH, HEIGHT};

// ── LCD control pins (board-specific assignments) ───────────────────────────
const LCD_RST_BANK: usize = 1; // GPIO bank 1
const LCD_RST_PIN: u8     = 2;
const LCD_BL_BANK: usize  = 1; // Backlight enable
const LCD_BL_PIN: u8      = 3;

// ── ST7789V command set (common IPS controller) ─────────────────────────────
#[allow(dead_code)]
mod cmd {
    pub const SWRESET: u8  = 0x01; // Software reset
    pub const SLPOUT: u8   = 0x11; // Sleep out
    pub const NORON: u8    = 0x13; // Normal display on
    pub const INVON: u8    = 0x21; // Display inversion on (typical for IPS)
    pub const DISPON: u8   = 0x29; // Display on
    pub const CASET: u8    = 0x2A; // Column address set
    pub const RASET: u8    = 0x2B; // Row address set
    pub const RAMWR: u8    = 0x2C; // Memory write (start pixel data)
    pub const MADCTL: u8   = 0x36; // Memory data access control
    pub const COLMOD: u8   = 0x3A; // Color mode (pixel format)
    pub const FRMCTR1: u8  = 0xB1; // Frame rate control
    pub const FRMCTR2: u8  = 0xB2; // Frame rate control (idle)
    pub const PWCTRL1: u8  = 0xD0; // Power control 1
}

// ═════════════════════════════════════════════════════════════════════════════
// Initialisation
// ═════════════════════════════════════════════════════════════════════════════

/// Full LCD init sequence: reset, configure, turn on backlight.
pub fn init() {
    // ── Configure control GPIOs ─────────────────────────────────────────
    gpio::set_output(LCD_RST_BANK, LCD_RST_PIN);
    gpio::set_output(LCD_BL_BANK, LCD_BL_PIN);

    // ── Hardware reset ──────────────────────────────────────────────────
    gpio::write_pin(LCD_RST_BANK, LCD_RST_PIN, false);
    delay_ms(10);
    gpio::write_pin(LCD_RST_BANK, LCD_RST_PIN, true);
    delay_ms(120);

    // ── SPI init (8 MHz — well within ST7789 max of ~62 MHz) ────────────
    spi::init(3); // divider for ~8 MHz from pclk

    spi::cs_assert(spi::CsPin::Cs0);

    // ── Software reset ──────────────────────────────────────────────────
    send_cmd(cmd::SWRESET, &[]);
    delay_ms(150);

    // ── Wake from sleep ─────────────────────────────────────────────────
    send_cmd(cmd::SLPOUT, &[]);
    delay_ms(50);

    // ── Pixel format: 16-bit RGB565 ─────────────────────────────────────
    send_cmd(cmd::COLMOD, &[0x55]); // 16bpp

    // ── Memory data access control (rotation / mirror) ──────────────────
    // MX=0, MY=0, MV=0 → portrait 170×320
    send_cmd(cmd::MADCTL, &[0x00]);

    // ── Frame rate: ~60 Hz ──────────────────────────────────────────────
    send_cmd(cmd::FRMCTR1, &[0x01, 0x08, 0x01, 0x08]);

    // ── Display inversion on (needed for IPS panels) ────────────────────
    send_cmd(cmd::INVON, &[]);

    // ── Normal display mode on ──────────────────────────────────────────
    send_cmd(cmd::NORON, &[]);
    delay_ms(10);

    // ── Display on ──────────────────────────────────────────────────────
    send_cmd(cmd::DISPON, &[]);
    delay_ms(10);

    // ── Set drawing window to full screen ───────────────────────────────
    set_window(0, 0, WIDTH as u16 - 1, HEIGHT as u16 - 1);

    // ── Backlight on ────────────────────────────────────────────────────
    gpio::write_pin(LCD_BL_BANK, LCD_BL_PIN, true);

    // ── Clear screen ────────────────────────────────────────────────────
    FrameBuffer::fill(framebuffer::COLOR_DARK_BG);
    FrameBuffer::flush_blocking();
}

/// Set the active drawing window (column and row address range).
pub fn set_window(x0: u16, y0: u16, x1: u16, y1: u16) {
    send_cmd(cmd::CASET, &[
        (x0 >> 8) as u8, (x0 & 0xFF) as u8,
        (x1 >> 8) as u8, (x1 & 0xFF) as u8,
    ]);
    send_cmd(cmd::RASET, &[
        (y0 >> 8) as u8, (y0 & 0xFF) as u8,
        (y1 >> 8) as u8, (y1 & 0xFF) as u8,
    ]);
    // Prepare for RAMWR
    spi::lcd_write_cmd(cmd::RAMWR);
}

/// Flush the framebuffer to the LCD (called by the UI layer).
pub fn flush() {
    set_window(0, 0, WIDTH as u16 - 1, HEIGHT as u16 - 1);
    spi::lcd_write_cmd(cmd::RAMWR);
    FrameBuffer::flush_blocking();
}

// ═════════════════════════════════════════════════════════════════════════════
// Low-level helpers
// ═════════════════════════════════════════════════════════════════════════════

/// Send a command with optional data bytes.
fn send_cmd(cmd: u8, data: &[u8]) {
    spi::lcd_write_cmd(cmd);
    if !data.is_empty() {
        spi::write_data(data);
    }
}

/// Crude busy-loop delay (good enough for init sequences).
fn delay_ms(ms: u32) {
    // Assume ~96 MHz Cortex-M3 core clock → ~96,000 iterations per ms.
    let iterations = ms * 96_000;
    for _ in 0..iterations {
        core::hint::spin_loop();
    }
}
