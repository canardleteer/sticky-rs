//! Wi-Fi channel survey and WPA2 SoftAP (`--features wifi` only).
//!
//! # Architecture
//!
//! This task is a walkthrough of **STA scan** and a **RAM SoftAP**, not a
//! phone STA join. On the unit: walk to `scene=wifi_survey` or
//! `scene=wifi_ap`, tap START, read the card. UART prints counts and
//! the fixed demo SSID/pass. Never a neighbor SSID, BSSID, or MAC.
//!
//! In the MCU:
//!
//! - **One mode machine.** [`WifiMode`] is Idle / SurveyScanning /
//!   SurveyComplete / Hotspot. Starting survey tears down SoftAP and
//!   vice versa. [`WifiCommand`] arrives from the display / touch path.
//! - **No factory NVS.** Scan and SoftAP stay in RAM. Do not write
//!   RF cal or credentials.
//! - **WPA2-Personal only.** The precompiled `esp-radio` ESP32-S3 blob
//!   has no WPA3/SAE path to advertise.
//! - **embassy-net + edge-dhcp + a tiny HTTP GET /** on
//!   [`embassy_debug::AP_IP_STR`]. JSON is device/scene/wifi counts —
//!   no gauge, no lamp, no PaperMono fields.
//! - **Leave drops the count.** Keep one event subscriber for the
//!   SoftAP session; emit the **new** `clients=` and [`bump_view`]
//!   so gray4 repaints. A USB STA that does not deauth waits the
//!   SoftAP idle timeout (10 s).
//!
//! Do not combine with `mic`, `radio`, `charge`, or `sd`. Packs with
//! `pair` (BLE stays in [`crate::pair`]; this module owns `WIFI` only).
//! How-to: [README.md](../README.md#wifi-test-instructions).

#[cfg(all(feature = "wifi", feature = "mic"))]
compile_error!("do not combine wifi with mic");
#[cfg(all(feature = "wifi", feature = "radio"))]
compile_error!("do not combine wifi with radio");
#[cfg(all(feature = "wifi", feature = "charge"))]
compile_error!("do not combine wifi with charge");
#[cfg(all(feature = "wifi", feature = "sd"))]
compile_error!("do not combine wifi with sd");

use crate::{emit, now_ms};

use core::cell::RefCell;
use core::fmt::Write;
use core::net::{Ipv4Addr, SocketAddrV4};
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU8, Ordering};

use edge_dhcp::io::{self, DEFAULT_SERVER_PORT};
use edge_dhcp::server::{Server as DhcpServer, ServerOptions};
use edge_nal::UdpBind;
use edge_nal_embassy::{Udp, UdpBuffers};
use embassy_debug::{Event, Scene, AP_PASSWORD, AP_SSID};
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_net::tcp::TcpSocket;
use embassy_net::{IpListenEndpoint, Ipv4Cidr, Runner, Stack, StackResources, StaticConfigV4};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use embedded_io_async::Write as AsyncWrite;
use esp_hal::peripherals::WIFI;
use esp_println::println;
use esp_radio::wifi::ap::AccessPointConfig;
use esp_radio::wifi::event::EventInfo;
use esp_radio::wifi::scan::{ScanConfig as WifiScanConfig, ScanTypeConfig};
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{
    AuthenticationMethod, AuthenticationMethodConfig, Config, ControllerConfig, Interface,
    WifiController,
};

const LOG: &str = "embassy-debug";

/// How many APs one survey window keeps for ranking.
const WIFI_MAX: usize = 32;

/// Passive dwell per channel, in milliseconds.
const WIFI_PASSIVE_MS: u64 = 150;

/// SoftAP IPv4 (same as [`AP_IP_STR`]).
pub const AP_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);

/// SoftAP station idle timeout, in seconds.
///
/// ESP-IDF `esp_wifi_set_inactive_time` on `WIFI_IF_AP`: SoftAP
/// minimum is **10**. A USB STA that drops without a deauth stays in
/// the AID table until this elapses; then
/// `AccessPointStationDisconnected` fires and we repaint.
const AP_STA_INACTIVE_SECS: u16 = 10;

