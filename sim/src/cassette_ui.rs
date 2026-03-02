// ═══════════════════════════════════════════════════════════════════════════════
// sim/src/cassette_ui.rs — Cassette UI rendering (pixel-identical to firmware)
//
// This is a direct port of src/ui/cassette.rs, but operating on the host
// FrameBuffer struct instead of the hardware-backed static.  All layout
// constants, colours, and drawing logic are identical so that what you
// see in the simulator window is exactly what appears on the device LCD.
// ═══════════════════════════════════════════════════════════════════════════════

use crate::framebuffer::{self, FrameBuffer, WIDTH};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle, Rectangle};
use embedded_graphics::mono_font::{ascii::FONT_6X10, MonoTextStyle};
use embedded_graphics::text::Text;
use embedded_graphics::pixelcolor::Rgb565;

// ── Colour palette (warm retro cassette tones) ──────────────────────────────
const AMBER: Rgb565      = Rgb565::new(31, 23, 0);
const DIM_AMBER: Rgb565  = Rgb565::new(16, 12, 0);
const REEL_COLOR: Rgb565 = Rgb565::new(20, 16, 4);
const DARK_GRAY: Rgb565  = Rgb565::new(8, 16, 8);

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
const STATUS_Y: i32 = 240;
const VOLUME_Y: i32 = 270;

// ── Playback state ──────────────────────────────────────────────────────────

pub struct UiState {
    pub playing: bool,
    pub volume: u8,
    pub progress: u16,       // 0–1000 (‰)
    pub reel_angle: u16,
    pub track_title: String,
    pub track_artist: String,
    pub dirty: bool,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            playing: false,
            volume: 80,
            progress: 0,
            reel_angle: 0,
            track_title: String::new(),
            track_artist: String::new(),
            dirty: true,
        }
    }
}

/// Button events (mirrors firmware ButtonEvent enum).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonEvent {
    None,
    Play,
    Next,
    Prev,
    VolUp,
    VolDown,
}

// ═════════════════════════════════════════════════════════════════════════════
// Public API
// ═════════════════════════════════════════════════════════════════════════════

/// Draw the initial idle screen.
pub fn draw_idle_screen(fb: &mut FrameBuffer) {
    fb.fill_color(framebuffer::COLOR_DARK_BG);

    draw_cassette_shell(fb);

    let style = MonoTextStyle::new(&FONT_6X10, AMBER);
    let _ = Text::new("Echo Mini OS", Point::new(34, 24), style).draw(fb);

    let dim_style = MonoTextStyle::new(&FONT_6X10, DIM_AMBER);
    let _ = Text::new("Bit-Perfect Audio", Point::new(22, 38), dim_style).draw(fb);

    draw_reel(fb, REEL_LEFT_X, REEL_Y, 0);
    draw_reel(fb, REEL_RIGHT_X, REEL_Y, 0);

    let _ = Text::new("-- No Track --", Point::new(24, TITLE_Y), dim_style).draw(fb);

    draw_progress_bar(fb, 0);

    let _ = Text::new("[STOP]", Point::new(56, STATUS_Y), style).draw(fb);

    draw_volume_bar(fb, 80);
}

