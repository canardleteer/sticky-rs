//! Linux BlueZ central (`bluer`). Connect, not `Pair()`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use remote_debug_wire::{GATT_RX_UUID, GATT_SERVICE_UUID, GATT_TX_UUID};
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

use crate::{Error, PasskeySource, Transport};

/// How long discovery may run before the advertise name is a miss.
pub const DISCOVER_SECS: u64 = 20;
/// How long a notify wait may block.
const NOTIFY_SECS: u64 = 30;
/// How long after `Connect` the ACL may take to show `Connected`.
const CONNECTED_SECS: u64 = 5;
/// Pair + `ServicesResolved` after ACL `Connect`.
pub const GATT_READY_SECS: u64 = 45;
/// UART scrape / [`crate::ChannelPasskey`] after ACL `Connect` starts.
///
/// One GATT wait, one rediscover, one GATT wait. The scrape thread
/// starts at `Connect`, not at advertise discovery.
pub const PAIR_WINDOW_SECS: u64 = GATT_READY_SECS + DISCOVER_SECS + GATT_READY_SECS;

/// Live GATT link. Address is never formatted.
///
/// TX notifies enqueue FIFO. `Vec` + `pop` reversed a multi-chunk
/// snapshot and the assembler reported `frame: Version`.
pub struct BluerTransport {
    /// Owns the tokio threads for notify + later writes.
    _rt: tokio::runtime::Runtime,
    rt: tokio::runtime::Handle,
    adapter: bluer::Adapter,
    device: bluer::Device,
    rx: bluer::gatt::remote::Characteristic,
    /// FIFO of TX notify payloads. A `Vec` + `pop` reversed
    /// multi-chunk snapshots (`frame: Version`).
    tx_chunks: Arc<Mutex<VecDeque<Vec<u8>>>>,
    _agent: bluer::agent::AgentHandle,
}

/// Connect by advertise name, enter the passkey, open RX/TX.
///
/// Stops LE discovery before Connect. Does not call `Device::pair`.
///
/// # Errors
///
/// No adapter, name miss, GATT walk, or passkey failure.
pub fn connect(
    advertise_name: &str,
    passkey: Arc<dyn PasskeySource>,
) -> Result<BluerTransport, Error> {
    connect_with(advertise_name, passkey, || {})
}

/// [`connect`] plus a hook fired once, after a live advertise is found
/// and immediately before BlueZ `Connect` (UART scrape starts here).
///
/// # Errors
///
/// Same as [`connect`].
pub fn connect_with(
    advertise_name: &str,
    passkey: Arc<dyn PasskeySource>,
    on_connecting: impl FnOnce() + Send + 'static,
) -> Result<BluerTransport, Error> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::Io(error.to_string()))?;
    let handle = rt.handle().clone();
    let opened = handle.block_on(connect_async(
        handle.clone(),
        advertise_name,
        passkey,
        on_connecting,
    ))?;
    Ok(BluerTransport {
        _rt: rt,
        rt: handle,
        adapter: opened.adapter,
        device: opened.device,
        rx: opened.rx,
        tx_chunks: opened.tx_chunks,
        _agent: opened.agent,
    })
}

/// Fields filled before the runtime is stored on [`BluerTransport`].
struct Opened {
    adapter: bluer::Adapter,
    device: bluer::Device,
    rx: bluer::gatt::remote::Characteristic,
    tx_chunks: Arc<Mutex<VecDeque<Vec<u8>>>>,
    agent: bluer::agent::AgentHandle,
}

async fn connect_async(
    handle: tokio::runtime::Handle,
    advertise_name: &str,
    passkey: Arc<dyn PasskeySource>,
    on_connecting: impl FnOnce() + Send + 'static,
) -> Result<Opened, Error> {
    let session = bluer::Session::new()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;

    let agent = bluer::agent::Agent {
        request_default: true,
        request_passkey: Some(Box::new(move |_req| {
            let passkey = passkey.clone();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || passkey.request_passkey())
                    .await
                    .map_err(|_| bluer::agent::ReqError::Rejected)?
                    .map_err(|_| bluer::agent::ReqError::Rejected)
            })
        })),
        ..bluer::agent::Agent::default()
    };
    let agent_handle = session
        .register_agent(agent)
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;

    let (adapter, device) = adapter_and_live_device(&session, advertise_name).await?;
    // Discovery token drops with the stream in `discover_named`.
    // Connect with discovery still up races BlueZ pairing.
    on_connecting();
    let (device, rx, tx) = connect_debug_chars(&adapter, advertise_name, device).await?;
    let tx_chunks = Arc::new(Mutex::new(VecDeque::new()));
    let notify_chunks = tx_chunks.clone();
    let notify = tx
        .notify()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;
    handle.spawn(async move {
        let mut notify = std::pin::pin!(notify);
        while let Some(chunk) = notify.next().await {
            notify_chunks.lock().await.push_back(chunk);
        }
    });

    Ok(Opened {
        adapter,
        device,
        rx,
        tx_chunks,
        agent: agent_handle,
    })
}

