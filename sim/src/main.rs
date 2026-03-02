// ═══════════════════════════════════════════════════════════════════════════════
// sim/src/main.rs — SnowskyOS Desktop Simulator
//
// Renders the cassette UI in a native window so you can see exactly what the
// device LCD looks like, and interact with it via keyboard:
//
//   Space      → Play / Pause
//   Right      → Next track
//   Left       → Previous track
//   Up         → Volume up
//   Down       → Volume down
//   Escape / Q → Quit
//
// The window is scaled 2× for comfortable desktop viewing.
// ═══════════════════════════════════════════════════════════════════════════════

mod framebuffer;
mod cassette_ui;

use framebuffer::{FrameBuffer, WIDTH, HEIGHT};
use cassette_ui::{ButtonEvent, UiState};
use minifb::{Key, Window, WindowOptions};
use std::time::{Duration, Instant};

/// Scale factor — the 170×320 LCD is tiny on a monitor.
const SCALE: usize = 3;
const WIN_W: usize = WIDTH * SCALE;
const WIN_H: usize = HEIGHT * SCALE;

/// Target frame rate for the simulator.
const FPS: u64 = 30;

fn main() {
    println!("╔═══════════════════════════════════════════════════╗");
    println!("║       SnowskyOS Simulator — Echo Mini DAP        ║");
    println!("╠═══════════════════════════════════════════════════╣");
    println!("║  Space   = Play / Pause                          ║");
    println!("║  →       = Next track                            ║");
    println!("║  ←       = Previous track                        ║");
    println!("║  ↑       = Volume up                             ║");
    println!("║  ↓       = Volume down                           ║");
    println!("║  Esc / Q = Quit                                  ║");
    println!("╚═══════════════════════════════════════════════════╝");

    // ── Create window ───────────────────────────────────────────────────
    let mut window = Window::new(
        "SnowskyOS — Echo Mini Simulator",
        WIN_W,
        WIN_H,
        WindowOptions {
            resize: false,
            scale_mode: minifb::ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
    .expect("Failed to create window");

    // Limit update rate
    window.set_target_fps(FPS as usize);

    // ── Framebuffer + UI state ──────────────────────────────────────────
    let mut fb = FrameBuffer::new();
    let mut state = UiState::new();

    // Draw initial idle screen
    cassette_ui::draw_idle_screen(&mut fb);

    // Scaled buffer for minifb
    let mut win_buf = vec![0u32; WIN_W * WIN_H];

    let frame_duration = Duration::from_micros(1_000_000 / FPS);
    let mut last_frame = Instant::now();

    // Track key states for edge detection (trigger on press, not hold)
    let mut prev_keys = Vec::new();

    // ── Main loop ───────────────────────────────────────────────────────
    while window.is_open() && !window.is_key_down(Key::Escape) && !window.is_key_down(Key::Q) {
        let now = Instant::now();
        if now.duration_since(last_frame) < frame_duration {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        last_frame = now;

        // ── Process keyboard input (edge-triggered) ─────────────────────
        let keys = window.get_keys();
        let event = map_keys_to_event(&keys, &prev_keys);
        prev_keys = keys;

        if event != ButtonEvent::None {
            cassette_ui::handle_input(&mut state, event);
        }

        // ── Animate reels when playing ──────────────────────────────────
        if state.playing || state.dirty {
            cassette_ui::redraw(&mut fb, &mut state);
        }

        // ── Scale framebuffer → window buffer ───────────────────────────
        let raw = fb.to_argb32();
        scale_buffer(&raw, WIDTH, HEIGHT, &mut win_buf, WIN_W, WIN_H, SCALE);

        // ── Draw a thin border around the "device" for realism ──────────
        draw_device_border(&mut win_buf, WIN_W, WIN_H);

        window
            .update_with_buffer(&win_buf, WIN_W, WIN_H)
            .expect("Failed to update window");
    }

    println!("Simulator closed.");
}

/// Map keyboard to ButtonEvent (edge-triggered: only fires on new presses).
fn map_keys_to_event(current: &[Key], previous: &[Key]) -> ButtonEvent {
    // Check for newly pressed keys (in current but not in previous)
    let newly_pressed = |key: Key| current.contains(&key) && !previous.contains(&key);

    if newly_pressed(Key::Space) {
        ButtonEvent::Play
    } else if newly_pressed(Key::Right) {
        ButtonEvent::Next
    } else if newly_pressed(Key::Left) {
        ButtonEvent::Prev
    } else if newly_pressed(Key::Up) {
        ButtonEvent::VolUp
    } else if newly_pressed(Key::Down) {
        ButtonEvent::VolDown
    } else {
        ButtonEvent::None
    }
}

/// Nearest-neighbour upscale from (src_w × src_h) to (dst_w × dst_h).
fn scale_buffer(
    src: &[u32], src_w: usize, src_h: usize,
    dst: &mut [u32], dst_w: usize, _dst_h: usize,
    scale: usize,
) {
    for y in 0..src_h {
        for x in 0..src_w {
            let pixel = src[y * src_w + x];
            for sy in 0..scale {
                for sx in 0..scale {
                    let dx = x * scale + sx;
                    let dy = y * scale + sy;
                    dst[dy * dst_w + dx] = pixel;
                }
            }
        }
    }
}

/// Draw a subtle dark border around the window to simulate the device bezel.
fn draw_device_border(buf: &mut [u32], w: usize, h: usize) {
    let border_color = 0xFF10_1010; // near-black bezel
    let thickness = 2;

    for t in 0..thickness {
        for x in 0..w {
            buf[t * w + x] = border_color;              // top
            buf[(h - 1 - t) * w + x] = border_color;    // bottom
        }
        for y in 0..h {
            buf[y * w + t] = border_color;               // left
            buf[y * w + (w - 1 - t)] = border_color;     // right
        }
    }
}
