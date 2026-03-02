// ═══════════════════════════════════════════════════════════════════════════════
// hal/interrupt.rs — NVIC-based interrupt management for RKNanoD (Cortex-M3)
//
// Uses the cortex-m crate for safe NVIC access.  Priority scheme:
//   Audio DMA  → priority 0x10 (highest — prevents clicks/pops)
//   I2C compl. → priority 0x20
//   GPIO / UI  → priority 0x40 (lowest — UI must never starve audio)
// ═══════════════════════════════════════════════════════════════════════════════

use cortex_m::peripheral::NVIC;

// ── RKNanoD IRQ numbers (external interrupts) ───────────────────────────────
// These map to the NVIC IRQ lines on the RKNanoD.  Exact numbers come from
// the RKNano SDK 1.0 header files.

/// DMA controller transfer-complete interrupt.
pub const IRQ_DMA: u8      = 0;
/// I2S0 interrupt (TX FIFO underrun / overrun).
pub const IRQ_I2S0: u8     = 1;
/// I2C0 interrupt (transfer complete / error).
pub const IRQ_I2C0: u8     = 4;
/// I2C1 interrupt (transfer complete / error).
pub const IRQ_I2C1: u8     = 5;
/// SPI0 interrupt.
pub const IRQ_SPI0: u8     = 6;
/// GPIO0 combined interrupt (all pins on bank 0).
pub const IRQ_GPIO0: u8    = 10;
/// GPIO1 combined interrupt.
pub const IRQ_GPIO1: u8    = 11;
/// Timer0 interrupt.
pub const IRQ_TIMER0: u8   = 14;
/// eMMC / SD controller interrupt.
pub const IRQ_EMMC: u8     = 16;

/// Maximum number of external IRQ sources.
pub const IRQ_COUNT: usize = 32;

// ── Priority levels (Cortex-M3: 3-bit priority = 8 levels, shifted to [7:5]) ─
pub const PRIO_AUDIO_DMA: u8 = 0x10; // Highest — audio must never glitch
pub const PRIO_I2C: u8       = 0x20;
pub const PRIO_I2S: u8       = 0x20;
pub const PRIO_SPI: u8       = 0x30;
pub const PRIO_GPIO: u8      = 0x40; // Lowest — UI is best-effort
pub const PRIO_TIMER: u8     = 0x30;

// ── Handler table ───────────────────────────────────────────────────────────
/// Type for an ISR callback.
pub type IsrFn = fn();

/// Stub handler — does nothing.
fn default_handler() {}

/// Global interrupt handler table, protected by a spinlock.
static HANDLER_TABLE: spin::Mutex<[IsrFn; IRQ_COUNT]> =
    spin::Mutex::new([default_handler; IRQ_COUNT]);

// ═════════════════════════════════════════════════════════════════════════════
// Initialisation
// ═════════════════════════════════════════════════════════════════════════════

/// Configure NVIC priorities and enable the interrupt lines we need.
///
/// Called from `main()` with the core peripheral handle.
pub fn init_nvic(nvic: &mut NVIC) {
    // ── Set priorities ──────────────────────────────────────────────────
    unsafe {
        nvic.set_priority(IrqNum(IRQ_DMA),   PRIO_AUDIO_DMA);
        nvic.set_priority(IrqNum(IRQ_I2S0),  PRIO_I2S);
        nvic.set_priority(IrqNum(IRQ_I2C0),  PRIO_I2C);
        nvic.set_priority(IrqNum(IRQ_I2C1),  PRIO_I2C);
        nvic.set_priority(IrqNum(IRQ_SPI0),  PRIO_SPI);
        nvic.set_priority(IrqNum(IRQ_GPIO0), PRIO_GPIO);
        nvic.set_priority(IrqNum(IRQ_GPIO1), PRIO_GPIO);
        nvic.set_priority(IrqNum(IRQ_TIMER0),PRIO_TIMER);
    }

    // ── Enable required IRQ lines ───────────────────────────────────────
    unsafe {
        NVIC::unmask(IrqNum(IRQ_DMA));
        NVIC::unmask(IrqNum(IRQ_I2S0));
        NVIC::unmask(IrqNum(IRQ_I2C0));
        NVIC::unmask(IrqNum(IRQ_I2C1));
        NVIC::unmask(IrqNum(IRQ_GPIO0));
    }
}

/// Register a handler for a specific IRQ number.
pub fn register(irq: u8, handler: IsrFn) {
    let mut table = HANDLER_TABLE.lock();
    table[irq as usize] = handler;
}

/// Globally enable interrupts (PRIMASK).
pub fn enable_global() {
    unsafe { cortex_m::interrupt::enable(); }
}

/// Globally disable interrupts and return previous PRIMASK state.
pub fn disable_global() -> u32 {
    let primask: u32;
    unsafe {
        core::arch::asm!(
            "mrs {}, PRIMASK",
            "cpsid i",
            out(reg) primask,
            options(nomem, nostack),
        );
    }
    primask
}

/// Restore interrupt state from a previous `disable_global()` call.
pub fn restore_global(state: u32) {
    unsafe {
        core::arch::asm!(
            "msr PRIMASK, {}",
            in(reg) state,
            options(nomem, nostack),
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Dispatcher — called from the default cortex-m-rt vector stubs
// ═════════════════════════════════════════════════════════════════════════════

/// Dispatch an interrupt by IRQ number.  Called from exception handlers.
pub fn dispatch(irq: u8) {
    let table = HANDLER_TABLE.lock();
    let handler = table[irq as usize];
    drop(table);
    handler();
}

// ═════════════════════════════════════════════════════════════════════════════
// IRQ number wrapper — implements `cortex_m::interrupt::InterruptNumber`
// ═════════════════════════════════════════════════════════════════════════════

/// Thin wrapper around a raw IRQ number for use with the cortex-m NVIC API.
#[derive(Clone, Copy)]
pub struct IrqNum(pub u8);

unsafe impl cortex_m::interrupt::InterruptNumber for IrqNum {
    fn number(self) -> u16 {
        self.0 as u16
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Critical section implementation for the `critical-section` crate
// ═════════════════════════════════════════════════════════════════════════════
struct EchoMiniCriticalSection;
critical_section::set_impl!(EchoMiniCriticalSection);

unsafe impl critical_section::Impl for EchoMiniCriticalSection {
    unsafe fn acquire() -> critical_section::RawRestoreState {
        let state = disable_global();
        state as u8
    }

    unsafe fn release(state: critical_section::RawRestoreState) {
        restore_global(state as u32);
    }
}
