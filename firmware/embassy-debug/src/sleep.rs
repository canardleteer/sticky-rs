//! Deep sleep entry and RTC-persistent scene resume.
//!
//! Wake is GPIO6 (`ext1` ANY_LOW). Latch stays high. Do not
//! [`Latch::release`].

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_debug::{
    classify_resume_hold, format_sleeping, ResumeHold, Scene, LINE_CAPACITY, PAGE_DOWN_RESUME_MS,
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use embedded_hal::delay::DelayNs;
use esp_hal::delay::Delay;
use esp_hal::gpio::{
    Event as GpioEvent, Input, InputConfig, Level, Output, OutputConfig, Pull, WakeupConfig,
};
use esp_hal::rtc_cntl::sleep::{LowPower, RtcSleepConfig};
use esp_hal::rtc_cntl::{reset_reason, SocResetReason};
use esp_hal::system::Cpu;
use esp_println::println;
use seeed_reterminal_sticky::display::PageRotation;
use seeed_reterminal_sticky::power::Latch;

/// Display task: paint the sleep card, park the panel, then signal.
pub(crate) static SLEEP_REQUEST: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Panel rail is off after `0x10`.
pub(crate) static PANEL_PARKED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// GPIO6 is released, listening, and on the low-power wake path.
pub(crate) static WAKE_ARMED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Set before [`SLEEP_REQUEST`]. Other tasks poll this; the Signal is
/// single-waiter (the display task).
static SLEEPING: AtomicBool = AtomicBool::new(false);

/// Ask the display to park, and let other tasks see the request.
pub(crate) fn request_sleep() {
    SLEEPING.store(true, Ordering::Release);
    SLEEP_REQUEST.signal(());
}

/// True after [`request_sleep`]. Safe to poll from any task.
pub(crate) fn is_requested() -> bool {
    SLEEPING.load(Ordering::Acquire)
}

const SNAP_MAGIC: u32 = 0x534C5031;

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut SNAP_MAGIC_CELL: u32 = 0;
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut SNAP_PACKED: u32 = 0;

/// Scene and splash rotation to restore after a deep-sleep wake.
#[derive(Clone, Copy)]
pub(crate) struct SleepSnap {
    pub scene: Scene,
    pub rotation: PageRotation,
}

fn rotation_byte(rotation: PageRotation) -> u8 {
    match rotation {
        PageRotation::Portrait0 => 0,
        PageRotation::Portrait180 => 1,
        PageRotation::Landscape0 => 2,
        PageRotation::Landscape180 => 3,
    }
}

fn rotation_from_byte(byte: u8) -> Option<PageRotation> {
    match byte {
        0 => Some(PageRotation::Portrait0),
        1 => Some(PageRotation::Portrait180),
        2 => Some(PageRotation::Landscape0),
        3 => Some(PageRotation::Landscape180),
        _ => None,
    }
}

/// Store the card that should come back after the 1 s resume hold.
pub(crate) fn persist(scene: Scene, rotation: PageRotation) {
    let packed = u32::from(scene.persist_byte()) | (u32::from(rotation_byte(rotation)) << 8);
    unsafe {
        SNAP_PACKED = packed;
        SNAP_MAGIC_CELL = SNAP_MAGIC;
    }
}

fn load_snap() -> Option<SleepSnap> {
    let (magic, packed) = unsafe { (SNAP_MAGIC_CELL, SNAP_PACKED) };
    if magic != SNAP_MAGIC {
        return None;
    }
    let scene = Scene::from_persist_byte((packed & 0xff) as u8)?;
    let rotation = rotation_from_byte(((packed >> 8) & 0xff) as u8)?;
    Some(SleepSnap { scene, rotation })
}

/// `Some` only after a deep-sleep reset with a valid snap.
pub(crate) fn resume_snap() -> Option<SleepSnap> {
    if reset_reason(Cpu::ProCpu) != Some(SocResetReason::CoreDeepSleep) {
        return None;
    }
    load_snap()
}

/// Wait until Page Down has been low for 1 s, or it goes high (abort).
pub(crate) fn wait_resume_hold(page_down: &Input<'_>, delay: &mut Delay) -> ResumeHold {
    let mut held = 0u32;
    loop {
        let still_low = page_down.is_low();
        match classify_resume_hold(held, still_low) {
            ResumeHold::Waiting => {
                delay.delay_ms(20);
                held = held.saturating_add(20);
                if held > PAGE_DOWN_RESUME_MS {
                    held = PAGE_DOWN_RESUME_MS;
                }
            }
            other => return other,
        }
    }
}

/// Listen for a low Page Down on the RTC path, then deep sleep.
pub(crate) fn arm_page_down_and_sleep(
    page_down: &mut Input<'static>,
    lpwr: LowPower<'static>,
) -> ! {
    page_down.listen(GpioEvent::LowLevel);
    page_down
        .apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("GPIO6 has an RTC wake path");
    enter_deep_sleep(lpwr)
}

/// Hold already-driven pads, print `sleeping`, then `sleep_deep`.
pub(crate) fn enter_deep_sleep(mut lpwr: LowPower<'static>) -> ! {
    let mut buf = [0u8; LINE_CAPACITY];
    if let Ok(line) = format_sleeping(&mut buf) {
        println!("{line}");
    }
    lpwr.sleep_deep(RtcSleepConfig::deep())
}

/// Drive latch pins high and hold them across sleep.
pub(crate) fn hold_latch(latch: Latch<Output<'static>, Output<'static>>) {
    let (mut hold, mut lock) = latch.release_ownership_only();
    hold.set_high();
    lock.set_high();
    hold.set_pad_hold(true);
    lock.set_pad_hold(true);
    core::mem::forget(hold);
    core::mem::forget(lock);
}

pub(crate) fn hold_output(pin: &mut Output<'static>) {
    pin.set_pad_hold(true);
}

pub(crate) fn hold_input(pin: &mut Input<'static>) {
    pin.set_pad_hold(true);
}

/// After the sleep card, wait for Page Down high, then arm `ext1`.
pub(crate) async fn wait_release_and_arm(page_down: &mut Input<'static>) {
    if page_down.is_low() {
        page_down.wait_for_high().await;
    }
    Timer::after(Duration::from_millis(20)).await;
    page_down.listen(GpioEvent::LowLevel);
    page_down
        .apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("GPIO6 has an RTC wake path");
    WAKE_ARMED.signal(());
}

/// Default Page Down input (external pull-up plus MCU pull-up).
pub(crate) fn page_down_input(pin: esp_hal::peripherals::GPIO6<'static>) -> Input<'static> {
    Input::new(pin, InputConfig::default().with_pull(Pull::Up))
}

/// Park `EPD_EN` low when aborting a wake without touching the panel.
pub(crate) fn park_epd_en_low(pin: esp_hal::peripherals::GPIO47<'static>) {
    let mut epd = Output::new(pin, Level::Low, OutputConfig::default());
    hold_output(&mut epd);
    core::mem::forget(epd);
}
