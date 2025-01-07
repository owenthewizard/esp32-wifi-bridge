//! ESP32 Wi-Fi to Ethernet Transparent Bridge

#![feature(never_type)]
#![warn(clippy::undocumented_unsafe_blocks, clippy::pedantic, clippy::nursery)]

mod bridge;
#[allow(clippy::wildcard_imports)]
use bridge::*;

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let idle = Bridge::new();
    let ethup = Bridge::<EthReady>::from(idle);
    let wifiup = Bridge::<WifiReady>::from(ethup);
    let _running = Bridge::<Running>::from(wifiup);
}
