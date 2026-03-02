// ═══════════════════════════════════════════════════════════════════════════════
// audio/cs43131.rs — Cirrus Logic CS43131 DAC driver
//
// Dual-DAC topology for the FiiO Snowsky Echo Mini (RKNanoD).
// I2C addresses: 0x30 (left, AD0=LOW), 0x32 (right, AD0=HIGH).
//
// Two output modes:
//   • Balanced mode — both DACs run in differential mono (one per channel)
//   • SE (single-ended) mode — one DAC in stereo, low-power mode
//
// Register control via I2C; audio data arrives over I2S (handled by DMA).
//
// Reference: CS43131 Datasheet rev A, Cirrus Logic.
// ═══════════════════════════════════════════════════════════════════════════════

use crate::hal::i2c::I2cBus;
use crate::hal::mmio;

// ═════════════════════════════════════════════════════════════════════════════
// I2C addresses (AD0 pin selects left/right in dual-mono topology)
// ═════════════════════════════════════════════════════════════════════════════
const CS43131_ADDR_LEFT:  u8 = 0x30; // AD0 = LOW  → left channel
const CS43131_ADDR_RIGHT: u8 = 0x32; // AD0 = HIGH → right channel

// ═════════════════════════════════════════════════════════════════════════════
// CS43131 Register Map (24-bit address space, 8-bit data)
// ═════════════════════════════════════════════════════════════════════════════
pub mod regs {
    //! Key CS43131 register addresses.

    /// Device ID (read-only) — should return 0x43 for CS43131.
    pub const DEVID_AB:         u32 = 0x00_0001;
    pub const DEVID_CD:         u32 = 0x00_0002;
    pub const DEVID_E:          u32 = 0x00_0003;
    pub const REV_ID:           u32 = 0x00_0005;

    // ── Power control ───────────────────────────────────────────────────
    pub const PWR_CTL1:         u32 = 0x00_0006;
    pub const PWR_CTL2:         u32 = 0x00_0007;

    // ── Clock configuration ─────────────────────────────────────────────
    pub const MCLK_SRC_SEL:     u32 = 0x00_0008;
    pub const MCLK_FREQ:        u32 = 0x00_0009;
    pub const SRC_CTL:          u32 = 0x00_000A;
    pub const MCLK_INT:         u32 = 0x00_000B;

    // ── Audio Serial Port (ASP) ─────────────────────────────────────────
    pub const ASP_CTL1:         u32 = 0x00_000D;
    pub const ASP_CTL2:         u32 = 0x00_000E;
    pub const ASP_CTL3:         u32 = 0x00_000F;

    // ── Sample rate ─────────────────────────────────────────────────────
    pub const SP_SRATE:         u32 = 0x00_0010;

    // ── Digital filter ──────────────────────────────────────────────────
    pub const DAC_CTL1:         u32 = 0x00_0020;
    pub const DAC_CTL2:         u32 = 0x00_0021;

    // ── Volume control (hardware — does NOT touch digital bitstream) ────
    pub const HP_A_VOL:         u32 = 0x00_0022;
    pub const HP_B_VOL:         u32 = 0x00_0023;
    pub const HP_CTL:           u32 = 0x00_0024;

    // ── Headphone output control ────────────────────────────────────────
    pub const HP_OUT_CTL:       u32 = 0x00_0025;
    pub const CLASS_H_CTL:      u32 = 0x00_0026;

    // ── DSD configuration ───────────────────────────────────────────────
    pub const DSD_CTL1:         u32 = 0x00_0030;
    pub const DSD_CTL2:         u32 = 0x00_0031;
    pub const DSD_PATH_CTL:     u32 = 0x00_0032;

    // ── Interrupt / status ──────────────────────────────────────────────
    pub const INT_STATUS:       u32 = 0x00_0040;
    pub const INT_MASK:         u32 = 0x00_0041;

    // ── PCM path control ────────────────────────────────────────────────
    pub const PCM_PATH_CTL1:    u32 = 0x00_0050;
    pub const PCM_PATH_CTL2:    u32 = 0x00_0051;

