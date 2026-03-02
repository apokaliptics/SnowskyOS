// ═══════════════════════════════════════════════════════════════════════════════
// hal/i2c.rs — I2C controller driver for Rockchip RKNanoD
// Used for CS43131 register control (addresses 0x30 / 0x32)
//
// The RKNanoD uses a DesignWare-compatible I2C controller.
// ═══════════════════════════════════════════════════════════════════════════════
use crate::hal::mmio;

/// I2C controller register offsets (DesignWare I2C on RKNanoD).
const I2C_CON:    usize = 0x00; // Control register
const I2C_TAR:    usize = 0x04; // Target address
const I2C_DATACMD:usize = 0x10; // Data / Command buffer
const I2C_SSHCNT: usize = 0x14; // Standard-speed SCL high count
const I2C_SSLCNT: usize = 0x18; // Standard-speed SCL low count
const I2C_FSHCNT: usize = 0x1C; // Fast-mode SCL high count
const I2C_FSLCNT: usize = 0x20; // Fast-mode SCL low count
const I2C_INTST:  usize = 0x2C; // Interrupt status
const I2C_INTMSK: usize = 0x30; // Interrupt mask
const I2C_CLR_INT:usize = 0x40; // Clear combined interrupt
const I2C_ENABLE: usize = 0x6C; // Enable register
const I2C_STATUS: usize = 0x70; // Bus status
const I2C_TXFLR:  usize = 0x74; // TX FIFO level
const I2C_RXFLR:  usize = 0x78; // RX FIFO level
const I2C_TX_ABRT:usize = 0x80; // TX abort source
const I2C_CLR_TX_ABRT: usize = 0x54; // Clear TX abort

// Status bits
const STATUS_TFE: u32 = 1 << 2;  // TX FIFO empty
const STATUS_TFNF: u32 = 1 << 1; // TX FIFO not full
const STATUS_RFNE: u32 = 1 << 3; // RX FIFO not empty
const STATUS_BUSY: u32 = 1 << 5; // Bus activity

// CON register bits
const CON_MASTER: u32     = 1 << 0;  // Master mode
const CON_SPEED_STD: u32  = 1 << 1;  // Standard speed (100 kHz)
const CON_SPEED_FAST: u32 = 2 << 1;  // Fast mode (400 kHz)
const CON_RESTART_EN: u32 = 1 << 5;  // Restart enable
const CON_SLAVE_DIS: u32  = 1 << 6;  // Slave disable

// DATACMD bits
const DATACMD_READ: u32 = 1 << 8;  // Read command
const DATACMD_STOP: u32 = 1 << 9;  // Issue STOP after this byte

/// Represents an I2C bus backed by a specific hardware controller.
pub struct I2cBus {
    base: usize,
}

impl I2cBus {
    /// Construct a new I2C bus from its MMIO base address.
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    /// Initialise the I2C controller for standard-mode (100 kHz) or fast-mode
    /// (400 kHz) operation.
    pub fn init(&self, fast_mode: bool) {
        // Disable first
        mmio::write32(self.base + I2C_ENABLE, 0);

        // Master mode, restart enable, slave disable, speed select
        let speed = if fast_mode { CON_SPEED_FAST } else { CON_SPEED_STD };
        let con = CON_MASTER | speed | CON_RESTART_EN | CON_SLAVE_DIS;
        mmio::write32(self.base + I2C_CON, con);

        // SCL timing (assuming 50 MHz APB clock)
        if fast_mode {
            mmio::write32(self.base + I2C_FSHCNT, 60);  // ~400 kHz
            mmio::write32(self.base + I2C_FSLCNT, 65);
        } else {
            mmio::write32(self.base + I2C_SSHCNT, 240); // ~100 kHz
            mmio::write32(self.base + I2C_SSLCNT, 260);
        }

        // Clear any pending interrupts
        let _ = mmio::read32(self.base + I2C_CLR_INT);

        // Enable
        mmio::write32(self.base + I2C_ENABLE, 1);
    }

