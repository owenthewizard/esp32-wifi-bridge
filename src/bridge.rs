//! Wi-Fi to Ethernet Bridge State Machine

extern crate alloc;
use alloc::{boxed::Box, format, string::String, sync::Arc};

use esp_idf_svc::{
    eth::{EthDriver, RmiiClockConfig, RmiiEth, RmiiEthChipset},
    eventloop::EspSystemEventLoop,
    hal::{gpio, modem::Modem, prelude::Peripherals},
    nvs::EspDefaultNvsPartition,
    wifi::{AuthMethod, ClientConfiguration, Configuration, WifiDeviceId, WifiDriver},
};

use once_cell::sync::OnceCell;

const SSID: &str = env!("WIFI_SSID");
const PASS: &str = env!("WIFI_PASS");
const AUTH: AuthMethod = AuthMethod::WPA2Personal;

/// Wi-Fi to Ethernet Bridge State Machine
pub struct Bridge<S> {
    state: S,
}

/// Idle State
///
/// In this state, `peripherals`, `sysloop`, and `nvs` are initialized,
/// but no action is being performed.
pub struct Idle {
    peripherals: Peripherals,
    sysloop: EspSystemEventLoop,
    nvs: Option<EspDefaultNvsPartition>,
}

/// Ethernet Ready State
///
/// In this state, [Ethernet](esp_idf_svc::eth::EthDriver) is ready to be transitioned into the
/// [`Running`] state. Additionally, `nvs`, `modem`, and `client_mac` have been initialized and are
/// ready to be used to bring Wi-Fi up.
/// Notably, `client_mac` is sniffed from the source MAC of the first Ethernet frame we catch.
/// At some point after we have sniffed `client_mac` (not necessarily immediately), we stop
/// sniffing future frames.
pub struct EthReady {
    modem: Modem,
    sysloop: EspSystemEventLoop,
    nvs: Option<EspDefaultNvsPartition>,
    eth: EthDriver<'static, RmiiEth>,
    client_mac: [u8; 6],
}

/// Wi-Fi Ready State
///
/// In this state, Wi-Fi is ready to be transitioned into the [`Running`] state.
/// Notably, the Wi-Fi `Sta` MAC has been set to `client_mac`.
pub struct WifiReady {
    eth: EthDriver<'static, RmiiEth>,
    wifi: WifiDriver<'static>,
}

/// Running State
///
/// In this state, the bridge keeps the drivers on the heap so their addresses remain stable for
/// the callbacks that forward frames between them.
pub struct Running {
    _eth: Box<EthDriver<'static, RmiiEth>>,
    _wifi: Box<WifiDriver<'static>>,
}

impl Bridge<Idle> {
    /// Construct a `Bridge` in the `Idle` state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use sm::*;
    ///
    /// let idle_bridge = Bridge::new();
    /// ```
    ///
    /// # Panics
    ///
    /// This function calls `take()` on
    /// [`Peripherals`], [`EspSystemEventLoop`], and [`EspDefaultNvsPartition`], and will panic if
    /// any of them return `Err`. Therefore, only one instance of `Bridge` should exist at any given
    /// time, and you shouldn't be using them elsewhere.
    pub fn new() -> Self {
        let peripherals = Peripherals::take().expect("Failed to take peripherals!");
        let sysloop = EspSystemEventLoop::take().expect("Failed to take sysloop!");
        let nvs = EspDefaultNvsPartition::take().ok();

        Self {
            state: Idle {
                peripherals,
                sysloop,
                nvs,
            },
        }
    }
}

/// Transition from [`Idle`] to [`EthReady`].
impl From<Bridge<Idle>> for Bridge<EthReady> {
    fn from(val: Bridge<Idle>) -> Self {
        let pins = val.state.peripherals.pins;
        let mut eth = EthDriver::new_rmii(
            val.state.peripherals.mac,
            pins.gpio25, // RMII RDX0
            pins.gpio26, // RMII RDX1
            pins.gpio27, // RMII CRS DV
            pins.gpio23, // WT32-ETH01 SMI MDC
            pins.gpio22, // EMII TXD1
            pins.gpio21, // RMII TX EN
            pins.gpio19, // RMII TXD0
            pins.gpio18, // WT32-ETH01 SMI MDIO
            RmiiClockConfig::<gpio::Gpio0, gpio::Gpio16, gpio::Gpio17>::Input(
                pins.gpio0, // WT32-ETH01 external clock
            ),
            Some(pins.gpio16), // WT32-ETH01 PHY reset
            RmiiEthChipset::LAN87XX,
            Some(1), // WT32-ETH01 PHY address
            val.state.sysloop.clone(),
        )
        .expect("Failed to init EthDriver!");

        // could emulate the following logic with mpsc::channel, but this is more efficient
        // at least in terms of binary size...

        let client_mac: Arc<OnceCell<[u8; 6]>> = Arc::new(OnceCell::new());
        let client_mac2 = Arc::clone(&client_mac);

        eth.set_rx_callback(move |frame| match frame.as_slice().get(6..12) {
            Some(mac_bytes) => {
                let src_mac = mac_bytes.try_into().unwrap();
                if client_mac2.set(src_mac).is_ok() {
                    log::warn!("Sniffed client MAC: {}", mac2str(src_mac));
                }
            }
            None => unreachable!("Failed to read source MAC from Ethernet frame!"),
        })
        .expect("Failed to set Ethernet callback! (macsniff)");

        log::warn!("Waiting to sniff client MAC...");
        eth.start().expect("Failed to start Ethernet!");
        let client_mac = *client_mac.wait();

        // maybe this should be non-fatal?
        eth.set_rx_callback(|_| {})
            .expect("Failed to unset Ethernet callback! (macsniff)");

        log::warn!("Setting Ethernet promiscuous...");
        eth.set_promiscuous(true)
            .expect("Failed to set Ethernet promiscuous!");
        log::warn!("Ethernet promiscuous success!");

        Self {
            state: EthReady {
                modem: val.state.peripherals.modem,
                sysloop: val.state.sysloop,
                nvs: val.state.nvs,
                eth,
                client_mac,
            },
        }
    }
}