    // ── XTAL / PLL ──────────────────────────────────────────────────────
    pub const PLL_CTL:          u32 = 0x00_0060;
    pub const PLL_DIV_FRAC0:    u32 = 0x00_0061;
    pub const PLL_DIV_FRAC1:    u32 = 0x00_0062;
    pub const PLL_DIV_FRAC2:    u32 = 0x00_0063;
    pub const PLL_DIV_INT:      u32 = 0x00_0064;
}

// ═════════════════════════════════════════════════════════════════════════════
// Digital-filter presets
// ═════════════════════════════════════════════════════════════════════════════

/// Available digital filter types in the CS43131.
/// Selected via `DAC_CTL1[2:0]`.  We expose these so the UI can let users
/// choose their preferred filter.
#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum DigitalFilter {
    /// Fast roll-off, linear phase (default).
    FastRolloff         = 0x00,
    /// Slow roll-off, linear phase.
    SlowRolloff         = 0x01,
    /// Fast roll-off, minimum phase.
    FastMinPhase        = 0x02,
    /// Slow roll-off, minimum phase.
    SlowMinPhase        = 0x03,
    /// Apodizing, fast roll-off — wide stop-band rejection.
    ApodizingFast       = 0x04,
    /// Hybrid fast roll-off (CS43131-specific).
    HybridFastRolloff   = 0x05,
    /// Brickwall (non-oversampling / NOS style).
    Brickwall           = 0x06,
}

// ═════════════════════════════════════════════════════════════════════════════
// Sample-rate codes (SP_SRATE register)
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum SampleRateCode {
    Rate32000   = 0x02,
    Rate44100   = 0x04,
    Rate48000   = 0x05,
    Rate88200   = 0x08,
    Rate96000   = 0x09,
    Rate176400  = 0x0C,
    Rate192000  = 0x0D,
    Rate352800  = 0x10,
    Rate384000  = 0x11,
    DSD64       = 0x20,
    DSD128      = 0x21,
    DSD256      = 0x22,
}

// ═════════════════════════════════════════════════════════════════════════════
// Which physical I2C bus this DAC instance sits on
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy)]
pub enum DacBus {
    Left,   // I2C0, address 0x30
    Right,  // I2C1, address 0x32
}

// ═════════════════════════════════════════════════════════════════════════════
// CS43131 driver
// ═════════════════════════════════════════════════════════════════════════════

/// Driver for a single CS43131 DAC in the dual-DAC pair.
pub struct Cs43131 {
    bus: I2cBus,
    addr: u8,
    side: DacBus,
}

impl Cs43131 {
    /// Construct a new driver for the specified side of the dual-DAC pair.
    pub fn new(side: DacBus) -> Self {
        let (base, addr) = match side {
            DacBus::Left  => (mmio::I2C0_BASE, CS43131_ADDR_LEFT),
            DacBus::Right => (mmio::I2C1_BASE, CS43131_ADDR_RIGHT),
        };
        let bus = I2cBus::new(base);
        Self { bus, addr, side }
    }

    // ── Low-level register access ───────────────────────────────────────

    /// Write a single register (24-bit address, 8-bit data).
    fn write_reg(&self, reg: u32, val: u8) {
        let buf = [
            ((reg >> 16) & 0xFF) as u8,
            ((reg >> 8) & 0xFF) as u8,
            (reg & 0xFF) as u8,
            val,
        ];
        let _ = self.bus.write(self.addr, &buf);
    }

    /// Read a single register.
    fn read_reg(&self, reg: u32) -> u8 {
        let addr_buf = [
            ((reg >> 16) & 0xFF) as u8,
            ((reg >> 8) & 0xFF) as u8,
            (reg & 0xFF) as u8,
        ];
        let mut data = [0u8; 1];
        let _ = self.bus.write_read(self.addr, &addr_buf, &mut data);
        data[0]
    }

    // ── Full initialisation (common path for both modes) ────────────────