/// ESP-IDF `wifi_interface_t` / `WIFI_IF_AP`.
const WIFI_IF_AP: u32 = 1;

/// What the Wi-Fi manager is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiMode {
    /// Neither survey nor hotspot.
    Idle,
    /// STA scan in flight.
    SurveyScanning,
    /// Last survey is on the card.
    SurveyComplete,
    /// SoftAP + DHCP + HTTP.
    Hotspot,
}

/// Touch / key command for the manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiCommand {
    /// Run (or re-run) the channel survey. Stops SoftAP first.
    StartSurvey,
    /// Return to idle from a scan.
    StopSurvey,
    /// Start SoftAP. Stops survey first.
    StartHotspot,
    /// Tear down SoftAP.
    StopHotspot,
}

/// One neighbor AP for the survey card. UART never prints this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurveyApEntry {
    /// Truncated SSID bytes (glass only).
    pub ssid: [u8; 18],
    /// Valid length of [`Self::ssid`].
    pub ssid_len: u8,
    /// 1..=13.
    pub channel: u8,
    /// dBm.
    pub rssi: i8,
    /// Short auth token (`WPA2`, `Open`, …).
    pub auth: &'static str,
}

impl SurveyApEntry {
    /// SSID as UTF-8 for the card, or `?`.
    #[must_use]
    pub fn ssid_str(&self) -> &str {
        core::str::from_utf8(&self.ssid[..usize::from(self.ssid_len)]).unwrap_or("?")
    }
}

/// Histogram + top four APs from the last completed survey.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiSurveyData {
    /// APs the scan returned.
    pub total_aps: u16,
    /// Channel 1 count.
    pub ch1_count: u16,
    /// Channel 6 count.
    pub ch6_count: u16,
    /// Channel 11 count.
    pub ch11_count: u16,
    /// Every other 1..=13 channel.
    pub other_count: u16,
    /// Strongest four by RSSI. Glass only.
    pub top_aps: [Option<SurveyApEntry>; 4],
}

/// Live SoftAP numbers for the hotspot card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WifiApStatus {
    /// Beaconing.
    pub active: bool,
    /// Associated stations (count only).
    pub clients: u16,
    /// `GET /` count this session.
    pub http_requests: u32,
}

/// 0=Idle, 1=SurveyScanning, 2=SurveyComplete, 3=Hotspot.
static WIFI_MODE: AtomicU8 = AtomicU8::new(0);
/// Bumped on every mode / client / HTTP change so the panel can repaint.
static WIFI_STATE_REV: AtomicU32 = AtomicU32::new(0);
/// Wake the display task.
pub static WIFI_VIEW: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static HOTSPOT_ACTIVE: AtomicBool = AtomicBool::new(false);
static AP_CLIENTS: AtomicU16 = AtomicU16::new(0);
static HTTP_REQUESTS: AtomicU32 = AtomicU32::new(0);
static SURVEY_DATA: Mutex<CriticalSectionRawMutex, RefCell<Option<WifiSurveyData>>> =
    Mutex::new(RefCell::new(None));
static WIFI_CMD: Channel<CriticalSectionRawMutex, WifiCommand, 4> = Channel::new();
static STACK_RESOURCES: static_cell::StaticCell<StackResources<4>> = static_cell::StaticCell::new();
static UDP_BUFFERS: static_cell::StaticCell<UdpBuffers<2, 1024, 1024, 4>> =
    static_cell::StaticCell::new();

/// Last painted scene persist-byte (JSON / hit-test). Display writes this.
static UI_SCENE: AtomicU8 = AtomicU8::new(0);
/// Last in-plane rotation discriminant. Display writes this.
static UI_ROTATION: AtomicU8 = AtomicU8::new(0);

/// Queue a command. Drops if the manager is backed up.
pub fn send_wifi_cmd(cmd: WifiCommand) {
    let _ = WIFI_CMD.try_send(cmd);
}