/// Transition from [`EthReady`] to [`WifiReady`].
impl From<Bridge<EthReady>> for Bridge<WifiReady> {
    fn from(val: Bridge<EthReady>) -> Self {
        let mut wifi = WifiDriver::new(val.state.modem, val.state.sysloop.clone(), val.state.nvs)
            .expect("Failed to init WifiDriver!");

        let wifi_config = Configuration::Client(ClientConfiguration {
            ssid: SSID.try_into().unwrap(),
            auth_method: AUTH,
            password: PASS.try_into().unwrap(),
            ..Default::default()
        });

        wifi.set_configuration(&wifi_config)
            .expect("Failed to set Wi-Fi configuration!");
        log::warn!("Wi-Fi configuration set!");

        wifi.set_mac(WifiDeviceId::Sta, val.state.client_mac)
            .expect("Failed to set Wi-Fi MAC!");

        Self {
            state: WifiReady {
                eth: val.state.eth,
                wifi,
            },
        }
    }
}

/// Transition from [`WifiReady`] to [`Running`].
// don't care about panic due to try_from().unwrap()
#[allow(clippy::fallible_impl_from)]
impl From<Bridge<WifiReady>> for Bridge<Running> {
    fn from(val: Bridge<WifiReady>) -> Self {
        let mut eth = Box::new(val.state.eth);
        let mut wifi = Box::new(val.state.wifi);

        let wifi_config = Configuration::Client(ClientConfiguration {
            ssid: SSID.try_into().unwrap(),
            auth_method: AUTH,
            password: PASS.try_into().unwrap(),
            ..Default::default()
        });

        wifi.set_configuration(&wifi_config)
            .expect("Failed to set Wi-Fi configuration!");
        log::warn!("Wi-Fi configuration set!");

        let eth_ptr = &mut *eth as *mut EthDriver<'static, RmiiEth> as usize;
        unsafe {
            wifi.set_nonstatic_callbacks(
                {
                    let eth_ptr = eth_ptr;
                    move |_, frame| {
                        // SAFETY: eth stays alive while callbacks are registered
                        let eth = &mut *(eth_ptr as *mut EthDriver<'static, RmiiEth>);
                        if eth.is_connected().unwrap_or(false) {
                            eth.send(frame.as_slice())?;
                        } else {
                            log::debug!("Ethernet not connected!");
                        }
                        Ok(())
                    }
                },
                |_, _, _| {},
            )
            .expect("Failed to set Wi-Fi callbacks!");
        }

        let wifi_ptr = &mut *wifi as *mut WifiDriver<'static> as usize;
        unsafe {
            eth.set_nonstatic_rx_callback({
                let wifi_ptr = wifi_ptr;
                move |frame| {
                    // SAFETY: wifi stays alive while callbacks are registered
                    let wifi = &mut *(wifi_ptr as *mut WifiDriver<'static>);
                    if wifi.is_connected().unwrap_or(false) {
                        let _ = wifi.send(WifiDeviceId::Sta, frame.as_slice());
                    } else {
                        log::debug!("Wi-Fi not connected!");
                    }
                }
            })
            .expect("Failed to set Ethernet callback!");
        }

        wifi.start().expect("Failed to start Wi-Fi!");
        eth.start().expect("Failed to start Ethernet!");

        wifi.connect().expect("Failed to connect to Wi-Fi!");

        Self {
            state: Running {
                _eth: eth,
                _wifi: wifi,
            },
        }
    }
}

/// Format MAC bytes as a hex string.
///
/// E.g. `02:aa:bb:cc:12:34`
#[inline]
fn mac2str(mac: [u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}
