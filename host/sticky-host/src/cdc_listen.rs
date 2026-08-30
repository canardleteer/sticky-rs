//! CDC ACM listen that never opens the kernel TTY.
//!
//! Linux `cdc-acm` asserts DTR+RTS in `acm_port_activate`. On this board those
//! lines are EN/IO0, so opening `/dev/ttyACM*` pulses a `POWERON` reset. This
//! path claims the USB interfaces instead and leaves the modem lines deasserted.

use std::io::{self, Read};
use std::time::Duration;

use nusb::descriptors::TransferType;
use nusb::transfer::{Bulk, ControlOut, ControlType, Direction, In, Recipient};
use nusb::MaybeFuture;

use crate::detect::{usb_device_key_for_port, QINHENG_PID, QINHENG_VID};
use crate::Error;

/// CDC `SET_LINE_CODING`.
const SET_LINE_CODING: u8 = 0x20;
/// CDC `SET_CONTROL_LINE_STATE`.
const SET_CONTROL_LINE_STATE: u8 = 0x22;
const CDC_COMM: u8 = 0x02;
const CDC_ACM: u8 = 0x02;
const CDC_DATA: u8 = 0x0A;

const USB_TIMEOUT: Duration = Duration::from_millis(250);

/// Open the CH343 as USB CDC and read UART bytes without pulsing EN/IO0.
pub struct CdcListen {
    reader: Option<nusb::io::EndpointRead<Bulk>>,
    data: Option<nusb::Interface>,
    comm: Option<nusb::Interface>,
    device: nusb::Device,
    comm_num: u8,
    data_num: u8,
}

impl CdcListen {
    /// Claim the USB device that backs `port`. Does not open the ACM node.
    pub fn open(port: &str) -> Result<Self, Error> {
        crate::detect::require_sticky_ch343(port)?;
        let key = usb_device_key_for_port(port)?;
        if key.vid != QINHENG_VID || key.pid != QINHENG_PID {
            return Err(Error::NotStickyUart {
                vid: Some(key.vid),
                pid: Some(key.pid),
            });
        }
        let info = nusb::list_devices()
            .wait()
            .map_err(|error| Error::Device(format!("USB list failed: {error}")))?
            .find(|dev| {
                dev.busnum() == key.busnum
                    && dev.device_address() == key.devnum
                    && dev.vendor_id() == key.vid
                    && dev.product_id() == key.pid
            })
            .ok_or_else(|| {
                Error::Device(format!(
                    "USB device bus {} addr {} is not visible to usbfs",
                    key.busnum, key.devnum
                ))
            })?;
        let device = info.open().wait().map_err(map_usb_open)?;
        let config = device
            .active_configuration()
            .map_err(|error| Error::Device(format!("USB configuration: {error}")))?;
        let layout = find_cdc_layout(&config)?;
        let comm = device
            .detach_and_claim_interface(layout.comm)
            .wait()
            .map_err(map_usb_open)?;
        let data = device
            .detach_and_claim_interface(layout.data)
            .wait()
            .map_err(map_usb_open)?;
        set_listen_coding(&comm, layout.comm)?;
        let mut reader = data
            .endpoint::<Bulk, In>(layout.bulk_in)
            .map_err(|error| Error::Device(format!("USB bulk-in: {error}")))?
            .reader(4096);
        reader.set_read_timeout(USB_TIMEOUT);
        log::info!(
            "CDC listen bus {} addr {} at 115200 (no ACM TTY, modem lines off)",
            key.busnum,
            key.devnum
        );
        Ok(Self {
            reader: Some(reader),
            data: Some(data),
            comm: Some(comm),
            device,
            comm_num: layout.comm,
            data_num: layout.data,
        })
    }
}

impl Read for CdcListen {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "CDC listen closed"))?
            .read(buf)
    }
}

impl Drop for CdcListen {
    fn drop(&mut self) {
        self.reader = None;
        self.data = None;
        self.comm = None;
        let _ = self.device.attach_kernel_driver(self.data_num);
        let _ = self.device.attach_kernel_driver(self.comm_num);
    }
}

struct CdcLayout {
    comm: u8,
    data: u8,
    bulk_in: u8,
}

