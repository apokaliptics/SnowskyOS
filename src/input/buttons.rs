// ═══════════════════════════════════════════════════════════════════════════════
// input/buttons.rs — 5-button matrix + debounce for the Echo Mini
//
// Button layout (typical FiiO DAP):
//   [PREV]  [PLAY/PAUSE]  [NEXT]  [VOL+]  [VOL-]
//
// All buttons are active-low with internal pull-ups, on GPIO bank 0.
// Debounce: 20 ms timer-based via interrupt + polling hybrid.
// ═══════════════════════════════════════════════════════════════════════════════

use crate::hal::{gpio, interrupt};
use core::sync::atomic::{AtomicU8, Ordering};

// ── GPIO assignments (Bank 0, active-low) ───────────────────────────────────
const BTN_BANK: usize = 0; // GPIO bank 0

const PIN_PREV:  u8 = 0;
const PIN_PLAY:  u8 = 1;
const PIN_NEXT:  u8 = 2;
const PIN_VOLUP: u8 = 3;
const PIN_VOLDN: u8 = 4;

const ALL_PINS: [u8; 5] = [PIN_PREV, PIN_PLAY, PIN_NEXT, PIN_VOLUP, PIN_VOLDN];

// ── Debounce parameters ─────────────────────────────────────────────────────
/// Debounce window in poll cycles (~20 ms at typical poll rate).
const DEBOUNCE_COUNT: u8 = 3;

/// Atomic event mailbox — the ISR writes, the main loop reads.
/// Encoded as a `ButtonEvent` discriminant (0 = none).
static PENDING_EVENT: AtomicU8 = AtomicU8::new(0);

// ═════════════════════════════════════════════════════════════════════════════
// Button events
// ═════════════════════════════════════════════════════════════════════════════

/// High-level button events consumed by the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ButtonEvent {
    None     = 0,
    Prev     = 1,
    Play     = 2,
    Next     = 3,
    VolUp    = 4,
    VolDown  = 5,
}

impl From<u8> for ButtonEvent {
    fn from(v: u8) -> Self {
        match v {
            1 => ButtonEvent::Prev,
            2 => ButtonEvent::Play,
            3 => ButtonEvent::Next,
            4 => ButtonEvent::VolUp,
            5 => ButtonEvent::VolDown,
            _ => ButtonEvent::None,
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Debounce state (one per button)
// ═════════════════════════════════════════════════════════════════════════════

struct DebounceState {
    stable: bool,
    count: u8,
}

static mut DEBOUNCE: [DebounceState; 5] = [
    DebounceState { stable: true, count: 0 },
    DebounceState { stable: true, count: 0 },
    DebounceState { stable: true, count: 0 },
    DebounceState { stable: true, count: 0 },
    DebounceState { stable: true, count: 0 },
];

// ═════════════════════════════════════════════════════════════════════════════
// Initialisation
// ═════════════════════════════════════════════════════════════════════════════

/// Configure button GPIOs as inputs with falling-edge interrupts.
pub fn init() {
    for &pin in &ALL_PINS {
        gpio::set_input_irq_falling(BTN_BANK, pin);
    }

    // Register GPIO bank 0 interrupt handler
    interrupt::register(interrupt::IRQ_GPIO0, button_isr);
}

// ═════════════════════════════════════════════════════════════════════════════
// ISR (called from interrupt context — must be fast)
// ═════════════════════════════════════════════════════════════════════════════

fn button_isr() {
    let flags = gpio::pending_irqs(BTN_BANK);

    for (i, &pin) in ALL_PINS.iter().enumerate() {
        if flags & (1 << pin) != 0 {
            gpio::clear_irq(BTN_BANK, pin);

            // Simple immediate event — debounce is done in `poll()`
            let event = (i as u8) + 1; // maps to ButtonEvent discriminant
            PENDING_EVENT.store(event, Ordering::Release);
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Polling API (called from the main loop)
// ═════════════════════════════════════════════════════════════════════════════

/// Poll for a debounced button event. Returns `Some(event)` if a button
/// press has been confirmed, `None` otherwise.
///
/// This uses a hybrid approach:
///   1. The ISR sets `PENDING_EVENT` on the falling edge.
///   2. `poll()` re-reads the pin state to confirm (debounce).
pub fn poll() -> Option<ButtonEvent> {
    let raw = PENDING_EVENT.swap(0, Ordering::AcqRel);
    if raw == 0 {
        // No ISR event — do a full scan with debounce anyway
        return scan_debounce();
    }

    let event = ButtonEvent::from(raw);
    if event == ButtonEvent::None {
        return None;
    }

    // Confirm: re-read the pin — if still low (pressed), accept
    let pin_idx = (raw - 1) as usize;
    let pin = ALL_PINS[pin_idx];
    if !gpio::read_pin(BTN_BANK, pin) {
        // Still pressed → confirmed
        Some(event)
    } else {
        // Bounced away — reject
        None
    }
}

/// Software debounce scan: read all buttons and require DEBOUNCE_COUNT
/// stable reads before accepting a press.
fn scan_debounce() -> Option<ButtonEvent> {
    for (i, &pin) in ALL_PINS.iter().enumerate() {
        let pressed = !gpio::read_pin(BTN_BANK, pin); // active-low
        let db = unsafe { &mut DEBOUNCE[i] };

        if pressed != db.stable {
            db.count += 1;
            if db.count >= DEBOUNCE_COUNT {
                db.stable = pressed;
                db.count = 0;
                if pressed {
                    return Some(ButtonEvent::from((i as u8) + 1));
                }
            }
        } else {
            db.count = 0;
        }
    }
    None
}