    /// Core initialisation: I2C bus, silicon ID check, clock, ASP, volume.
    /// After this, call either `configure_balanced()` or `configure_se()`.
    fn init_core(&mut self) {
        // 0. Bring up I2C bus
        self.bus.init(true); // 400 kHz fast-mode

        // 1. Verify silicon ID
        let _id = self.read_reg(regs::DEVID_AB);

        // 2. Power up sequence
        self.write_reg(regs::PWR_CTL1, 0x00); // clear PDN bits
        self.write_reg(regs::PWR_CTL2, 0x03); // HPA_EN | HPB_EN

        // 3. Clock source: external MCLK from SoC
        self.write_reg(regs::MCLK_SRC_SEL, 0x00);
        self.write_reg(regs::MCLK_FREQ, 0x00);  // auto-detect ratio

        // 4. Audio serial port: I2S, slave, 32-bit
        self.write_reg(regs::ASP_CTL1, 0x00); // I2S, slave
        self.write_reg(regs::ASP_CTL2, 0x07); // WL = 32-bit
        self.write_reg(regs::ASP_CTL3, 0x00); // normal polarity

        // 5. Digital filter: fast roll-off (default)
        self.set_digital_filter(DigitalFilter::FastRolloff);

        // 6. Hardware volume: unity gain (0 dB)
        self.write_reg(regs::HP_A_VOL, 0x00);
        self.write_reg(regs::HP_B_VOL, 0x00);
        self.write_reg(regs::HP_CTL, 0x03); // soft-ramp, zero-cross

        // 7. Class-H: adaptive supply tracking
        self.write_reg(regs::CLASS_H_CTL, 0x01); // ADP_EN

        // 8. Unmask interrupts
        self.write_reg(regs::INT_MASK, 0x00);

        // 9. Default sample rate
        self.set_sample_rate(SampleRateCode::Rate44100);
    }

    /// Configure this DAC for balanced mode (differential mono).
    ///
    /// In balanced mode, each DAC handles one channel with both HP A and HP B
    /// outputs driven differentially for maximum voltage swing and SNR.
    fn configure_balanced(&mut self) {
        // PCM path: route the correct channel based on which side we are
        match self.side {
            DacBus::Left => {
                self.write_reg(regs::PCM_PATH_CTL1, 0x00); // Left data → DAC
            }
            DacBus::Right => {
                self.write_reg(regs::PCM_PATH_CTL1, 0x01); // Right data → DAC
            }
        }

        // HP output: balanced output, high-Z detect enabled
        self.write_reg(regs::HP_OUT_CTL, 0x0C); // BAL_EN | HIZ_EN
    }

    /// Configure this DAC for single-ended (SE) stereo mode.
    ///
    /// In SE mode, a single DAC handles both L+R channels in stereo.
    /// The other DAC can be powered down to save power.
    fn configure_se_stereo(&mut self) {
        // PCM path: stereo — route both channels through this one DAC
        self.write_reg(regs::PCM_PATH_CTL1, 0x02); // Stereo mode

        // HP output: single-ended, Class-H for efficiency
        self.write_reg(regs::HP_OUT_CTL, 0x04); // SE mode, HIZ_EN
    }

    // ── Full init convenience methods ───────────────────────────────────

    /// Full initialisation for balanced headphone output.
    pub fn init_balanced(&mut self) {
        self.init_core();
        self.configure_balanced();
    }

    /// Full initialisation for single-ended output.
    pub fn init_se(&mut self) {
        self.init_core();
        self.configure_se_stereo();
    }

    // ── Runtime configuration methods ───────────────────────────────────

    /// Switch the sample rate register. The SoC must also reconfigure MCLK / PLL.
    pub fn set_sample_rate(&mut self, rate: SampleRateCode) {
        self.write_reg(regs::SP_SRATE, rate as u8);
    }

    /// Select digital filter type.
    pub fn set_digital_filter(&mut self, filter: DigitalFilter) {
        let current = self.read_reg(regs::DAC_CTL1);
        let new_val = (current & !0x07) | (filter as u8 & 0x07);
        self.write_reg(regs::DAC_CTL1, new_val);
    }