/// Current mode for the survey / AP cards.
#[must_use]
pub fn wifi_mode() -> WifiMode {
    match WIFI_MODE.load(Ordering::Acquire) {
        1 => WifiMode::SurveyScanning,
        2 => WifiMode::SurveyComplete,
        3 => WifiMode::Hotspot,
        _ => WifiMode::Idle,
    }
}

/// Revision the display task samples to decide a same-card repaint.
#[must_use]
pub fn state_rev() -> u32 {
    WIFI_STATE_REV.load(Ordering::Acquire)
}

/// Last completed survey, if any.
#[must_use]
pub fn survey_data() -> Option<WifiSurveyData> {
    SURVEY_DATA.lock(|cell| *cell.borrow())
}

/// SoftAP counters for the hotspot card.
#[must_use]
pub fn ap_status() -> WifiApStatus {
    WifiApStatus {
        active: HOTSPOT_ACTIVE.load(Ordering::Acquire),
        clients: AP_CLIENTS.load(Ordering::Acquire),
        http_requests: HTTP_REQUESTS.load(Ordering::Acquire),
    }
}

/// Display task: remember the current card for JSON and touch hit-test.
pub fn set_ui_scene(scene: Scene) {
    UI_SCENE.store(scene.persist_byte(), Ordering::Release);
}

/// Display task: remember the current IMU page for touch hit-test.
pub fn set_ui_rotation(rotation: seeed_reterminal_sticky::display::PageRotation) {
    let byte = match rotation {
        seeed_reterminal_sticky::display::PageRotation::Portrait0 => 0,
        seeed_reterminal_sticky::display::PageRotation::Portrait180 => 1,
        seeed_reterminal_sticky::display::PageRotation::Landscape0 => 2,
        seeed_reterminal_sticky::display::PageRotation::Landscape180 => 3,
    };
    UI_ROTATION.store(byte, Ordering::Release);
}

/// Scene the display last painted.
#[must_use]
pub fn ui_scene() -> Option<Scene> {
    Scene::from_persist_byte(UI_SCENE.load(Ordering::Acquire))
}

/// IMU page the display last painted.
#[must_use]
pub fn ui_rotation() -> seeed_reterminal_sticky::display::PageRotation {
    match UI_ROTATION.load(Ordering::Acquire) {
        1 => seeed_reterminal_sticky::display::PageRotation::Portrait180,
        2 => seeed_reterminal_sticky::display::PageRotation::Landscape0,
        3 => seeed_reterminal_sticky::display::PageRotation::Landscape180,
        _ => seeed_reterminal_sticky::display::PageRotation::Portrait0,
    }
}

/// Bump the revision and wake the display task for a same-card repaint.
///
/// Gray4 paint is slow. [`WIFI_VIEW`] is a one-slot signal: a leave
/// during a refresh still wakes the next wait so [`ap_status`] is
/// sampled after the current frame.
fn bump_view() {
    WIFI_STATE_REV.fetch_add(1, Ordering::Release);
    WIFI_VIEW.signal(());
}

/// Print `wifi_ap` with the **new** count and request a gray4 refresh.
///
/// [`AtomicU16::fetch_update`] returns the previous value — do not emit
/// that as `clients=`. Never a station MAC.
fn publish_ap_clients(clients: u16) {
    emit(Event::WifiAp {
        t_ms: now_ms(),
        active: true,
        clients,
    });
    bump_view();
}

/// Saturating decrement of [`AP_CLIENTS`]. Returns the stored value.
fn decrement_ap_clients() -> u16 {
    let mut next = 0;
    let _ = AP_CLIENTS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
        next = cur.saturating_sub(1);
        Some(next)
    });
    next
}

/// ESP-IDF `esp_wifi_set_inactive_time` (SoftAP min 10 s). Linked by
/// `esp-radio`; not re-exported. Call after [`WifiController::set_config`]
/// so the AP interface exists. Ignore the status — a miss leaves the
/// blob default (often 300 s).
fn apply_softap_idle_timeout() {
    unsafe extern "C" {
        fn esp_wifi_set_inactive_time(ifx: u32, sec: u16) -> i32;
    }
    let _ = unsafe { esp_wifi_set_inactive_time(WIFI_IF_AP, AP_STA_INACTIVE_SECS) };
}

