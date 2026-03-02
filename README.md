# Echo Mini OS

Bare-metal Rust operating system for the **FiiO Snowsky Echo Mini DAP**.  
100% Rust, `no_std`, `no_main` — replaces the stock firmware entirely.

## Architecture

```
src/
├── main.rs              # Entry point (_entry → kernel_main), panic handler
├── lib.rs               # Crate root — re-exports all modules
├── hal/                 # Hardware Abstraction Layer (Ingenic X1000)
│   ├── mmio.rs          # Volatile register access, SoC base addresses
│   ├── interrupt.rs     # INTC controller, critical-section impl
│   ├── i2c.rs           # I2C driver (embedded-hal compliant)
│   ├── i2s.rs           # I2S/AIC controller for audio output
│   ├── spi.rs           # SPI/SSI driver for LCD
│   ├── dma.rs           # PDMA controller, descriptor chains
│   └── gpio.rs          # GPIO ports A–D
├── mem/
│   ├── allocator.rs     # Buddy allocator (#[global_allocator])
│   └── dma_buffer.rs    # Zero-copy ping-pong DMA ring buffer
├── audio/
│   ├── cs43131.rs       # Cirrus Logic CS43131 dual-DAC driver
│   └── engine.rs        # DMA-driven audio pipeline (ISR + main-loop)
├── clock/
│   └── pll.rs           # APLL config for 44.1/48 kHz MCLK families
├── display/
│   ├── lcd.rs           # ST7789V LCD init (SPI 4-wire)
│   └── framebuffer.rs   # RGB565 framebuffer + embedded-graphics DrawTarget
├── input/
│   └── buttons.rs       # 5-button matrix with ISR + debounce
├── ui/
│   └── cassette.rs      # Retro cassette-tape UI (embedded-graphics)
└── boot/
    └── image.rs         # Firmware image header & CRC-32

tools/
└── pack_firmware.py     # Wraps .bin → HIFIEC001.img for recovery flash
```

## Building

Requires Rust nightly (for `build-std`, `alloc_error_handler`, inline asm):

```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly

# Build for MIPS (default)
cargo +nightly build --release

# Build for ARM (Rockchip alternative)
cargo +nightly build --release --target thumbv7em-none-eabihf --features soc-arm-rockchip --no-default-features
```

## Packaging

```bash
# Convert ELF → raw binary
rust-objcopy -O binary target/mipsel-unknown-none/release/echo-mini-os firmware.bin

# Wrap into recovery image
python tools/pack_firmware.py firmware.bin
# → produces HIFIEC001.img
```

## Audio Pipeline

```
SD Card → Decoder → CPU fills inactive half-buffer
                        ↓
              DMA Ring Buffer (uncached DRAM)
             [Half A] ←→ [Half B]  (ping-pong)
                        ↓
              DMA → I2S/AIC peripheral
                        ↓
              CS43131 L (balanced) + CS43131 R (balanced)
                        ↓
                  Headphone output (bit-perfect)
```

**Zero digital-domain processing**: Volume is hardware-analog (CS43131 `HP_A_VOL`/`HP_B_VOL`).  
**Clock families**: APLL switches between 22.5792 MHz (44.1k) and 24.5760 MHz (48k).

## License

MIT / Apache-2.0 dual license.
