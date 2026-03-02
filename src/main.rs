// ═══════════════════════════════════════════════════════════════════════════════
// Echo Mini OS — main.rs  (entry point)
// `no_std`, `no_main` bare-metal binary for Rockchip RKNanoD (Dual Cortex-M3)
// ═══════════════════════════════════════════════════════════════════════════════
#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use cortex_m::peripheral::Peripherals as CorePeripherals;
use cortex_m_rt::{entry, exception};

use echo_mini_os::mem::allocator;
use echo_mini_os::hal::mmio;
use echo_mini_os::hal::interrupt;
use echo_mini_os::audio::engine;
use echo_mini_os::audio::cs43131;
use echo_mini_os::clock;
use echo_mini_os::display;
use echo_mini_os::input;
use echo_mini_os::ui;

use core::panic::PanicInfo;

// ── Linker-provided symbols ──────────────────────────────────────────────────
extern "C" {
    static __heap_start: u8;
    static __heap_end: u8;
}

// ═════════════════════════════════════════════════════════════════════════════
// #[entry] — cortex-m-rt calls this after reset, BSS is zeroed, .data copied
// ═════════════════════════════════════════════════════════════════════════════
#[entry]
fn main() -> ! {
    // ── 1. Initialise heap allocator ────────────────────────────────────
    let heap_start = unsafe { &__heap_start as *const u8 as usize };
    let heap_end = unsafe { &__heap_end as *const u8 as usize };
    allocator::init(heap_start, heap_end - heap_start);

    // ── 2. Grab Cortex-M core peripherals for NVIC setup ───────────────
    let mut cp = CorePeripherals::take().unwrap();

    // ── 3. Platform init (clock gates, watchdog disable) ───────────────
    mmio::init_platform();

    // ── 4. Configure NVIC interrupt priorities ─────────────────────────
    //   Audio DMA  = priority 0x10 (highest — must never be starved)
    //   I2C        = priority 0x20
    //   GPIO (UI)  = priority 0x40 (lower — UI must not cause audio pops)
    interrupt::init_nvic(&mut cp.NVIC);

    // ── 5. Clock: default to 44.1 kHz family ───────────────────────────
    clock::pll::set_audio_clock(clock::pll::AudioFamily::F44100);

    // ── 6. DAC bring-up (dual CS43131) ─────────────────────────────────
    let mut dac_left = cs43131::Cs43131::new(cs43131::DacBus::Left);
    let mut dac_right = cs43131::Cs43131::new(cs43131::DacBus::Right);

    // Default: balanced mode (differential mono, both DACs active)
    cs43131::set_balanced_mode(&mut dac_left, &mut dac_right);

    // ── 7. Audio engine (DMA double-buffer → I2S) ──────────────────────
    engine::init();

    // ── 8. Display bring-up ────────────────────────────────────────────
    display::lcd::init();

    // ── 9. Input subsystem (5 buttons + debounce) ──────────────────────
    input::buttons::init();

    // ── 10. Draw initial UI frame ──────────────────────────────────────
    ui::cassette::draw_idle_screen();

    // ── 11. Enable interrupts globally ─────────────────────────────────
    interrupt::enable_global();

    // ── Main event loop ─────────────────────────────────────────────────
    loop {
        // Process button events
        if let Some(event) = input::buttons::poll() {
            ui::cassette::handle_input(event);
        }

        // If the audio engine signals a buffer-underrun, refill
        if engine::needs_refill() {
            engine::refill_from_decoder();
        }

        // Yield / WFI — halt until next interrupt to save power
        cortex_m::asm::wfi();
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Exception handlers (cortex-m-rt)
// ═════════════════════════════════════════════════════════════════════════════

/// HardFault handler — in release we just spin.
#[exception]
unsafe fn HardFault(_frame: &cortex_m_rt::ExceptionFrame) -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}

/// DefaultHandler — catches any unregistered interrupt.
#[exception]
unsafe fn DefaultHandler(_irqn: i16) {
    // Unhandled interrupt — ignore
}

// ═════════════════════════════════════════════════════════════════════════════
// Panic handler — in release we just spin; debug could blink an LED / UART
// ═════════════════════════════════════════════════════════════════════════════
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Alloc error handler
// ═════════════════════════════════════════════════════════════════════════════
#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
