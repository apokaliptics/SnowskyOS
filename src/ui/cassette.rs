// ═══════════════════════════════════════════════════════════════════════════════
// ui/cassette.rs — Retro cassette-style UI for the Echo Mini
//
// Uses embedded-graphics primitives to render a cassette tape aesthetic:
//   • Dark background with amber/warm tones
//   • Cassette reels (spinning circles) during playback
//   • Track info, progress bar, volume indicator
//   • Minimal CPU usage — only redraws changed regions (dirty rects)
//
// Design goal: the UI must NEVER cause audio jitter.  All drawing is done
// in the main loop between poll cycles; the DMA-backed framebuffer flush
// is non-blocking.
// ═══════════════════════════════════════════════════════════════════════════════

use crate::display::framebuffer::{self, FrameBuffer, WIDTH};
use crate::display::lcd;
use crate::input::buttons::ButtonEvent;
use crate::audio::engine;

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{
    Circle, PrimitiveStyle, Rectangle, Line,
};
use embedded_graphics::mono_font::{ascii::FONT_6X10, MonoTextStyle};
use embedded_graphics::text::Text;

// ── Colour palette (warm retro cassette tones) ──────────────────────────────
use embedded_graphics::pixelcolor::Rgb565;

#[allow(dead_code)]
const BG: Rgb565        = Rgb565::new(3, 3, 4);       // near-black
const AMBER: Rgb565     = Rgb565::new(31, 23, 0);     // warm amber
const DIM_AMBER: Rgb565 = Rgb565::new(16, 12, 0);     // dimmed amber
const REEL_COLOR: Rgb565 = Rgb565::new(20, 16, 4);    // cassette reel brown
#[allow(dead_code)]
const WHITE: Rgb565     = Rgb565::new(31, 63, 31);    // bright white
const DARK_GRAY: Rgb565 = Rgb565::new(8, 16, 8);      // subtle separator

// ── Layout constants (170×320 portrait) ─────────────────────────────────────
const MARGIN: i32 = 8;
const REEL_Y: i32 = 60;
const REEL_RADIUS: u32 = 30;
const REEL_LEFT_X: i32 = 42;
const REEL_RIGHT_X: i32 = 128;
const TITLE_Y: i32 = 140;
const ARTIST_Y: i32 = 158;
const PROGRESS_Y: i32 = 190;
const PROGRESS_H: u32 = 4;
#[allow(dead_code)]
const TIME_Y: i32 = 204;
const STATUS_Y: i32 = 240;
const VOLUME_Y: i32 = 270;

// ── Playback state (owned by the UI — no heap allocation) ───────────────────
static mut UI_STATE: UiState = UiState::new();

struct UiState {
    playing: bool,
    volume: u8,          // 0–100
    progress: u16,       // 0–1000 (‰)
    reel_angle: u16,     // 0–359 for animation
    track_title: [u8; 32],
    track_artist: [u8; 32],
    title_len: usize,
    artist_len: usize,
    dirty: bool,
}

