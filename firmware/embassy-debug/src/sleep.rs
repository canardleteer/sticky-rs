//! Deep sleep, latch power-off, and RTC-persistent scene resume.
//!
//! Sleep keeps the latch high and wakes on GPIO5 (`ext1` ANY_LOW).
//! Power-off is [`Latch::release`] after the panel parks.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_debug::{
    classify_resume_hold, format_poweroff, format_sleeping, ResumeHold, Scene, LINE_CAPACITY,
    PAGE_UP_RESUME_MS,
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

/// Display task: paint Ferris, park the panel, then signal.
pub(crate) static SLEEP_REQUEST: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Display task: paint Ferris, park the panel, then drop the latch.
pub(crate) static POWER_OFF_REQUEST: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Panel rail is off after [`ssd1677_gray4::Ssd1677::sleep`].
pub(crate) static PANEL_PARKED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Panel rail is off and the latch may drop.
pub(crate) static POWER_OFF_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// GPIO5 is released, listening, and on the low-power wake path.
pub(crate) static WAKE_ARMED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Set before [`SLEEP_REQUEST`]. Other tasks poll this; the Signal is
/// single-waiter (the display task).
static SLEEPING: AtomicBool = AtomicBool::new(false);
/// Set before [`POWER_OFF_REQUEST`].
static POWERING_OFF: AtomicBool = AtomicBool::new(false);
/// Panel is in standby (`EPD_EN` high, clock off). Page Up 1 s leaves.
static IN_STANDBY: AtomicBool = AtomicBool::new(false);

/// Ask the display to park for MCU deep sleep.
pub(crate) fn request_sleep() {
    IN_STANDBY.store(false, Ordering::Release);
    SLEEPING.store(true, Ordering::Release);
    SLEEP_REQUEST.signal(());
}

/// Ask the display to park, then drop the latch.
pub(crate) fn request_power_off() {
    IN_STANDBY.store(false, Ordering::Release);
    POWERING_OFF.store(true, Ordering::Release);
    POWER_OFF_REQUEST.signal(());
}

/// True after [`request_sleep`] or [`request_power_off`]. Other tasks
/// park rails when this is set.
pub(crate) fn is_requested() -> bool {
    SLEEPING.load(Ordering::Acquire) || POWERING_OFF.load(Ordering::Acquire)
}

/// True while the panel is in standby and waiting for Page Up 1 s / 5 s.
pub(crate) fn is_in_standby() -> bool {
    IN_STANDBY.load(Ordering::Acquire)
}

/// Mark panel standby so the next Page Up uses the standby-exit hold.
pub(crate) fn enter_standby() {
    IN_STANDBY.store(true, Ordering::Release);
}

/// Leave panel standby (resume, sleep, or power-off).
pub(crate) fn leave_standby() {
    IN_STANDBY.store(false, Ordering::Release);
}

/// Sleep card failed; stay awake so a later hold can try again.
pub(crate) fn cancel_sleep_request() {
    SLEEPING.store(false, Ordering::Release);
}

/// Power-off paint failed; stay awake and keep the latch.
pub(crate) fn cancel_power_off_request() {
    POWERING_OFF.store(false, Ordering::Release);
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

/// Wait until Page Up has been low for 1 s, or it goes high (abort).
pub(crate) fn wait_resume_hold(page_up: &Input<'_>, delay: &mut Delay) -> ResumeHold {
    let mut held = 0u32;
    loop {
        let still_low = page_up.is_low();
        match classify_resume_hold(held, still_low) {
            ResumeHold::Waiting => {
                delay.delay_ms(20);
                held = held.saturating_add(20);
                if held > PAGE_UP_RESUME_MS {
                    held = PAGE_UP_RESUME_MS;
                }
            }
            other => return other,
        }
    }
}

/// Listen for a low Page Up on the RTC path, then deep sleep.
pub(crate) fn arm_page_up_and_sleep(page_up: &mut Input<'static>, lpwr: LowPower<'static>) -> ! {
    page_up.listen(GpioEvent::LowLevel);
    page_up
        .apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("GPIO5 has an RTC wake path");
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

/// Drop both latch pins: software power-off.
///
/// On battery the board dies. USB-C plug or the stock ~3 s AI Voice
/// hold is the power-on path. After this returns the MCU may already
/// be unpowered; the caller should not expect further work.
pub(crate) fn release_latch(latch: Latch<Output<'static>, Output<'static>>) {
    let mut buf = [0u8; LINE_CAPACITY];
    if let Ok(line) = format_poweroff(&mut buf) {
        println!("{line}");
    }
    let _ = latch.release();
}

pub(crate) fn hold_output(pin: &mut Output<'static>) {
    pin.set_pad_hold(true);
}

pub(crate) fn hold_input(pin: &mut Input<'static>) {
    pin.set_pad_hold(true);
}

/// After Ferris is on the glass, wait for Page Up high, then arm `ext1`.
pub(crate) async fn wait_release_and_arm(page_up: &mut Input<'static>) {
    if page_up.is_low() {
        page_up.wait_for_high().await;
    }
    Timer::after(Duration::from_millis(20)).await;
    page_up.listen(GpioEvent::LowLevel);
    page_up
        .apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
        .expect("GPIO5 has an RTC wake path");
    WAKE_ARMED.signal(());
}

/// Default Page Up input (external pull-up plus MCU pull-up).
pub(crate) fn page_up_input(pin: esp_hal::peripherals::GPIO5<'static>) -> Input<'static> {
    Input::new(pin, InputConfig::default().with_pull(Pull::Up))
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
