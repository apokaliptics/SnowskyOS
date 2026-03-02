// ═══════════════════════════════════════════════════════════════════════════════
// clock/pll.rs — RKNanoD CRU (Clock & Reset Unit) PLL management
//
// The CS43131 requires a specific MCLK frequency depending on the sample rate
// family:
//
//   44.1 kHz family (44.1 / 88.2 / 176.4 / 352.8 kHz):
//       MCLK = 22.5792 MHz (512 × 44100)
//
//   48 kHz family (32 / 48 / 96 / 192 / 384 kHz):
//       MCLK = 24.5760 MHz (512 × 48000)
//
// The RKNanoD CRU contains a configurable PLL. We program it to generate
// the correct MCLK via the I2S clock divider chain.
// ═══════════════════════════════════════════════════════════════════════════════

use crate::hal::mmio;

// ── CRU register offsets (base = 0x4000_0000) ──────────────────────────────
const CRU_BASE: usize = mmio::CRU_BASE;

/// PLL configuration register 0 (FBDIV, POSTDIV1)
const CRU_PLL_CON0:     usize = CRU_BASE + 0x00;
/// PLL configuration register 1 (REFDIV, POSTDIV2, DSMPD)
const CRU_PLL_CON1:     usize = CRU_BASE + 0x04;
/// PLL configuration register 2 (FRACDIV)
const CRU_PLL_CON2:     usize = CRU_BASE + 0x08;
/// PLL status register (LOCK bit)
const CRU_PLL_STATUS:   usize = CRU_BASE + 0x0C;

/// Clock selection register (I2S MCLK source select)
const CRU_CLKSEL_CON0:  usize = CRU_BASE + 0x20;
/// Clock selection register 1 (I2S MCLK divider)
const CRU_CLKSEL_CON1:  usize = CRU_BASE + 0x24;
/// Clock gate register 0 (peripheral clock enables)
const CRU_CLKGATE_CON0: usize = CRU_BASE + 0x40;

// ── PLL control bits ────────────────────────────────────────────────────────
/// PLL lock status bit in CRU_PLL_STATUS
const PLL_LOCK:   u32 = 1 << 0;
/// PLL power-down bit in CRU_PLL_CON1 (1 = powered down)
const PLL_PD:     u32 = 1 << 13;
/// Integer mode (disable fractional divider) in CRU_PLL_CON1
#[allow(dead_code)]
const PLL_DSMPD:  u32 = 1 << 12;

/// Audio clock family selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFamily {
    /// 44.1 kHz base → MCLK = 22.5792 MHz
    F44100,
    /// 48 kHz base → MCLK = 24.5760 MHz
    F48000,
}

