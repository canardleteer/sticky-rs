//! PDM RX energy on UART (`--features mic` only).
//!
//! Community recipe: 16 kHz, 16-bit, left (mono). AI Voice dumps PCM
//! windows and leaves the buzzer off so a through-hole tone is unmixed.

use crate::{emit, now_ms, TONE_CAPTURE};

use core::sync::atomic::Ordering;

use embassy_debug::{
    format_mic_pcm_header, format_mic_pcm_row, pcm_energy, Event, LINE_CAPACITY, MIC_REPORT_MS,
    PCM_DUMP_NO_TONE_HZ, PCM_ROW_SAMPLES, PCM_WINDOW_SAMPLES,
};
use embassy_time::{Duration, Timer};
use embedded_hal::delay::DelayNs;
use esp_hal::dma_rx_buffer;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::i2s::master::{I2s, PdmConfig, PdmRxConfig, PdmSlotMode};
use esp_hal::peripherals::{DMA_CH0, GPIO19, GPIO20, GPIO38, I2S0};
use esp_hal::time::Rate;
use esp_println::println;
use seeed_reterminal_sticky::power::Latched;
use seeed_reterminal_sticky::rails::{Enabled, MicRail, Rail};

/// Community hold before enabling the load switch (GPIO38 can float).
const RAIL_HOLD_MS: u32 = 150;

const WINDOW_BYTES: usize = PCM_WINDOW_SAMPLES * 2;

/// Drive GPIO38 low, wait, then enable. Caller keeps the rail alive.
pub fn enable_rail<D: DelayNs>(
    pin: GPIO38<'static>,
    latch: &Latched,
    delay: &mut D,
) -> MicRail<Output<'static>, Enabled> {
    let rail: MicRail<_, _> =
        Rail::new(Output::new(pin, Level::Low, OutputConfig::default()), latch)
            .expect("driving the mic rail cannot fail");
    delay.delay_ms(RAIL_HOLD_MS);
    rail.enable(delay)
        .expect("driving the mic rail cannot fail")
}

/// Read PDM windows and print `mic rms=… peak=…`.
#[embassy_executor::task]
pub async fn mic_task(
    i2s: I2S0<'static>,
    dma: DMA_CH0<'static>,
    clk: GPIO19<'static>,
    din: GPIO20<'static>,
    rail: MicRail<Output<'static>, Enabled>,
) {
    let rx_cfg = PdmRxConfig::new_pcm_default(Rate::from_hz(16_000), PdmSlotMode::Mono);
    let Ok(i2s) = I2s::new_pdm(i2s, dma, PdmConfig::rx_only(rx_cfg)) else {
        println!("{LOG}: pdm config failed");
        return;
    };
    let mut rx = i2s.into_async().i2s_rx.with_clk(clk).with_din(din).build();

    let Ok(mut buffer) = dma_rx_buffer!(WINDOW_BYTES) else {
        println!("{LOG}: pdm dma buffer failed");
        return;
    };
    println!("{LOG}: pdm rx 16kHz mono left; AI Voice dumps pcm (buzzer off)");

    loop {
        if crate::sleep::is_requested() {
            let disabled = rail.disable().expect("driving the mic rail cannot fail");
            let mut pin = disabled.release();
            crate::sleep::hold_output(&mut pin);
            core::mem::forget(pin);
            loop {
                Timer::after(Duration::from_secs(3_600)).await;
            }
        }
        buffer.set_length(WINDOW_BYTES);
        let dump = TONE_CAPTURE.load(Ordering::Relaxed) > 0;
        match rx.read(buffer) {
            Ok(transfer) => {
                let (status, next, filled) = transfer.wait_async().await;
                rx = next;
                buffer = filled;
                if status.is_ok() {
                    emit_window(buffer.as_slice(), dump);
                    if dump {
                        let _ = TONE_CAPTURE.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
            Err((_, next, returned)) => {
                rx = next;
                buffer = returned;
                println!("{LOG}: pdm read failed");
            }
        }
        if TONE_CAPTURE.load(Ordering::Relaxed) == 0 {
            Timer::after(Duration::from_millis(u64::from(MIC_REPORT_MS))).await;
        }
    }
}

fn emit_window(bytes: &[u8], dump_pcm: bool) {
    let n = bytes.len() / 2;
    if n == 0 {
        return;
    }
    let mut samples = [0i16; PCM_WINDOW_SAMPLES];
    for (i, chunk) in bytes.chunks_exact(2).take(n).enumerate() {
        samples[i] = i16::from_le_bytes([chunk[0], chunk[1]]);
    }
    let samples = &samples[..n];
    let (rms, peak) = pcm_energy(samples);
    emit(Event::Mic {
        t_ms: now_ms(),
        rms,
        peak,
    });
    if !dump_pcm {
        return;
    }
    let mut buf = [0u8; LINE_CAPACITY];
    if let Ok(line) = format_mic_pcm_header(now_ms(), PCM_DUMP_NO_TONE_HZ, n, &mut buf) {
        println!("{line}");
    }
    let mut offset = 0;
    while offset < n {
        let end = core::cmp::min(offset + PCM_ROW_SAMPLES, n);
        if let Ok(line) = format_mic_pcm_row(offset, &samples[offset..end], &mut buf) {
            println!("{line}");
        }
        offset = end;
    }
}

const LOG: &str = "embassy-debug";
