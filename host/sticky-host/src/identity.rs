//! Factory serial, USB serial, and `board-info` text.

use crate::Error;

/// Live chip identity used to bind a unit to an original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveIdentity {
    /// Station MAC from `board-info`, lowercase hex pairs.
    pub mac: String,
    /// CH343 serial parsed from `ESPFLASH_PORT` when it is a by-id node.
    pub usb_serial: Option<String>,
}

/// Parsed `cargo espflash board-info` / xtask-formatted board info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardInfo {
    /// Live identity (MAC; USB serial filled by the caller from the port).
    pub identity: LiveIdentity,
    /// Raw `Flash size:` field.
    pub flash_size: String,
    /// `Secure Boot:` reported enabled.
    pub secure_boot: bool,
    /// `Flash Encryption:` reported enabled.
    pub flash_encryption: bool,
}

/// QinHeng udev by-id marker. Confirmed form:
/// `USB_Single_Serial_<serial>-if00` (underscore). A hyphen before the serial
/// still parses.
pub(crate) fn qinheng_marker() -> &'static str {
    "usb-1a86_USB_Single_Serial"
}

/// Parse a CH343 serial out of an `ESPFLASH_PORT` value when it contains a
/// QinHeng by-id node. Never embed a device path in source; pass the env value.
#[must_use]
pub fn parse_usb_serial_from_port(port: &str) -> Option<String> {
    let marker = qinheng_marker();
    let rest = port.split(marker).nth(1)?;
    let rest = rest.trim_start_matches(['_', '-']);
    let serial = rest.split("-if").next()?.trim();
    if serial.is_empty() {
        None
    } else {
        Some(serial.to_string())
    }
}

/// Take `serial_number` from stock firmware UART (product-general log shape).
pub fn parse_factory_serial(uart: &str) -> Result<String, Error> {
    let mut found: Option<String> = None;
    for line in uart.lines() {
        let Some(value) = serial_from_line(line) else {
            continue;
        };
        match &found {
            None => found = Some(value),
            Some(previous) if previous == &value => {}
            Some(_) => return Err(Error::AmbiguousFactorySerial),
        }
    }
    let serial = found.ok_or(Error::MissingFactorySerial)?;
    validate_factory_serial(&serial)?;
    Ok(serial)
}

/// Whether [`parse_factory_serial`] would succeed on this buffer.
///
/// Used to stop UART sampling as soon as stock firmware has printed
/// `serial_number` (about 4.5–6.5 s after a run-mode EN/RTS pulse; IDF `I (5672)`).
#[must_use]
pub fn uart_has_unique_factory_serial(uart: &str) -> bool {
    parse_factory_serial(uart).is_ok()
}

fn serial_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(idx) = trimmed.find("key=serial_number") {
        return trimmed[idx..]
            .split("value=")
            .nth(1)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }
    None
}

/// `mac-<hex>` directory name from a board-info MAC (colons stripped).
pub fn mac_unit_id(mac: &str) -> Result<String, Error> {
    let hex: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let id = format!("mac-{}", hex.to_ascii_lowercase());
    validate_factory_serial(&id)?;
    Ok(id)
}

/// Factory serial when UART had one, otherwise [`mac_unit_id`].
pub fn unit_id(factory_serial: Option<&str>, mac: &str) -> Result<String, Error> {
    match factory_serial {
        Some(serial) => {
            validate_factory_serial(serial)?;
            Ok(serial.to_string())
        }
        None => mac_unit_id(mac),
    }
}

/// Directory names: no slashes, no `..`, printable ASCII.
pub fn validate_factory_serial(serial: &str) -> Result<(), Error> {
    if serial.is_empty()
        || serial.contains('/')
        || serial.contains('\\')
        || serial.contains("..")
        || !serial
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(Error::InvalidFactorySerial(serial.to_string()));
    }
    Ok(())
}

