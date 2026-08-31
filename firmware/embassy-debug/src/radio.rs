//! Concurrent Wi-Fi + BLE scan on UART (`--features radio` only).
//!
//! Scan only. No STA join, no SoftAP, no BLE connect or advertise.
//! SSID / local name and RSSI only — never a MAC or BSSID.

use crate::{emit, now_ms};

use bt_hci::cmd::le::LeSetScanParams;
use bt_hci::controller::{ControllerCmdSync, ExternalController};
use embassy_debug::{
    format_ble_name, format_wifi_ssid, sanitize_radio_label, Event, LINE_CAPACITY, RADIO_LABEL_MAX,
    RADIO_REPORT_SECS,
};
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::{BT, WIFI};
use esp_println::println;
use esp_radio::ble::controller::BleConnector;
use esp_radio::wifi::{scan::ScanConfig, ControllerConfig, WifiController};
use trouble_host::connection::ScanConfig as BleScanConfig;
use trouble_host::prelude::{
    Address, Controller, DefaultPacketPool, EventHandler, HostResources, LeAdvReportsIter, Scanner,
};

const MAX_WIFI_LINES: usize = 8;
const MAX_BLE_LINES: usize = 8;

/// Bring up Wi-Fi and BLE together and print scan lines.
#[embassy_executor::task]
pub async fn radio_task(wifi: WIFI<'static>, bluetooth: BT<'static>) {
    let Ok(connector) = BleConnector::new(bluetooth, Default::default()) else {
        println!("{LOG}: ble connector failed");
        return;
    };
    let ble_controller: ExternalController<_, 1> = ExternalController::new(connector);

    let Ok(wifi_controller) = WifiController::new(wifi, ControllerConfig::default()) else {
        println!("{LOG}: wifi controller failed");
        return;
    };

    println!("{LOG}: radio scan wifi+ble; no join; no MAC");

    embassy_futures::join::join(
        wifi_scan_loop(wifi_controller),
        ble_scan_loop(ble_controller),
    )
    .await;
}

async fn wifi_scan_loop(mut controller: WifiController<'static>) {
    let scan_config = ScanConfig::default().with_max(MAX_WIFI_LINES);
    loop {
        match controller.scan_async(&scan_config).await {
            Ok(result) => {
                let n = result.len().min(255) as u8;
                emit(Event::Wifi { t_ms: now_ms(), n });
                let t_ms = now_ms();
                for ap in result.iter().take(MAX_WIFI_LINES) {
                    let mut label = [0u8; RADIO_LABEL_MAX];
                    let ssid = sanitize_radio_label(ap.ssid.as_str().as_bytes(), &mut label);
                    let mut buf = [0u8; LINE_CAPACITY];
                    if let Ok(line) = format_wifi_ssid(t_ms, ssid, ap.signal_strength, &mut buf) {
                        println!("{line}");
                    }
                }
            }
            Err(_) => println!("{LOG}: wifi scan failed"),
        }
        Timer::after(Duration::from_secs(u64::from(RADIO_REPORT_SECS))).await;
    }
}

async fn ble_scan_loop<C>(controller: C)
where
    C: Controller + ControllerCmdSync<LeSetScanParams>,
{
    // Fixed random address so we do not read or print the eFuse MAC.
    let address = Address::random([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let mut resources: HostResources<_, DefaultPacketPool, 1, 1> = HostResources::new();
    let stack = trouble_host::new(controller, &mut resources)
        .set_random_address(address)
        .build();
    let mut runner = stack.runner();
    let central = stack.central();

    let printer = NamePrinter::new();
    let mut scanner = Scanner::new(central);
    let config = BleScanConfig::default();

    // One session per report window, same as Wi-Fi `scan_async`. A single
    // long-lived enable can look frozen: trouble-host's scan docs say to
    // call `scan` again after a report, and some controllers keep their
    // own duplicate filter for the whole enable.
    let scan = async {
        loop {
            match scanner.scan(&config).await {
                Ok(_session) => {
                    Timer::after(Duration::from_secs(u64::from(RADIO_REPORT_SECS))).await;
                    printer.flush();
                }
                Err(_) => {
                    println!("{LOG}: ble scan failed");
                    Timer::after(Duration::from_secs(u64::from(RADIO_REPORT_SECS))).await;
                }
            }
        }
    };

    let _ = embassy_futures::join::join(runner.run_with_handler(&printer), scan).await;
}

struct NamePrinter {
    count: core::sync::atomic::AtomicU32,
    names: embassy_sync::blocking_mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        core::cell::RefCell<heapless::Vec<(heapless::String<RADIO_LABEL_MAX>, i8), MAX_BLE_LINES>>,
    >,
}

impl NamePrinter {
    fn new() -> Self {
        Self {
            count: core::sync::atomic::AtomicU32::new(0),
            names: embassy_sync::blocking_mutex::Mutex::new(core::cell::RefCell::new(
                heapless::Vec::new(),
            )),
        }
    }

    fn flush(&self) {
        let n = self
            .count
            .swap(0, core::sync::atomic::Ordering::Relaxed)
            .min(255) as u8;
        emit(Event::Ble { t_ms: now_ms(), n });
        let t_ms = now_ms();
        self.names.lock(|cell| {
            let mut names = cell.borrow_mut();
            for (name, rssi) in names.iter() {
                let mut buf = [0u8; LINE_CAPACITY];
                if let Ok(line) = format_ble_name(t_ms, name.as_str(), *rssi, &mut buf) {
                    println!("{line}");
                }
            }
            names.clear();
        });
    }

    fn consider(&self, name: &str, rssi: i8) {
        self.names.lock(|cell| {
            let mut names = cell.borrow_mut();
            if let Some((_, seen_rssi)) = names.iter_mut().find(|(seen, _)| seen.as_str() == name) {
                if rssi > *seen_rssi {
                    *seen_rssi = rssi;
                }
                return;
            }
            let Ok(stored) = heapless::String::try_from(name) else {
                return;
            };
            if !names.is_full() {
                let _ = names.push((stored, rssi));
                return;
            }
            if let Some((i, _)) = names.iter().enumerate().min_by_key(|(_, (_, r))| *r) {
                if rssi > names[i].1 {
                    names[i] = (stored, rssi);
                }
            }
        });
    }
}

impl EventHandler for NamePrinter {
    fn on_adv_reports(&self, mut it: LeAdvReportsIter<'_>) {
        while let Some(Ok(report)) = it.next() {
            self.count
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let raw_name = adv_local_name(report.data).unwrap_or(b"");
            let mut label = [0u8; RADIO_LABEL_MAX];
            let name = sanitize_radio_label(raw_name, &mut label);
            self.consider(name, report.rssi);
        }
    }
}

fn adv_local_name(data: &[u8]) -> Option<&[u8]> {
    let mut i = 0;
    while i + 1 < data.len() {
        let len = data[i] as usize;
        if len == 0 || i + 1 + len > data.len() {
            break;
        }
        let typ = data[i + 1];
        let payload = &data[i + 2..i + 1 + len];
        if typ == 0x08 || typ == 0x09 {
            return Some(payload);
        }
        i += 1 + len;
    }
    None
}

const LOG: &str = "embassy-debug";
