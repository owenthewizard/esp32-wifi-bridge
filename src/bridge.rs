//! Wi-Fi to Ethernet Bridge State Machine

extern crate alloc;
use alloc::{boxed::Box, format, string::String, sync::Arc};

use esp_idf_svc::{
    eth::{EthDriver, RmiiClockConfig, RmiiEth, RmiiEthChipset},
    eventloop::EspSystemEventLoop,
    // === HERE: Import 'delay' which was already available via main.rs ===
    hal::{delay, gpio, modem::Modem, prelude::Peripherals},
    nvs::EspDefaultNvsPartition,
    wifi::{AuthMethod, ClientConfiguration, Configuration, WifiDeviceId, WifiDriver},
};

use once_cell::sync::OnceCell;

// === HERE: Removed the old constants ===
// const SSID: &str = env!("WIFI_SSID");
// const PASS: &str = env!("WIFI_PASS");
// const AUTH: AuthMethod = AuthMethod::WPA2Personal;

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
    // ... (This function is unchanged from your original) ...
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
    // ... (This function is unchanged from your original) ...
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
        // === HERE: Add 'mut' to fix the compiler error from step 81 ===
        let mut wifi = WifiDriver::new(val.state.modem, val.state.sysloop.clone(), val.state.nvs)
            .expect("Failed to init WifiDriver!");

        // === MODIFIED (THE FIX) ===
        // We DO NOT set the configuration here. We just prepare the driver.
        // We *must* set the MAC *before* starting.
        wifi.set_mac(WifiDeviceId::Sta, val.state.client_mac)
            .expect("Failed to set Wi-Fi MAC!");
        // === END MODIFIED ===

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

        // === MODIFIED (THE FIX) ===
        // This is the correct logical order, as seen in the original:
        // 1. Set callbacks
        // 2. Start drivers
        // 3. Connect (with fallback loop)
        // === END MODIFIED ===

        // === STEP 1: Set up the callbacks (same as original code) ===
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
        
        // === STEP 2: Start Ethernet (same as original code) ===
        // Ethernet was already started, but we do it again to match the original logic.
        eth.start().expect("Failed to start Ethernet!");

        // === STEP 3: Start the Wi-Fi connection loop (NEW LOGIC) ===
        // === HERE: Define credentials list ===
        // We store the Options directly. This is allowed in a const context.
        const CREDENTIALS: &[(Option<&str>, Option<&str>)] = &[
            (
                option_env!("WIFI_SSID_1"),
                option_env!("WIFI_PASS_1"),
            ),
            (
                option_env!("WIFI_SSID_2"),
                option_env!("WIFI_PASS_2"),
            ),
        ];

        let mut connected = false;

        for (ssid_opt, pass_opt) in CREDENTIALS.iter() {
            let ssid = ssid_opt.unwrap_or("");
            let pass = pass_opt.unwrap_or("");

            if ssid.is_empty() {
                continue;
            }

            log::info!("Attempting connection to WiFi: '{}'", ssid);

            let wifi_config = Configuration::Client(ClientConfiguration {
                ssid: ssid.try_into().unwrap(),
                auth_method: AuthMethod::WPA2Personal,
                password: pass.try_into().unwrap(),
                ..Default::default()
            });

            wifi.set_configuration(&wifi_config)
                .expect("Failed to set Wi-Fi configuration!");
            
            // This matches the original logic
            wifi.start().expect("Failed to start Wi-Fi!");
            wifi.connect().expect("Failed to start Wi-Fi connect");

            log::info!("Waiting for connection...");
            for _ in 0..100 { // 10 second timeout
                if wifi.is_connected().unwrap_or(false) {
                    connected = true;
                    log::info!("Successfully connected to: '{}'", ssid);
                    break;
                }
                delay::FreeRtos::delay_ms(100); // Use the delay from main.rs
            }

            if connected {
                break; // Exit credentials loop
            } else {
                wifi.stop().expect("Failed to stop wifi");
                log::warn!("Connection to '{}' failed. Trying next...", ssid);
            }
        }

        if !connected {
            panic!("Could not connect to ANY of the provided WiFi networks.");
        }
        
        log::info!("Bridge is running.");

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