impl UiState {
    const fn new() -> Self {
        Self {
            playing: false,
            volume: 80,
            progress: 0,
            reel_angle: 0,
            track_title: [0u8; 32],
            track_artist: [0u8; 32],
            title_len: 0,
            artist_len: 0,
            dirty: true,
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Public API
// ═════════════════════════════════════════════════════════════════════════════

/// Draw the initial idle (stopped) screen.
pub fn draw_idle_screen() {
    let mut fb = FrameBuffer;

    // Clear to dark background
    FrameBuffer::fill(framebuffer::COLOR_DARK_BG);

    // ── Cassette shell outline ──────────────────────────────────────────
    draw_cassette_shell(&mut fb);

    // ── Title: "Echo Mini OS" ───────────────────────────────────────────
    let style = MonoTextStyle::new(&FONT_6X10, AMBER);
    let _ = Text::new("Echo Mini OS", Point::new(34, 24), style).draw(&mut fb);

    // ── Subtitle ────────────────────────────────────────────────────────
    let dim_style = MonoTextStyle::new(&FONT_6X10, DIM_AMBER);
    let _ = Text::new("Bit-Perfect Audio", Point::new(22, 38), dim_style).draw(&mut fb);

    // ── Cassette reels (static) ─────────────────────────────────────────
    draw_reel(&mut fb, REEL_LEFT_X, REEL_Y, 0);
    draw_reel(&mut fb, REEL_RIGHT_X, REEL_Y, 0);

    // ── "NO TRACK" placeholder ──────────────────────────────────────────
    let _ = Text::new("-- No Track --", Point::new(24, TITLE_Y), dim_style).draw(&mut fb);

    // ── Progress bar (empty) ────────────────────────────────────────────
    draw_progress_bar(&mut fb, 0);

    // ── Status line ─────────────────────────────────────────────────────
    let _ = Text::new("[STOP]", Point::new(56, STATUS_Y), style).draw(&mut fb);

    // ── Volume bar ──────────────────────────────────────────────────────
    draw_volume_bar(&mut fb, 80);

    // ── Flush ───────────────────────────────────────────────────────────
    lcd::flush();
}

/// Handle a button press and update the UI.
pub fn handle_input(event: ButtonEvent) {
    let state = unsafe { &mut *(&raw mut UI_STATE) };

    match event {
        ButtonEvent::Play => {
            state.playing = !state.playing;
            if state.playing {
                engine::start();
            } else {
                engine::stop();
            }
            state.dirty = true;
        }
        ButtonEvent::Next => {
            // TODO: advance to next track in playlist
            state.progress = 0;
            state.dirty = true;
        }
        ButtonEvent::Prev => {
            // TODO: go to previous track / restart
            state.progress = 0;
            state.dirty = true;
        }
        ButtonEvent::VolUp => {
            if state.volume < 100 {
                state.volume += 2;
            }
            state.dirty = true;
        }
        ButtonEvent::VolDown => {
            if state.volume >= 2 {
                state.volume -= 2;
            }
            state.dirty = true;
        }
        ButtonEvent::None => {}
    }

    if state.dirty {
        redraw();
        state.dirty = false;
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Internal drawing functions
// ═════════════════════════════════════════════════════════════════════════════

/// Full redraw (called only when `dirty` — minimises CPU usage).
fn redraw() {
    let mut fb = FrameBuffer;
    let state = unsafe { &*(&raw const UI_STATE) };

    FrameBuffer::fill(framebuffer::COLOR_DARK_BG);

    draw_cassette_shell(&mut fb);

    // ── Header ──────────────────────────────────────────────────────────
    let style = MonoTextStyle::new(&FONT_6X10, AMBER);
    let dim_style = MonoTextStyle::new(&FONT_6X10, DIM_AMBER);

    let _ = Text::new("Echo Mini OS", Point::new(34, 24), style).draw(&mut fb);
    let _ = Text::new("Bit-Perfect Audio", Point::new(22, 38), dim_style).draw(&mut fb);

    // ── Reels (animated angle if playing) ───────────────────────────────
    let angle = if state.playing {
        unsafe {
            let p = &raw mut UI_STATE;
            (*p).reel_angle = ((*p).reel_angle + 15) % 360;
            (*p).reel_angle
        }
    } else {
        0
    };
    draw_reel(&mut fb, REEL_LEFT_X, REEL_Y, angle);
    draw_reel(&mut fb, REEL_RIGHT_X, REEL_Y, angle);

    // ── Track title / artist ────────────────────────────────────────────
    if state.title_len > 0 {
        let title = core::str::from_utf8(&state.track_title[..state.title_len]).unwrap_or("???");
        let _ = Text::new(title, Point::new(MARGIN, TITLE_Y), style).draw(&mut fb);
    } else {
        let _ = Text::new("-- No Track --", Point::new(24, TITLE_Y), dim_style).draw(&mut fb);
    }
    if state.artist_len > 0 {
        let artist = core::str::from_utf8(&state.track_artist[..state.artist_len]).unwrap_or("");
        let _ = Text::new(artist, Point::new(MARGIN, ARTIST_Y), dim_style).draw(&mut fb);
    }

    // ── Progress bar ────────────────────────────────────────────────────
    draw_progress_bar(&mut fb, state.progress);

    // ── Playback status ─────────────────────────────────────────────────
    let status_text = if state.playing { "[PLAY]" } else { "[STOP]" };
    let _ = Text::new(status_text, Point::new(56, STATUS_Y), style).draw(&mut fb);

    // ── Volume ──────────────────────────────────────────────────────────
    draw_volume_bar(&mut fb, state.volume);

    // ── DMA flush (non-blocking to avoid audio stall) ───────────────────
    lcd::flush();
}

/// Draw the cassette "shell" outline — a rounded rectangle with a window.
fn draw_cassette_shell(fb: &mut FrameBuffer) {
    // Outer shell
    let _ = Rectangle::new(Point::new(4, 44), Size::new(162, 80))
        .into_styled(PrimitiveStyle::with_stroke(DARK_GRAY, 1))
        .draw(fb);

    // Inner tape window
    let _ = Rectangle::new(Point::new(20, 52), Size::new(130, 64))
        .into_styled(PrimitiveStyle::with_stroke(DIM_AMBER, 1))
        .draw(fb);
}

/// Draw a cassette reel at (cx, cy) with a given rotation angle.
fn draw_reel(fb: &mut FrameBuffer, cx: i32, cy: i32, angle: u16) {
    // Outer ring
    let _ = Circle::new(
        Point::new(cx - REEL_RADIUS as i32, cy - REEL_RADIUS as i32),
        REEL_RADIUS * 2,
    )
    .into_styled(PrimitiveStyle::with_stroke(REEL_COLOR, 2))
    .draw(fb);

    // Hub (smaller circle)
    let hub_r = 8u32;
    let _ = Circle::new(
        Point::new(cx - hub_r as i32, cy - hub_r as i32),
        hub_r * 2,
    )
    .into_styled(PrimitiveStyle::with_fill(AMBER))
    .draw(fb);

    // Spokes (3 lines from hub, rotated by `angle`)
    for spoke in 0..3 {
        let a = angle as i32 + spoke * 120;
        // Approximate sin/cos with integer math (good enough for UI)
        let (dx, dy) = fast_sincos(a, REEL_RADIUS as i32 - 4);
        let _ = Line::new(
            Point::new(cx, cy),
            Point::new(cx + dx, cy + dy),
        )
        .into_styled(PrimitiveStyle::with_stroke(DIM_AMBER, 1))
        .draw(fb);
    }
}

/// Draw the progress bar (0–1000 ‰).
fn draw_progress_bar(fb: &mut FrameBuffer, progress: u16) {
    let bar_w = (WIDTH as i32) - 2 * MARGIN;

    // Background
    let _ = Rectangle::new(
        Point::new(MARGIN, PROGRESS_Y),
        Size::new(bar_w as u32, PROGRESS_H),
    )
    .into_styled(PrimitiveStyle::with_fill(DARK_GRAY))
    .draw(fb);

    // Filled portion
    let filled = ((bar_w as u32) * progress as u32) / 1000;
    if filled > 0 {
        let _ = Rectangle::new(
            Point::new(MARGIN, PROGRESS_Y),
            Size::new(filled, PROGRESS_H),
        )
        .into_styled(PrimitiveStyle::with_fill(AMBER))
        .draw(fb);
    }
}

/// Draw the volume bar (0–100).
fn draw_volume_bar(fb: &mut FrameBuffer, volume: u8) {
    let style = MonoTextStyle::new(&FONT_6X10, DIM_AMBER);
    let _ = Text::new("VOL", Point::new(MARGIN, VOLUME_Y), style).draw(fb);

    let bar_x = MARGIN + 30;
    let bar_w = (WIDTH as i32) - bar_x - MARGIN;

    // Background
    let _ = Rectangle::new(
        Point::new(bar_x, VOLUME_Y - 6),
        Size::new(bar_w as u32, 8),
    )
    .into_styled(PrimitiveStyle::with_fill(DARK_GRAY))
    .draw(fb);

    // Filled portion
    let filled = ((bar_w as u32) * volume as u32) / 100;
    if filled > 0 {
        let _ = Rectangle::new(
            Point::new(bar_x, VOLUME_Y - 6),
            Size::new(filled, 8),
        )
        .into_styled(PrimitiveStyle::with_fill(AMBER))
        .draw(fb);
    }
}

/// Fast integer sin/cos approximation for spoke animation.
/// Returns (dx, dy) scaled by `radius`.
fn fast_sincos(angle_deg: i32, radius: i32) -> (i32, i32) {
    // Use a tiny LUT for 30° increments (good enough for 3 spokes at 120° apart)
    // sin/cos × 1024 for 0°, 30°, 60°, 90°, 120°, 150°, 180°, ...
    const SIN_LUT: [i32; 12] = [0, 512, 886, 1024, 886, 512, 0, -512, -886, -1024, -886, -512];
    const COS_LUT: [i32; 12] = [1024, 886, 512, 0, -512, -886, -1024, -886, -512, 0, 512, 886];

    let idx = ((angle_deg % 360 + 360) % 360 / 30) as usize % 12;
    let dx = (COS_LUT[idx] * radius) / 1024;
    let dy = (SIN_LUT[idx] * radius) / 1024;
    (dx, dy)
}
