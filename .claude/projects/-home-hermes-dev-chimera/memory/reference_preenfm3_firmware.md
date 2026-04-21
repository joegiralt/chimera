---
name: PreenFM3 firmware architecture reference
description: Detailed hardware register config, pin map, DMA assignments, clock tree, audio/display/controls architecture from Ixox/preenfm3 C firmware — essential reference for Rust reimplementation
type: reference
---

Source: github.com/Ixox/preenfm3

## Chip Correction
Actual chip is **STM32H753** (2MB flash), not H750 (128KB). Linker scripts and CMSIS headers reference stm32h753xx. The H753 explains the 1920K flash region at 0x08020000.

## Clock Tree
- HSE: 8 MHz external oscillator
- PLL1: M=1, N=120, P=2 → 480 MHz SYSCLK; AHB /2=240MHz; APB /2=120MHz; Flash 4WS
- PLL2: M=1, N=40, P=5 → 64 MHz for SPI1, SPI2, USART1
- PLL3: M=1, N=46, P=3 → SAI audio MCLK → actual rate 47916 Hz
- USB: HSI48 (48 MHz internal)

## Memory Layout
```
FLASH:    0x08020000, 1920K  (bootloader at 0x08000000, 128K)
DTCMRAM:  0x20000000, 128K  (.data, .bss, stack 0x400, heap 0x200)
RAM_D1:   0x24000000, 512K  (TFT framebuffer, font bitmaps)
RAM_D2:   0x30000000, 128K  (SAI DMA audio buffers — must be here for DMA1)
RAM_D2B:  0x30020000, 128K  (misc)
RAM_D3:   0x38000000, 64K
ITCMRAM:  0x00000004, 63K
```

MPU: RAM_D1 and RAM_D2 use write-through caching (cacheable, not bufferable, not shareable) for DMA coherence.

## Audio (SAI + DMA)
- SAI1_A: Master TX, async, 48kHz, I2S, 32-bit stereo → DAC 1
- SAI1_B: Slave TX, sync to SAI1_A → DAC 2  
- SAI2_A: Slave TX, ext sync to SAI1_A → DAC 3
- SAI pins: MCLK=PE2, FS=PE4, SCK=PE5, SD_A=PE6, SD_B=PE3, SAI2_SD_A=PD11
- DMA: DMA1_Stream0/1/2, circular, word-aligned, FIFO 1/4, IRQ priority 3
- Buffers: 128 int32_t each (64 stereo pairs), double-buffered via half/complete callbacks
- BLOCK_SIZE=32 stereo pairs per render call
- MIDI decoded inside SAI callback

## Display (ILI9341 via SPI1)
- SPI1: Master, 8-bit, CPOL=LOW, CPHA=1EDGE, prescaler /2 = 32MHz
- Pins: SCK=PA5, MISO=PA6, MOSI=PA7 (AF5_SPI1)
- Control: DC=PD8, RESET=PD9, CS=PD10
- DMA: DMA2_Stream0, SPI1_TX, normal mode, byte-aligned, IRQ priority 2
- Backlight: TIM1_CH2 PWM on PE11, prescaler 240, period 100
- MADCTL: MX | BGR (X-mirror, BGR color order), portrait 240x320
- Three-layer compositing using DMA2D hardware:
  - tftForeground: A8 alpha masks (chars, oscilloscope)
  - tftBackground: RGB565 background tiles
  - tftMemory: 240×320 RGB565 framebuffer (composited output)
- DMA2D blends A8 foreground + RGB565 background → framebuffer
- Dirty-region tracking: 8 screen parts, 20ms min between SPI pushes
- TFT health check: reads power mode reg 0x0A, re-inits if bad

## Controls (HC165 Shift Registers)
- Pins (LQFP144/176): DATA=PF2, LOAD=PF1, CLK=PF0
- Pins (LQFP100): DATA=PA0, LOAD=PA1, CLK=PA2
- Board version detected via PD0-3 with pull-ups
- Protocol: pulse LOAD low→high, then clock out 24-32 bits
- 6 encoders (12 bits) + 12-18 buttons
- Polled at 500 Hz from SysTick
- Encoder state: 4-bit history with direction-locking, acceleration (tickSpeed capped at 12)
- Encoder pins: {17,18,15,16,9,10,20,19,14,13,12,11}
- Button pins: {23,21,4,24,3,2,22,5,6,7,8,1,32,31,30,28,27,29}

## MIDI
- USART1: 31250 baud, 8N1, FIFO enabled, pins PB6(TX)/PB7(RX), AF7, IRQ priority 1
- Ring buffer usartBufferIn (64 bytes), IRQ-driven RX/TX
- USB MIDI: OTG_FS, custom MIDI class, EP OUT=0x01, IN=0x81, 64-byte packets
- Both sources decoded in SAI DMA callback

## SD Card (SPI2)
- SPI2: Master, 8-bit, CPOL=LOW, CPHA=2EDGE, prescaler /4
- Pins: SCK=PA9, MISO=PB14, MOSI=PB15 (AF5_SPI2), CS=PE12
- DMA: DMA2_Stream1(RX)/Stream2(TX), IRQ priority 4
- FatFS filesystem

## DMA Summary
| Stream | Peripheral | Priority | Mode |
|---|---|---|---|
| DMA1_Stream0 | SAI1_A | VERY_HIGH | Circular |
| DMA1_Stream1 | SAI1_B | VERY_HIGH | Circular |
| DMA1_Stream2 | SAI2_A | VERY_HIGH | Circular |
| DMA2_Stream0 | SPI1_TX (TFT) | HIGH | Normal |
| DMA2_Stream1 | SPI2_RX (SD) | HIGH | Normal |
| DMA2_Stream2 | SPI2_TX (SD) | HIGH | Normal |

## IRQ Priorities
1 = SPI1 (TFT), USART1 (MIDI)
2 = DMA2_Stream0 (TFT DMA complete)
3 = DMA1 (SAI audio)
4 = DMA2 (SD card)

## Key Constants
- BLOCK_SIZE = 32
- MAX_NUMBER_OF_VOICES = 16
- NUMBER_OF_TIMBRES = 6
- NUMBER_OF_OPERATORS = 6
- PREENFM_FREQUENCY = 47916.0