/// Connect, wait for ACL, walk RX/TX. One retry after a leftover cache miss.
///
/// A prior sit may leave a named BlueZ object whose GATT cache is stale
/// (`ServicesResolved` never becomes true). Drop that object (never print
/// the address) and discover again. Does not call `Device::pair`.
async fn connect_debug_chars(
    adapter: &bluer::Adapter,
    advertise_name: &str,
    device: bluer::Device,
) -> Result<
    (
        bluer::Device,
        bluer::gatt::remote::Characteristic,
        bluer::gatt::remote::Characteristic,
    ),
    Error,
> {
    match connect_debug_chars_once(&device).await {
        Ok((rx, tx)) => Ok((device, rx, tx)),
        Err(error) if is_stale_gatt(&error) => {
            forget_device(adapter, &device).await;
            let device = discover_named(adapter, advertise_name).await?;
            let (rx, tx) = connect_debug_chars_once(&device).await?;
            Ok((device, rx, tx))
        }
        Err(error) => Err(error),
    }
}

async fn connect_debug_chars_once(
    device: &bluer::Device,
) -> Result<
    (
        bluer::gatt::remote::Characteristic,
        bluer::gatt::remote::Characteristic,
    ),
    Error,
> {
    if device.is_connected().await.unwrap_or(false) {
        let _ = device.disconnect().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    device
        .connect()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;
    wait_connected(device).await?;
    wait_gatt_ready(device).await?;
    open_debug_chars(device).await
}

/// BlueZ `Connect` can return before DisplayOnly SMP finishes. Walking
/// GATT then is `ServicesUnresolved` (or `Connected` drops mid-pair).
async fn wait_gatt_ready(device: &bluer::Device) -> Result<(), Error> {
    let deadline = std::time::Instant::now() + Duration::from_secs(GATT_READY_SECS);
    loop {
        let connected = device.is_connected().await.unwrap_or(false);
        let resolved = device.is_services_resolved().await.unwrap_or(false);
        if connected && resolved {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(Error::Ble(if connected {
                "GATT services not resolved after pair".into()
            } else {
                "Connect dropped before pair (UART pair pin= or --pin)".into()
            }));
        }
        if !connected {
            let _ = device.connect().await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_connected(device: &bluer::Device) -> Result<(), Error> {
    let deadline = std::time::Instant::now() + Duration::from_secs(CONNECTED_SECS);
    loop {
        if device.is_connected().await.unwrap_or(false) {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(Error::Ble("Connect returned but the ACL is not up".into()));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Default adapter that currently sees advertise `name` (RSSI present).
///
/// A leftover object with no RSSI is skipped so we do not Connect a
/// ghost. Never prints an address.
async fn adapter_and_live_device(
    session: &bluer::Session,
    name: &str,
) -> Result<(bluer::Adapter, bluer::Device), Error> {
    let adapter = session
        .default_adapter()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;
    adapter
        .set_powered(true)
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;
    forget_named(&adapter, name).await;
    let device = discover_named(&adapter, name).await?;
    Ok((adapter, device))
}

async fn forget_named(adapter: &bluer::Adapter, name: &str) {
    let Ok(addrs) = adapter.device_addresses().await else {
        return;
    };
    let mut forgot = false;
    for addr in addrs {
        let Ok(device) = adapter.device(addr) else {
            continue;
        };
        let alias = device.alias().await.ok();
        let local = device.name().await.ok().flatten();
        if alias.as_deref() == Some(name) || local.as_deref() == Some(name) {
            forget_device(adapter, &device).await;
            forgot = true;
        }
    }
    if forgot {
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

async fn forget_device(adapter: &bluer::Adapter, device: &bluer::Device) {
    let addr = device.address();
    let _ = device.disconnect().await;
    let _ = adapter.remove_device(addr).await;
}

fn is_stale_gatt(error: &Error) -> bool {
    match error {
        Error::Ble(reason) => {
            reason.contains("have not been resolved")
                || reason.contains("Connect dropped before pair")
                || reason.contains("GATT services not resolved after pair")
        }
        _ => false,
    }
}

async fn discover_named(adapter: &bluer::Adapter, name: &str) -> Result<bluer::Device, Error> {
    // Property-change stream: DeviceAdded can arrive before Name/Alias.
    let mut discover = adapter
        .discover_devices_with_changes()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;
    let found = timeout(Duration::from_secs(DISCOVER_SECS), async {
        while let Some(event) = discover.next().await {
            let addr = match event {
                bluer::AdapterEvent::DeviceAdded(addr) => addr,
                _ => continue,
            };
            let device = match adapter.device(addr) {
                Ok(device) => device,
                Err(_) => continue,
            };
            let alias = device.alias().await.ok();
            let local = device.name().await.ok().flatten();
            if alias.as_deref() == Some(name) || local.as_deref() == Some(name) {
                // No RSSI: BlueZ still has the object but it is not
                // advertising. Connect on that ghost drops before
                // `PassKeyDisplay` (no UART `pair pin=`).
                if device.rssi().await.ok().flatten().is_none() {
                    continue;
                }
                return Ok(device);
            }
        }
        Err(Error::Ble(format!(
            "advertise name {name} not seen (remote-debug: splash; else scene=pair)"
        )))
    })
    .await
    .map_err(|_| Error::Ble(format!("timed out waiting for {name}")))?;
    found
}

async fn open_debug_chars(
    device: &bluer::Device,
) -> Result<
    (
        bluer::gatt::remote::Characteristic,
        bluer::gatt::remote::Characteristic,
    ),
    Error,
> {
    let service_uuid =
        Uuid::parse_str(GATT_SERVICE_UUID).map_err(|error| Error::Ble(error.to_string()))?;
    let rx_uuid = Uuid::parse_str(GATT_RX_UUID).map_err(|error| Error::Ble(error.to_string()))?;
    let tx_uuid = Uuid::parse_str(GATT_TX_UUID).map_err(|error| Error::Ble(error.to_string()))?;

    let mut rx = None;
    let mut tx = None;
    for service in device
        .services()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?
    {
        let uuid = service
            .uuid()
            .await
            .map_err(|error| Error::Ble(error.to_string()))?;
        if uuid != service_uuid {
            continue;
        }
        for ch in service
            .characteristics()
            .await
            .map_err(|error| Error::Ble(error.to_string()))?
        {
            let uuid = ch
                .uuid()
                .await
                .map_err(|error| Error::Ble(error.to_string()))?;
            if uuid == rx_uuid {
                rx = Some(ch);
            } else if uuid == tx_uuid {
                tx = Some(ch);
            }
        }
    }
    match (rx, tx) {
        (Some(rx), Some(tx)) => Ok((rx, tx)),
        _ => Err(Error::Ble(
            "remote-debug GATT service found but RX/TX missing".into(),
        )),
    }
}

impl Transport for BluerTransport {
    fn write_frame(&mut self, framed: &[u8]) -> Result<(), Error> {
        let rx = self.rx.clone();
        self.rt.block_on(async move {
            let mtu = rx.mtu().await.unwrap_or(20).max(20);
            for chunk in framed.chunks(mtu) {
                rx.write(chunk)
                    .await
                    .map_err(|error| Error::Ble(error.to_string()))?;
            }
            Ok(())
        })
    }

    fn read_chunk(&mut self) -> Result<Vec<u8>, Error> {
        let chunks = self.tx_chunks.clone();
        self.rt.block_on(async move {
            timeout(Duration::from_secs(NOTIFY_SECS), async {
                loop {
                    if let Some(chunk) = chunks.lock().await.pop_front() {
                        return Ok(chunk);
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .map_err(|_| Error::Io("notify timeout".into()))?
        })
    }

    fn try_read_chunk(&mut self) -> Option<Vec<u8>> {
        let chunks = self.tx_chunks.clone();
        self.rt
            .block_on(async move { chunks.lock().await.pop_front() })
    }

    fn disconnect(&mut self, keep_bond: bool) -> Result<(), Error> {
        let device = self.device.clone();
        let adapter = self.adapter.clone();
        self.rt.block_on(async move {
            let addr = device.address();
            let _ = device.disconnect().await;
            if !keep_bond {
                let _ = adapter.remove_device(addr).await;
            }
            Ok(())
        })
    }
}