/// One SoftAP session: keep the event subscriber so a leave is not
/// dropped between `wait_for_*` calls. Returns a queued command, or
/// `None` when sleep asked us to tear down.
async fn drive_hotspot_events(
    events: &mut esp_radio::wifi::event::EventSubscriber<'_>,
) -> Option<WifiCommand> {
    loop {
        if crate::sleep::is_requested() {
            return None;
        }
        match select(events.next_event_pure(), WIFI_CMD.receive()).await {
            Either::First(EventInfo::AccessPointStationConnected { .. }) => {
                let clients = AP_CLIENTS.fetch_add(1, Ordering::Relaxed) + 1;
                publish_ap_clients(clients);
            }
            Either::First(EventInfo::AccessPointStationDisconnected { .. }) => {
                let clients = decrement_ap_clients();
                publish_ap_clients(clients);
            }
            Either::First(_) => {}
            Either::Second(cmd) => return Some(cmd),
        }
    }
}

/// Fallback when [`WifiController::subscribe`] is full: same count /
/// repaint rules, but each wait recreates a subscriber.
async fn drive_hotspot_wait(controller: &WifiController<'_>) -> Option<WifiCommand> {
    loop {
        if crate::sleep::is_requested() {
            return None;
        }
        match select(
            controller.wait_for_access_point_connected_event_async(),
            WIFI_CMD.receive(),
        )
        .await
        {
            Either::First(Ok(esp_radio::wifi::ap::EventInfo::Connected(_))) => {
                let clients = AP_CLIENTS.fetch_add(1, Ordering::Relaxed) + 1;
                publish_ap_clients(clients);
            }
            Either::First(Ok(esp_radio::wifi::ap::EventInfo::Disconnected(_))) => {
                let clients = decrement_ap_clients();
                publish_ap_clients(clients);
            }
            Either::First(Err(_)) => {
                Timer::after(Duration::from_millis(500)).await;
            }
            Either::Second(cmd) => return Some(cmd),
        }
    }
}

/// Store the mode discriminant and wake the panel.
fn set_wifi_mode(mode: WifiMode) {
    let val = match mode {
        WifiMode::Idle => 0,
        WifiMode::SurveyScanning => 1,
        WifiMode::SurveyComplete => 2,
        WifiMode::Hotspot => 3,
    };
    WIFI_MODE.store(val, Ordering::Release);
    bump_view();
}

/// Short auth token for the survey card. UART never prints this.
fn auth_str(auth: Option<AuthenticationMethod>) -> &'static str {
    match auth {
        Some(AuthenticationMethod::None) => "Open",
        Some(AuthenticationMethod::Wep) => "WEP",
        Some(AuthenticationMethod::Wpa) => "WPA",
        Some(AuthenticationMethod::Wpa2Personal) => "WPA2",
        Some(AuthenticationMethod::WpaWpa2Personal) => "WPA/WPA2",
        Some(AuthenticationMethod::Wpa2Enterprise) => "WPA2-Ent",
        Some(AuthenticationMethod::Wpa3Personal) => "WPA3",
        Some(AuthenticationMethod::Wpa2Wpa3Personal) => "WPA2/WPA3",
        _ => "Secured",
    }
}

/// Bounded ASCII writer for JSON and HTTP headers (no `alloc`).
struct BufWriter<'a> {
    /// Destination bytes.
    buf: &'a mut [u8],
    /// Next write index.
    pos: usize,
}

impl<'a> BufWriter<'a> {
    /// Wrap a stack buffer.
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// UTF-8 view of the bytes written so far.
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.pos]).unwrap_or("")
    }

    /// Consume and return the written prefix.
    fn finish(self) -> &'a str {
        core::str::from_utf8(&self.buf[..self.pos]).unwrap_or("")
    }
}

