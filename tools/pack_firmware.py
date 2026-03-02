#!/usr/bin/env python3
"""
pack_firmware.py — Package the compiled .bin into an RKnanoFW image for the
FiiO Snowsky Echo Mini (Rockchip RKNanoD).

Usage:
    python tools/pack_firmware.py target/thumbv7m-none-eabi/release/echo-mini-os.bin

Output:
    RKnanoFW.bin  (in current directory)

Image format (see src/boot/image.rs):
    Offset  Size  Description
    0x000   8     Magic "RKnanoFW"
    0x008   4     Format version (1)
    0x00C   4     Payload size
    0x010   4     CRC-32 of payload
    0x014   4     Load address   (0x00000000 — SRAM base)
    0x018   4     Entry address  (0x00000200 — after header)
    0x01C   4     Flags (0 = raw)
    0x020   4     Chip ID (0x4E414E44 = "NAND")
    0x024   12    Reserved
    0x030   464   Padding
    0x200   ...   Payload (.bin — vector table + application)
"""

import struct
import sys
import os

# ── Constants ────────────────────────────────────────────────────────────────
MAGIC = b"RKnanoFW"
FORMAT_VERSION = 1
LOAD_ADDR  = 0x00000000   # SRAM base (RKNanoD)
ENTRY_ADDR = 0x00000200   # Vector table starts after 512-byte header
HEADER_SIZE = 512          # 0x200 bytes — matches linker.ld fw_header region
CHIP_ID    = 0x4E414E44   # "NAND" in little-endian ASCII
OUTPUT_NAME = "RKnanoFW.bin"


def crc32(data: bytes) -> int:
    """ISO 3309 CRC-32 (same as zlib.crc32 & 0xFFFFFFFF)."""
    import zlib
    return zlib.crc32(data) & 0xFFFFFFFF


def pack(bin_path: str, output_path: str = OUTPUT_NAME):
    if not os.path.isfile(bin_path):
        print(f"Error: '{bin_path}' not found.")
        sys.exit(1)

    payload = open(bin_path, "rb").read()
    payload_size = len(payload)
    checksum = crc32(payload)

    print(f"[pack_firmware] Input : {bin_path}")
    print(f"[pack_firmware] Size  : {payload_size} bytes ({payload_size / 1024:.1f} KB)")
    print(f"[pack_firmware] CRC-32: 0x{checksum:08X}")
    print(f"[pack_firmware] Load  : 0x{LOAD_ADDR:08X}")
    print(f"[pack_firmware] Entry : 0x{ENTRY_ADDR:08X}")
    print(f"[pack_firmware] Chip  : 0x{CHIP_ID:08X} (RKNanoD)")

    # ── Build 512-byte header ────────────────────────────────────────────
    header = bytearray(HEADER_SIZE)
    struct.pack_into("<8s", header, 0x000, MAGIC)
    struct.pack_into("<I",  header, 0x008, FORMAT_VERSION)
    struct.pack_into("<I",  header, 0x00C, payload_size)
    struct.pack_into("<I",  header, 0x010, checksum)
    struct.pack_into("<I",  header, 0x014, LOAD_ADDR)
    struct.pack_into("<I",  header, 0x018, ENTRY_ADDR)
    struct.pack_into("<I",  header, 0x01C, 0)         # Flags: 0 = raw
    struct.pack_into("<I",  header, 0x020, CHIP_ID)
    # 0x024–0x1FF reserved (already zeroed)

    # ── Write output ─────────────────────────────────────────────────────
    with open(output_path, "wb") as f:
        f.write(header)
        f.write(payload)

    total = HEADER_SIZE + payload_size
    print(f"[pack_firmware] Output: {output_path} ({total} bytes)")
    print("[pack_firmware] Done.")


def main():
    if len(sys.argv) < 2:
        print("Usage: python pack_firmware.py <path-to-echo-mini-os.bin>")
        print()
        print("  Wraps the raw binary into RKnanoFW.bin for the Echo Mini")
        print("  (Rockchip RKNanoD) bootloader.")
        sys.exit(1)

    bin_path = sys.argv[1]
    output = sys.argv[2] if len(sys.argv) > 2 else OUTPUT_NAME
    pack(bin_path, output)


if __name__ == "__main__":
    main()