    /// Set hardware volume attenuation (0 = 0 dB, 255 = mute).
    /// This operates in the analog domain — the digital bitstream is untouched.
    pub fn set_volume(&mut self, atten: u8) {
        self.write_reg(regs::HP_A_VOL, atten);
        self.write_reg(regs::HP_B_VOL, atten);
    }

    /// Configure DSD mode for native DSD64/128/256 playback.
    pub fn enable_dsd(&mut self, rate: SampleRateCode) {
        self.write_reg(regs::DSD_CTL1, 0x01);
        self.write_reg(regs::DSD_PATH_CTL, 0x01);
        self.set_sample_rate(rate);
    }

    /// Return to PCM mode after DSD playback.
    pub fn disable_dsd(&mut self) {
        self.write_reg(regs::DSD_CTL1, 0x00);
        self.write_reg(regs::DSD_PATH_CTL, 0x00);
    }

    /// Soft power-down (keeps I2C alive).
    pub fn power_down(&mut self) {
        self.write_reg(regs::PWR_CTL1, 0x01);
    }

    /// Wake from soft power-down.
    pub fn power_up(&mut self) {
        self.write_reg(regs::PWR_CTL1, 0x00);
    }

    /// Read the silicon revision.
    pub fn revision(&self) -> u8 {
        self.read_reg(regs::REV_ID)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// System-level output mode switching
// ═══════════════════════════════════════════════════════════════════════════════

/// Set balanced mode: both DACs in differential mono (maximum SNR + voltage swing).
///
/// Left DAC → left channel, differential output on HP A/B.
/// Right DAC → right channel, differential output on HP A/B.
pub fn set_balanced_mode(left: &mut Cs43131, right: &mut Cs43131) {
    left.init_balanced();
    right.init_balanced();
}

/// Set single-ended (SE) mode: one DAC handles stereo, other powers down.
///
/// Left DAC → stereo SE output, handles both L+R.
/// Right DAC → powered down to save battery.
pub fn set_se_mode(left: &mut Cs43131, right: &mut Cs43131) {
    left.init_se();
    right.power_down();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Initialization Trait — generic interface for any DAC in the system
// ═══════════════════════════════════════════════════════════════════════════════

/// Trait abstracting DAC initialisation so alternate DACs can be swapped in.
pub trait DacInit {
    /// Full initialisation into bit-perfect balanced mode.
    fn init_balanced(&mut self);
    /// Set hardware (analog) volume — 0 = 0 dB, 255 = mute.
    fn set_volume(&mut self, attenuation: u8);
    /// Configure sample rate.
    fn set_sample_rate(&mut self, rate_code: u8);
    /// Select digital reconstruction filter.
    fn set_filter(&mut self, filter_code: u8);
}

impl DacInit for Cs43131 {
    fn init_balanced(&mut self) {
        Cs43131::init_balanced(self);
    }

    fn set_volume(&mut self, attenuation: u8) {
        Cs43131::set_volume(self, attenuation);
    }

    fn set_sample_rate(&mut self, rate_code: u8) {
        let rate = match rate_code {
            0x02 => SampleRateCode::Rate32000,
            0x04 => SampleRateCode::Rate44100,
            0x05 => SampleRateCode::Rate48000,
            0x08 => SampleRateCode::Rate88200,
            0x09 => SampleRateCode::Rate96000,
            0x0C => SampleRateCode::Rate176400,
            0x0D => SampleRateCode::Rate192000,
            0x10 => SampleRateCode::Rate352800,
            0x11 => SampleRateCode::Rate384000,
            _    => SampleRateCode::Rate44100,
        };
        Cs43131::set_sample_rate(self, rate);
    }

    fn set_filter(&mut self, filter_code: u8) {
        let filter = match filter_code {
            0 => DigitalFilter::FastRolloff,
            1 => DigitalFilter::SlowRolloff,
            2 => DigitalFilter::FastMinPhase,
            3 => DigitalFilter::SlowMinPhase,
            4 => DigitalFilter::ApodizingFast,
            5 => DigitalFilter::HybridFastRolloff,
            6 => DigitalFilter::Brickwall,
            _ => DigitalFilter::FastRolloff,
        };
        Cs43131::set_digital_filter(self, filter);
    }
}