impl core::fmt::Write for BufWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remain = self.buf.len().saturating_sub(self.pos);
        let to_copy = bytes.len().min(remain);
        self.buf[self.pos..self.pos + to_copy].copy_from_slice(&bytes[..to_copy]);
        self.pos += to_copy;
        Ok(())
    }
}

/// Histogram + top four by RSSI. UART is counts; SSIDs stay on glass.
fn process_survey_results(aps: &[esp_radio::wifi::ap::AccessPointInfo]) {
    let mut ch1_count = 0u16;
    let mut ch6_count = 0u16;
    let mut ch11_count = 0u16;
    let mut other_count = 0u16;
    for ap in aps {
        match ap.channel {
            1 => ch1_count = ch1_count.saturating_add(1),
            6 => ch6_count = ch6_count.saturating_add(1),
            11 => ch11_count = ch11_count.saturating_add(1),
            _ => other_count = other_count.saturating_add(1),
        }
    }

    let mut indices = [0usize; WIFI_MAX];
    let count = aps.len().min(WIFI_MAX);
    for (i, slot) in indices.iter_mut().enumerate().take(count) {
        *slot = i;
    }
    indices[..count].sort_by(|&a, &b| aps[b].signal_strength.cmp(&aps[a].signal_strength));

    let mut top_aps = [None; 4];
    for (i, &idx) in indices[..count.min(4)].iter().enumerate() {
        let ap = &aps[idx];
        let mut ssid_buf = [0u8; 18];
        let bytes = ap.ssid.as_str().as_bytes();
        let len = bytes.len().min(18);
        ssid_buf[..len].copy_from_slice(&bytes[..len]);
        top_aps[i] = Some(SurveyApEntry {
            ssid: ssid_buf,
            ssid_len: len as u8,
            channel: ap.channel,
            rssi: ap.signal_strength,
            auth: auth_str(ap.auth_method),
        });
    }

    let total = u16::try_from(count).unwrap_or(u16::MAX);
    SURVEY_DATA.lock(|cell| {
        *cell.borrow_mut() = Some(WifiSurveyData {
            total_aps: total,
            ch1_count,
            ch6_count,
            ch11_count,
            other_count,
            top_aps,
        });
    });
    emit(Event::WifiSurvey {
        t_ms: now_ms(),
        count: total,
        ch1: ch1_count,
        ch6: ch6_count,
        ch11: ch11_count,
        other: other_count,
    });
    bump_view();
}

/// Drop SoftAP and return the controller to STA. Emits `wifi_ap state=stopped`.
async fn stop_hotspot(controller: &mut WifiController<'static>) {
    if HOTSPOT_ACTIVE.swap(false, Ordering::Release) {
        AP_CLIENTS.store(0, Ordering::Relaxed);
        HTTP_REQUESTS.store(0, Ordering::Relaxed);
        let _ = controller.set_config(&Config::Station(StationConfig::default()));
        emit(Event::WifiAp {
            t_ms: now_ms(),
            active: false,
            clients: 0,
        });
        bump_view();
    }
}

/// Apply a touch command. Starting one mode tears the other down.
async fn handle_command(cmd: WifiCommand, controller: &mut WifiController<'static>) {
    match cmd {
        WifiCommand::StartSurvey => {
            stop_hotspot(controller).await;
            set_wifi_mode(WifiMode::SurveyScanning);
        }
        WifiCommand::StopSurvey => set_wifi_mode(WifiMode::Idle),
        WifiCommand::StartHotspot => set_wifi_mode(WifiMode::Hotspot),
        WifiCommand::StopHotspot => {
            stop_hotspot(controller).await;
            set_wifi_mode(WifiMode::Idle);
        }
    }
}

/// Sticky-shaped `GET /` body: device / scene / wifi counts. No gauge.
fn build_status_json(buf: &mut [u8], req_count: u32) -> &str {
    let mut writer = BufWriter::new(buf);
    let clients = AP_CLIENTS.load(Ordering::Relaxed);
    let scene = ui_scene().map(Scene::as_str).unwrap_or("unknown");
    let _ = write!(
        writer,
        "{{\"device\":\"sticky-rs\",\"scene\":\"{scene}\",\"wifi\":{{\"hotspot\":true,\"ssid\":\"{AP_SSID}\",\"clients\":{clients},\"requests\":{req_count}}}}}"
    );
    writer.finish()
}