/// Handle a button event and update state.
pub fn handle_input(state: &mut UiState, event: ButtonEvent) {
    match event {
        ButtonEvent::Play => {
            state.playing = !state.playing;
            state.dirty = true;
        }
        ButtonEvent::Next => {
            state.progress = (state.progress + 50).min(1000);
            state.dirty = true;
        }
        ButtonEvent::Prev => {
            state.progress = state.progress.saturating_sub(50);
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
}

/// Full redraw (should be called when state.dirty or periodically for animation).
pub fn redraw(fb: &mut FrameBuffer, state: &mut UiState) {
    fb.fill_color(framebuffer::COLOR_DARK_BG);

    draw_cassette_shell(fb);

    let style = MonoTextStyle::new(&FONT_6X10, AMBER);
    let dim_style = MonoTextStyle::new(&FONT_6X10, DIM_AMBER);

    let _ = Text::new("Echo Mini OS", Point::new(34, 24), style).draw(fb);
    let _ = Text::new("Bit-Perfect Audio", Point::new(22, 38), dim_style).draw(fb);

    // Reels (animated when playing)
    let angle = if state.playing {
        state.reel_angle = (state.reel_angle + 15) % 360;
        state.reel_angle
    } else {
        0
    };
    draw_reel(fb, REEL_LEFT_X, REEL_Y, angle);
    draw_reel(fb, REEL_RIGHT_X, REEL_Y, angle);

    // Track info
    if !state.track_title.is_empty() {
        let _ = Text::new(&state.track_title, Point::new(MARGIN, TITLE_Y), style).draw(fb);
    } else {
        let _ = Text::new("-- No Track --", Point::new(24, TITLE_Y), dim_style).draw(fb);
    }
    if !state.track_artist.is_empty() {
        let _ = Text::new(&state.track_artist, Point::new(MARGIN, ARTIST_Y), dim_style).draw(fb);
    }

    // Progress bar
    draw_progress_bar(fb, state.progress);

    // Playback status
    let status_text = if state.playing { "[PLAY]" } else { "[STOP]" };
    let _ = Text::new(status_text, Point::new(56, STATUS_Y), style).draw(fb);

    // Volume
    draw_volume_bar(fb, state.volume);

    state.dirty = false;
}

// ═════════════════════════════════════════════════════════════════════════════
// Internal drawing functions (identical to firmware)
// ═════════════════════════════════════════════════════════════════════════════

fn draw_cassette_shell(fb: &mut FrameBuffer) {
    let _ = Rectangle::new(Point::new(4, 44), Size::new(162, 80))
        .into_styled(PrimitiveStyle::with_stroke(DARK_GRAY, 1))
        .draw(fb);

    let _ = Rectangle::new(Point::new(20, 52), Size::new(130, 64))
        .into_styled(PrimitiveStyle::with_stroke(DIM_AMBER, 1))
        .draw(fb);
}

fn draw_reel(fb: &mut FrameBuffer, cx: i32, cy: i32, angle: u16) {
    let _ = Circle::new(
        Point::new(cx - REEL_RADIUS as i32, cy - REEL_RADIUS as i32),
        REEL_RADIUS * 2,
    )
    .into_styled(PrimitiveStyle::with_stroke(REEL_COLOR, 2))
    .draw(fb);

    let hub_r = 8u32;
    let _ = Circle::new(
        Point::new(cx - hub_r as i32, cy - hub_r as i32),
        hub_r * 2,
    )
    .into_styled(PrimitiveStyle::with_fill(AMBER))
    .draw(fb);

    for spoke in 0..3 {
        let a = angle as i32 + spoke * 120;
        let (dx, dy) = fast_sincos(a, REEL_RADIUS as i32 - 4);
        let _ = Line::new(
            Point::new(cx, cy),
            Point::new(cx + dx, cy + dy),
        )
        .into_styled(PrimitiveStyle::with_stroke(DIM_AMBER, 1))
        .draw(fb);
    }
}

fn draw_progress_bar(fb: &mut FrameBuffer, progress: u16) {
    let bar_w = (WIDTH as i32) - 2 * MARGIN;

    let _ = Rectangle::new(
        Point::new(MARGIN, PROGRESS_Y),
        Size::new(bar_w as u32, PROGRESS_H),
    )
    .into_styled(PrimitiveStyle::with_fill(DARK_GRAY))
    .draw(fb);

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

fn draw_volume_bar(fb: &mut FrameBuffer, volume: u8) {
    let style = MonoTextStyle::new(&FONT_6X10, DIM_AMBER);
    let _ = Text::new("VOL", Point::new(MARGIN, VOLUME_Y), style).draw(fb);

    let bar_x = MARGIN + 30;
    let bar_w = (WIDTH as i32) - bar_x - MARGIN;

    let _ = Rectangle::new(
        Point::new(bar_x, VOLUME_Y - 6),
        Size::new(bar_w as u32, 8),
    )
    .into_styled(PrimitiveStyle::with_fill(DARK_GRAY))
    .draw(fb);

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

fn fast_sincos(angle_deg: i32, radius: i32) -> (i32, i32) {
    const SIN_LUT: [i32; 12] = [0, 512, 886, 1024, 886, 512, 0, -512, -886, -1024, -886, -512];
    const COS_LUT: [i32; 12] = [1024, 886, 512, 0, -512, -886, -1024, -886, -512, 0, 512, 886];

    let idx = ((angle_deg % 360 + 360) % 360 / 30) as usize % 12;
    let dx = (COS_LUT[idx] * radius) / 1024;
    let dy = (SIN_LUT[idx] * radius) / 1024;
    (dx, dy)
}