fn find_cdc_layout(
    config: &nusb::descriptors::ConfigurationDescriptor<'_>,
) -> Result<CdcLayout, Error> {
    let mut comm = None;
    let mut data = None;
    let mut bulk_in = None;
    for alt in config.interface_alt_settings() {
        if alt.alternate_setting() != 0 {
            continue;
        }
        if alt.class() == CDC_COMM && alt.subclass() == CDC_ACM {
            comm = Some(alt.interface_number());
        }
        if alt.class() == CDC_DATA {
            data = Some(alt.interface_number());
        }
        for ep in alt.endpoints() {
            if ep.transfer_type() == TransferType::Bulk && ep.direction() == Direction::In {
                bulk_in = Some(ep.address());
                if data.is_none() {
                    data = Some(alt.interface_number());
                }
            }
        }
    }
    match (comm, data, bulk_in) {
        (Some(comm), Some(data), Some(bulk_in)) => Ok(CdcLayout {
            comm,
            data,
            bulk_in,
        }),
        _ => Err(Error::Device(
            "CH343 USB descriptors have no CDC ACM + bulk-in pair".into(),
        )),
    }
}

fn set_listen_coding(comm: &nusb::Interface, comm_num: u8) -> Result<(), Error> {
    let mut coding = [0u8; 7];
    coding[..4].copy_from_slice(&crate::monitor_impl::MONITOR_BAUD.to_le_bytes());
    coding[4] = 0;
    coding[5] = 0;
    coding[6] = 8;
    comm.control_out(
        ControlOut {
            control_type: ControlType::Class,
            recipient: Recipient::Interface,
            request: SET_LINE_CODING,
            value: 0,
            index: u16::from(comm_num),
            data: &coding,
        },
        USB_TIMEOUT,
    )
    .wait()
    .map_err(|error| Error::Device(format!("SET_LINE_CODING: {error}")))?;
    comm.control_out(
        ControlOut {
            control_type: ControlType::Class,
            recipient: Recipient::Interface,
            request: SET_CONTROL_LINE_STATE,
            value: 0,
            index: u16::from(comm_num),
            data: &[],
        },
        USB_TIMEOUT,
    )
    .wait()
    .map_err(|error| Error::Device(format!("SET_CONTROL_LINE_STATE: {error}")))?;
    Ok(())
}

fn map_usb_open(error: nusb::Error) -> Error {
    let text = error.to_string();
    if matches!(
        error.kind(),
        nusb::ErrorKind::PermissionDenied | nusb::ErrorKind::Busy
    ) || text.contains("Permission denied")
        || text.contains("Access denied")
    {
        return Error::Device(format!(
            "{text}. Claim the CH343 over usbfs so monitor does not open the ACM TTY \
             (cdc-acm asserts DTR+RTS on open and that pulses EN). Add a udev rule and replug: \
             SUBSYSTEM==\"usb\", ATTR{{idVendor}}==\"1a86\", ATTR{{idProduct}}==\"55d3\", \
             MODE=\"0660\", GROUP=\"dialout\". Or pass --acm-tty (embassy will POWERON)."
        ));
    }
    Error::Device(format!("USB open failed: {text}"))
}

#[cfg(test)]
mod tests {
    use super::find_cdc_layout;
    use nusb::descriptors::ConfigurationDescriptor;

    /// CDC ACM comm (if 0) + data (if 1) with bulk IN 0x81. Lengths are USB-legal.
    fn tiny_cdc_config() -> Vec<u8> {
        vec![
            9, 2, 41, 0, 2, 1, 0, 0x80, 50, // config, wTotalLength 41
            9, 4, 0, 0, 1, 0x02, 0x02, 0x01, 0, // comm
            7, 5, 0x83, 0x03, 8, 0, 10, // interrupt
            9, 4, 1, 0, 1, 0x0A, 0x00, 0x00, 0, // data
            7, 5, 0x81, 0x02, 64, 0, 0, // bulk IN
        ]
    }

    #[test]
    fn layout_finds_acm_and_bulk_in() {
        let bytes = tiny_cdc_config();
        let config = ConfigurationDescriptor::new(&bytes).expect("config");
        let layout = find_cdc_layout(&config).expect("cdc");
        assert_eq!(layout.comm, 0);
        assert_eq!(layout.data, 1);
        assert_eq!(layout.bulk_in, 0x81);
    }
}
