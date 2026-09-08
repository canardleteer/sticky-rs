//! Linux BlueZ central (`bluer`). Connect, not `Pair()`.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use remote_debug_wire::{GATT_RX_UUID, GATT_SERVICE_UUID, GATT_TX_UUID};
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

use crate::{Error, PasskeySource, Transport};

/// How long discovery may run before the advertise name is a miss.
const DISCOVER_SECS: u64 = 20;
/// How long a notify wait may block.
const NOTIFY_SECS: u64 = 30;

/// Live GATT link. Address is never formatted.
pub struct BluerTransport {
    /// Owns the tokio threads for notify + later writes.
    _rt: tokio::runtime::Runtime,
    rt: tokio::runtime::Handle,
    adapter: bluer::Adapter,
    device: bluer::Device,
    rx: bluer::gatt::remote::Characteristic,
    tx_chunks: Arc<Mutex<Vec<Vec<u8>>>>,
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
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::Io(error.to_string()))?;
    let handle = rt.handle().clone();
    let opened = handle.block_on(connect_async(handle.clone(), advertise_name, passkey))?;
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
    tx_chunks: Arc<Mutex<Vec<Vec<u8>>>>,
    agent: bluer::agent::AgentHandle,
}

async fn connect_async(
    handle: tokio::runtime::Handle,
    advertise_name: &str,
    passkey: Arc<dyn PasskeySource>,
) -> Result<Opened, Error> {
    let session = bluer::Session::new()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;
    let adapter = session
        .default_adapter()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;
    adapter
        .set_powered(true)
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

    let device = discover_named(&adapter, advertise_name).await?;
    // Discovery token drops with the stream in `discover_named`.
    // Connect with discovery still up races BlueZ pairing.

    device
        .connect()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;

    let (rx, tx) = open_debug_chars(&device).await?;
    let tx_chunks = Arc::new(Mutex::new(Vec::new()));
    let notify_chunks = tx_chunks.clone();
    let notify = tx
        .notify()
        .await
        .map_err(|error| Error::Ble(error.to_string()))?;
    handle.spawn(async move {
        let mut notify = std::pin::pin!(notify);
        while let Some(chunk) = notify.next().await {
            notify_chunks.lock().await.push(chunk);
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
                return Ok(device);
            }
        }
        Err(Error::Ble(format!(
            "advertise name {name} not seen (walk to scene=pair)"
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
                    if let Some(chunk) = chunks.lock().await.pop() {
                        return Ok(chunk);
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .map_err(|_| Error::Io("notify timeout".into()))?
        })
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