/// Parse board-info text (`cargo espflash board-info` shape). MAC is taken
/// from the `MAC address:` line without hard-coding an example address.
pub fn parse_board_info(text: &str) -> Result<BoardInfo, Error> {
    let mut mac = None;
    let mut flash_size = None;
    let mut secure_boot = false;
    let mut flash_encryption = false;

    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("MAC address:") {
            mac = Some(normalize_mac(rest.trim())?);
        }
        if let Some(rest) = line.strip_prefix("MAC:") {
            if mac.is_none() {
                mac = Some(normalize_mac(rest.trim())?);
            }
        }
        if let Some(rest) = line.strip_prefix("Flash size:") {
            flash_size = Some(rest.trim().to_string());
        }
        if line.to_ascii_lowercase().contains("secure boot:") {
            secure_boot = line.to_ascii_lowercase().contains("enabled");
        }
        if line.to_ascii_lowercase().contains("flash encryption:") {
            flash_encryption = line.to_ascii_lowercase().contains("enabled");
        }
    }

    let mac = mac.ok_or_else(|| Error::Device("board-info missing MAC address".into()))?;
    let flash_size = flash_size.unwrap_or_default();
    if !flash_size.to_ascii_uppercase().contains("32") {
        return Err(Error::FlashSizeNot32Mb(flash_size));
    }

    Ok(BoardInfo {
        identity: LiveIdentity {
            mac,
            usb_serial: None,
        },
        flash_size,
        secure_boot,
        flash_encryption,
    })
}

fn normalize_mac(raw: &str) -> Result<String, Error> {
    let token = raw.split_whitespace().next().unwrap_or("");
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 6 || parts.iter().any(|p| p.len() != 2) {
        return Err(Error::Device(
            "board-info MAC address was not six octets".into(),
        ));
    }
    Ok(parts
        .iter()
        .map(|p| p.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(":"))
}

#[cfg(test)]
pub(crate) fn test_mac() -> String {
    [0x10u8, 0x20, 0x30, 0x40, 0x50, 0x60]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_serial_from_stock_log_line() {
        let uart = "I (5672) app_deviceinfo: key=serial_number        value=TESTFACTORY001\n";
        assert_eq!(parse_factory_serial(uart).unwrap(), "TESTFACTORY001");
        assert!(uart_has_unique_factory_serial(uart));
        assert!(!uart_has_unique_factory_serial(
            "I (100) boot: still booting\n"
        ));
    }

    #[test]
    fn missing_factory_serial() {
        assert!(matches!(
            parse_factory_serial("hello"),
            Err(Error::MissingFactorySerial)
        ));
    }

    #[test]
    fn ambiguous_factory_serial() {
        let uart = concat!(
            "key=serial_number value=TESTFACTORY001\n",
            "key=serial_number value=TESTFACTORY002\n",
        );
        assert!(matches!(
            parse_factory_serial(uart),
            Err(Error::AmbiguousFactorySerial)
        ));
    }

    #[test]
    fn rejects_path_serial() {
        assert!(matches!(
            validate_factory_serial("../evil"),
            Err(Error::InvalidFactorySerial(_))
        ));
    }

    #[test]
    fn usb_serial_from_by_id_port() {
        let marker = qinheng_marker();
        let hyphen = format!("prefix/{marker}-TESTUSB-if00");
        let underscore = format!("prefix/{marker}_TESTUSB-if00");
        assert_eq!(
            parse_usb_serial_from_port(&hyphen).as_deref(),
            Some("TESTUSB")
        );
        assert_eq!(
            parse_usb_serial_from_port(&underscore).as_deref(),
            Some("TESTUSB")
        );
    }

    #[test]
    fn board_info_requires_32mb_and_mac() {
        let mac = test_mac();
        let text = format!(
            "Flash size:        32MB\nMAC address:       {mac}\nSecure Boot: Disabled\nFlash Encryption: Disabled\n"
        );
        let info = parse_board_info(&text).unwrap();
        assert_eq!(info.identity.mac, mac);
        assert!(!info.secure_boot);
        assert!(!info.flash_encryption);
    }

    #[test]
    fn board_info_rejects_wrong_size() {
        let mac = test_mac();
        let text = format!("Flash size: 16MB\nMAC address: {mac}\n");
        assert!(matches!(
            parse_board_info(&text),
            Err(Error::FlashSizeNot32Mb(_))
        ));
    }
}