/// embassy-net runner. Must stay polled while SoftAP is up.
#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) {
    runner.run().await;
}

/// DHCP leases on the SoftAP subnet. Idle while hotspot is down.
#[embassy_executor::task]
async fn dhcp_task(stack: Stack<'static>) {
    let buffers = UDP_BUFFERS.init(UdpBuffers::new());
    let unbound = Udp::new(stack, buffers);
    let Ok(mut socket) = unbound
        .bind(core::net::SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            DEFAULT_SERVER_PORT,
        )))
        .await
    else {
        println!("{LOG}: wifi dhcp bind failed");
        return;
    };

    let mut buf = [0u8; 1500];
    let mut gw_buf = [Ipv4Addr::UNSPECIFIED];
    loop {
        if crate::sleep::is_requested() {
            Timer::after(Duration::from_secs(3_600)).await;
            continue;
        }
        if !HOTSPOT_ACTIVE.load(Ordering::Relaxed) {
            Timer::after(Duration::from_millis(200)).await;
            continue;
        }
        let _ = io::server::run(
            &mut DhcpServer::<_, 64>::new_with_et(AP_IP),
            &ServerOptions::new(AP_IP, Some(&mut gw_buf)),
            &mut socket,
            &mut buf,
        )
        .await;
        Timer::after(Duration::from_millis(200)).await;
    }
}

