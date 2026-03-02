// ═══════════════════════════════════════════════════════════════════════════════
// boot/image.rs — Constants and structures for the RKnanoFW firmware format
//
// The Echo Mini's bootloader (RKNanoD mask ROM + RKNano SDK 1.0) expects
// a firmware image in the RKnanoFW format. This module defines the header
// structure so the Python packaging tool and any future Rust-native flasher
// can produce valid images.
// ═══════════════════════════════════════════════════════════════════════════════

/// Magic bytes at the start of an RKnanoFW image.
pub const MAGIC: [u8; 8] = *b"RKnanoFW";

/// Current firmware format version.
pub const FORMAT_VERSION: u32 = 1;

/// Header size in bytes (512 bytes = 0x200, matches linker.ld RKnanoFW header region).
pub const HEADER_SIZE: usize = 512;

/// RKnanoFW firmware image header — 512 bytes (0x200), little-endian.
///
/// Layout:
/// ```text
/// Offset  Size  Field
/// 0x000   8     Magic ("RKnanoFW")
/// 0x008   4     Format version
/// 0x00C   4     Payload size (bytes, excl. header)
/// 0x010   4     CRC-32 of payload
/// 0x014   4     Load address (SRAM base, 0x00000000)
/// 0x018   4     Entry point (vector table, 0x00000200)
/// 0x01C   4     Flags (0x00 = raw, 0x01 = compressed)
/// 0x020   4     Chip ID (RKNanoD = 0x4E414E44)
/// 0x024   4     Reserved 1
/// 0x028   4     Reserved 2
/// 0x02C   4     Reserved 3
/// 0x030   464   Padding (zero-filled)
/// 0x200   ...   Payload (raw .bin — vector table + application)
/// ```
#[repr(C, packed)]
pub struct FirmwareHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub payload_size: u32,
    pub crc32: u32,
    pub load_addr: u32,
    pub entry_addr: u32,
    pub flags: u32,
    pub chip_id: u32,
    pub _reserved: [u8; 12],
    pub _padding: [u8; 464],
}

/// RKNanoD chip identifier ("NAND" in little-endian ASCII = 0x4E414E44).
pub const CHIP_ID_RKNANOD: u32 = 0x4E41_4E44;

/// SRAM load address (beginning of SRAM).
pub const LOAD_ADDR: u32 = 0x0000_0000;

/// Entry point address (right after the 512-byte header → vector table).
pub const ENTRY_ADDR: u32 = 0x0000_0200;

impl FirmwareHeader {
    /// Validate the magic, version, and chip ID fields.
    pub fn is_valid(&self) -> bool {
        self.magic == MAGIC
            && self.version == FORMAT_VERSION
            && self.chip_id == CHIP_ID_RKNANOD
    }
}

/// Compute a simple CRC-32 (ISO 3309) of a byte slice.
/// Used by the Python packaging tool and verified by the bootloader stub.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}