    /// Set the target slave address (7-bit).
    pub fn set_target(&self, addr: u8) {
        // Disable before changing target address
        mmio::write32(self.base + I2C_ENABLE, 0);
        mmio::write32(self.base + I2C_TAR, addr as u32 & 0x7F);
        mmio::write32(self.base + I2C_ENABLE, 1);
    }

    /// Write a buffer of bytes to the target.
    pub fn write(&self, addr: u8, data: &[u8]) -> Result<(), I2cError> {
        self.set_target(addr);
        self.wait_idle()?;

        for (i, &byte) in data.iter().enumerate() {
            let stop = if i == data.len() - 1 { DATACMD_STOP } else { 0 };
            self.wait_tx_not_full()?;
            mmio::write32(self.base + I2C_DATACMD, byte as u32 | stop);
        }

        self.wait_idle()?;
        self.check_abrt()?;
        Ok(())
    }

    /// Write a register address then read `buf.len()` bytes back.
    pub fn write_read(&self, addr: u8, reg: &[u8], buf: &mut [u8]) -> Result<(), I2cError> {
        self.set_target(addr);
        self.wait_idle()?;

        // Write phase — send register address bytes (no STOP)
        for &byte in reg {
            self.wait_tx_not_full()?;
            mmio::write32(self.base + I2C_DATACMD, byte as u32);
        }

        // Read phase — issue READ commands
        for i in 0..buf.len() {
            let stop = if i == buf.len() - 1 { DATACMD_STOP } else { 0 };
            self.wait_tx_not_full()?;
            mmio::write32(self.base + I2C_DATACMD, DATACMD_READ | stop);
        }

        // Collect
        for byte in buf.iter_mut() {
            self.wait_rx_ready()?;
            *byte = mmio::read32(self.base + I2C_DATACMD) as u8;
        }

        self.wait_idle()?;
        self.check_abrt()?;
        Ok(())
    }

    // ── Private helpers ─────────────────────────────────────────────────

    fn wait_idle(&self) -> Result<(), I2cError> {
        let mut timeout = 100_000u32;
        while mmio::read32(self.base + I2C_STATUS) & STATUS_BUSY != 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(I2cError::Timeout);
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    fn wait_tx_not_full(&self) -> Result<(), I2cError> {
        let mut timeout = 100_000u32;
        while mmio::read32(self.base + I2C_STATUS) & STATUS_TFNF == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(I2cError::Timeout);
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    fn wait_rx_ready(&self) -> Result<(), I2cError> {
        let mut timeout = 100_000u32;
        while mmio::read32(self.base + I2C_STATUS) & STATUS_RFNE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(I2cError::Timeout);
            }
            core::hint::spin_loop();
        }
        Ok(())
    }

    fn check_abrt(&self) -> Result<(), I2cError> {
        let abrt = mmio::read32(self.base + I2C_TX_ABRT);
        if abrt != 0 {
            // Clear the abort
            let _ = mmio::read32(self.base + I2C_CLR_TX_ABRT);
            return Err(I2cError::Nack);
        }
        Ok(())
    }
}

// ── embedded-hal I2c trait implementation ─────────────────────────────────────
impl embedded_hal::i2c::ErrorType for I2cBus {
    type Error = I2cError;
}

impl embedded_hal::i2c::I2c for I2cBus {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        for op in operations {
            match op {
                embedded_hal::i2c::Operation::Write(data) => {
                    self.write(address, data)?;
                }
                embedded_hal::i2c::Operation::Read(buf) => {
                    self.write_read(address, &[], buf)?;
                }
            }
        }
        Ok(())
    }
}

/// I2C bus errors.
#[derive(Debug, Clone, Copy)]
pub enum I2cError {
    Timeout,
    Nack,
    BusError,
}

impl embedded_hal::i2c::Error for I2cError {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        match self {
            I2cError::Timeout => embedded_hal::i2c::ErrorKind::Other,
            I2cError::Nack => embedded_hal::i2c::ErrorKind::NoAcknowledge(
                embedded_hal::i2c::NoAcknowledgeSource::Unknown,
            ),
            I2cError::BusError => embedded_hal::i2c::ErrorKind::Bus,
        }
    }
}