/// `GET /` JSON on port 80. Idle while hotspot is down.
#[embassy_executor::task]
async fn http_task(stack: Stack<'static>) {
    let mut rx_buf = [0u8; 1024];
    let mut tx_buf = [0u8; 1024];
    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
    socket.set_timeout(Some(Duration::from_secs(5)));

    loop {
        if crate::sleep::is_requested() {
            Timer::after(Duration::from_secs(3_600)).await;
            continue;
        }
        if !HOTSPOT_ACTIVE.load(Ordering::Relaxed) {
            Timer::after(Duration::from_millis(200)).await;
            continue;
        }
        if socket
            .accept(IpListenEndpoint {
                addr: None,
                port: 80,
            })
            .await
            .is_err()
        {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        }

        let mut req_buf = [0u8; 512];
        let mut n = 0;
        while n < req_buf.len() {
            match socket.read(&mut req_buf[n..]).await {
                Ok(0) | Err(_) => break,
                Ok(read_len) => {
                    n += read_len;
                    if req_buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
            }
        }

        let req_count = HTTP_REQUESTS.fetch_add(1, Ordering::Relaxed) + 1;
        emit(Event::WifiHttp {
            t_ms: now_ms(),
            req: req_count,
        });
        bump_view();

        let mut json_buf = [0u8; 512];
        let json_str = build_status_json(&mut json_buf, req_count);
        let mut header_buf = [0u8; 128];
        let mut writer = BufWriter::new(&mut header_buf);
        let _ = write!(
            writer,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            json_str.len()
        );
        let _ = socket.write_all(writer.as_str().as_bytes()).await;
        let _ = socket.write_all(json_str.as_bytes()).await;
        let _ = socket.flush().await;
        Timer::after(Duration::from_millis(50)).await;
        socket.close();
        Timer::after(Duration::from_millis(50)).await;
        socket.abort();
    }
}

/// Mode machine: survey vs SoftAP, one owner of [`WifiController`].
#[embassy_executor::task]
async fn wifi_manager_task(mut controller: WifiController<'static>) {
    println!("{LOG}: wifi idle; survey/ap on tap; no NVS; no MAC");
    loop {
        if crate::sleep::is_requested() {
            stop_hotspot(&mut controller).await;
            set_wifi_mode(WifiMode::Idle);
            loop {
                Timer::after(Duration::from_secs(3_600)).await;
            }
        }
        match wifi_mode() {
            WifiMode::Idle | WifiMode::SurveyComplete => {
                let cmd = WIFI_CMD.receive().await;
                handle_command(cmd, &mut controller).await;
            }
            WifiMode::SurveyScanning => {
                let _ = controller.set_config(&Config::Station(StationConfig::default()));
                let scan_cfg = WifiScanConfig::default()
                    .with_max(WIFI_MAX)
                    .with_show_hidden(true)
                    .with_scan_type(ScanTypeConfig::Passive(
                        esp_hal::time::Duration::from_millis(WIFI_PASSIVE_MS),
                    ));
                match select(controller.scan_async(&scan_cfg), WIFI_CMD.receive()).await {
                    Either::First(Ok(aps)) => {
                        process_survey_results(&aps);
                        set_wifi_mode(WifiMode::SurveyComplete);
                    }
                    Either::First(Err(_)) => {
                        println!("{LOG}: wifi survey failed");
                        set_wifi_mode(WifiMode::Idle);
                    }
                    Either::Second(cmd) => handle_command(cmd, &mut controller).await,
                }
            }
            WifiMode::Hotspot => {
                let ap_cfg = Config::AccessPoint(
                    AccessPointConfig::default()
                        .with_ssid(AP_SSID.try_into().unwrap())
                        .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
                            AP_PASSWORD.try_into().unwrap(),
                        )),
                );
                let _ = controller.set_config(&ap_cfg);
                apply_softap_idle_timeout();
                HOTSPOT_ACTIVE.store(true, Ordering::Release);
                AP_CLIENTS.store(0, Ordering::Relaxed);
                HTTP_REQUESTS.store(0, Ordering::Relaxed);
                emit(Event::WifiAp {
                    t_ms: now_ms(),
                    active: true,
                    clients: 0,
                });
                bump_view();

                // Hold one subscriber for the session. Recreating
                // `wait_for_access_point_connected_event_async` drops
                // events in the gap (2026-09-04: join counted, leave
                // never printed `wifi_ap` / glass stayed at 1).
                let leave_cmd = match controller.subscribe() {
                    Ok(mut events) => drive_hotspot_events(&mut events).await,
                    Err(_) => {
                        println!("{LOG}: wifi event subscribe failed");
                        drive_hotspot_wait(&controller).await
                    }
                };
                if crate::sleep::is_requested() {
                    stop_hotspot(&mut controller).await;
                    set_wifi_mode(WifiMode::Idle);
                } else if let Some(cmd) = leave_cmd {
                    stop_hotspot(&mut controller).await;
                    handle_command(cmd, &mut controller).await;
                } else {
                    stop_hotspot(&mut controller).await;
                    set_wifi_mode(WifiMode::Idle);
                }
            }
        }
    }
}

/// Bring up SoftAP net stack + the mode machine. Owns `WIFI` only.
///
/// BLE pairing keeps [`esp_hal::peripherals::BT`]. Do not init NVS.
pub fn init_wifi(wifi: WIFI<'static>, spawner: Spawner) {
    let wifi_ap_device = Interface::access_point();
    let Ok(controller) = WifiController::new(
        wifi,
        ControllerConfig::default().with_initial_config(Config::Station(StationConfig::default())),
    ) else {
        println!("{LOG}: wifi controller failed");
        return;
    };

    let ap_net_config = embassy_net::Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(AP_IP, 24),
        gateway: Some(AP_IP),
        dns_servers: Default::default(),
    });
    let seed = 0xA5A5_5A5A_1234_5678;
    let stack_res = STACK_RESOURCES.init(StackResources::new());
    let (stack, runner) = embassy_net::new(wifi_ap_device, ap_net_config, stack_res, seed);

    spawner.spawn(net_task(runner).expect("wifi net task"));
    spawner.spawn(dhcp_task(stack).expect("wifi dhcp task"));
    spawner.spawn(http_task(stack).expect("wifi http task"));
    spawner.spawn(wifi_manager_task(controller).expect("wifi manager task"));
}