impl AudioFamily {
    /// Target MCLK frequency in Hz.
    pub const fn mclk_hz(self) -> u32 {
        match self {
            AudioFamily::F44100 => 22_579_200,
            AudioFamily::F48000 => 24_576_000,
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// PLL configuration presets
// ═════════════════════════════════════════════════════════════════════════════
//
// The RKNanoD PLL uses the formula:
//     Fout = Fin × FBDIV / (REFDIV × POSTDIV1 × POSTDIV2)
//
// With fractional mode:
//     Fout = Fin × (FBDIV + FRACDIV/2^24) / (REFDIV × POSTDIV1 × POSTDIV2)
//
// With a 24 MHz crystal:
//
//   22.5792 MHz: FBDIV=188, FRACDIV=4194304(≈0.25), REFDIV=5, POSTDIV1=4, POSTDIV2=10
//                → 24 × 188.25 / (5×4×10) = 24 × 188.25 / 200 = 22.59 MHz
//
//   24.576 MHz:  FBDIV=204, FRACDIV=13421773(≈0.8), REFDIV=5, POSTDIV1=4, POSTDIV2=10
//                → 24 × 204.8 / (5×4×10) = 24 × 204.8 / 200 = 24.576 MHz

const XTAL_HZ: u32 = 24_000_000;

/// PLL parameter set for the RKNanoD CRU PLL.
struct PllParams {
    fbdiv:    u32,   // Feedback divider (integer part)
    fracdiv:  u32,   // Fractional divider (24-bit)
    refdiv:   u32,   // Reference divider
    postdiv1: u32,   // Post divider 1
    postdiv2: u32,   // Post divider 2
}

/// 22.5792 MHz — 44.1 kHz family
const PLL_44100: PllParams = PllParams {
    fbdiv: 188, fracdiv: 4_194_304, refdiv: 5, postdiv1: 4, postdiv2: 10,
};

/// 24.576 MHz — 48 kHz family
const PLL_48000: PllParams = PllParams {
    fbdiv: 204, fracdiv: 13_421_773, refdiv: 5, postdiv1: 4, postdiv2: 10,
};

// ═════════════════════════════════════════════════════════════════════════════
// Public API
// ═════════════════════════════════════════════════════════════════════════════

/// Switch the audio PLL to generate the MCLK for the given sample-rate family.
///
/// This must be called **before** starting I2S playback, and the CS43131
/// `set_sample_rate()` must be called to match.
pub fn set_audio_clock(family: AudioFamily) {
    let params = match family {
        AudioFamily::F44100 => &PLL_44100,
        AudioFamily::F48000 => &PLL_48000,
    };

    // ── 1. Power down PLL before reconfiguring ──────────────────────────
    mmio::set_bits(CRU_PLL_CON1, PLL_PD);

    // ── 2. Program PLL parameters ───────────────────────────────────────
    // CON0: [15:0] FBDIV, [19:16] POSTDIV1
    let con0 = (params.fbdiv & 0xFFF)
             | ((params.postdiv1 & 0x07) << 16);
    mmio::write32(CRU_PLL_CON0, con0);

    // CON1: [5:0] REFDIV, [10:8] POSTDIV2, [12] DSMPD (0 = fractional mode), [13] PD (0 = active)
    let con1 = (params.refdiv & 0x3F)
             | ((params.postdiv2 & 0x07) << 8);
    // Enable fractional mode (DSMPD = 0), power on (PD = 0)
    mmio::write32(CRU_PLL_CON1, con1);

    // CON2: [23:0] FRACDIV
    mmio::write32(CRU_PLL_CON2, params.fracdiv & 0x00FF_FFFF);

    // ── 3. Wait for PLL lock ────────────────────────────────────────────
    mmio::wait_for(CRU_PLL_STATUS, PLL_LOCK, PLL_LOCK);

    // ── 4. Route PLL → I2S MCLK ────────────────────────────────────────
    // CLKSEL_CON0: bit[0] = 1 → select PLL as I2S clock source
    mmio::set_bits(CRU_CLKSEL_CON0, 1 << 0);

    // CLKSEL_CON1: I2S MCLK divider = 1 (pass-through from PLL)
    mmio::write32(CRU_CLKSEL_CON1, 0x0000); // divider = 0 means /1

    // ── 5. Ensure I2S clock gate is open ────────────────────────────────
    // Clear the I2S clock gate bit (bit 4) to un-gate the clock
    mmio::clear_bits(CRU_CLKGATE_CON0, 1 << 4);

    // ── 6. Memory fence ─────────────────────────────────────────────────
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// Read back the current PLL lock status.
pub fn is_pll_locked() -> bool {
    mmio::read32(CRU_PLL_STATUS) & PLL_LOCK != 0
}

/// Get the currently configured MCLK frequency based on PLL settings.
///
/// Uses the integer approximation:
///   Fout ≈ XTAL × FBDIV / (REFDIV × POSTDIV1 × POSTDIV2)
pub fn current_mclk_hz() -> u32 {
    let con0 = mmio::read32(CRU_PLL_CON0);
    let con1 = mmio::read32(CRU_PLL_CON1);

    let fbdiv    = con0 & 0xFFF;
    let postdiv1 = (con0 >> 16) & 0x07;
    let refdiv   = con1 & 0x3F;
    let postdiv2 = (con1 >> 8) & 0x07;

    let denom = refdiv * postdiv1 * postdiv2;
    if denom == 0 { return 0; }

    XTAL_HZ / denom * fbdiv // integer approximation (ignores FRACDIV)
}
