// ═══════════════════════════════════════════════════════════════════════════════
// Echo Mini OS — lib.rs  (crate root)
// Pure Rust, bare-metal OS for FiiO Snowsky Echo Mini DAP
// Target: Rockchip RKNanoD (Dual-Core ARM Cortex-M3)
// ═══════════════════════════════════════════════════════════════════════════════
#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

// ── Public modules ───────────────────────────────────────────────────────────
pub mod hal;
pub mod mem;
pub mod audio;
pub mod display;
pub mod ui;
pub mod input;
pub mod clock;
pub mod boot;
