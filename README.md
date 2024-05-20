# esp32-wifi-bridge

Wi-Fi to Ethernet bridge targeting the ESP32.

## Quick Start

Build it yourself:
```bash
export WIFI_SSID="My Awesome Network" WIFI_PASS="hunter2"
git clone --depth=1 https://github.com/owenthewizard/esp32-wifi-bridge.git && cd esp32-wifi-bridge
cargo run --release # flash to esp32
```

## Performance

In my testing, approximately 50 Mbps symmetrical throughput can be attained.

### Coding Style

Obey `rustfmt` and Rust 2021 conventions, as well as `clippy` lints.

## Contributing

Pull requests are always welcome.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be licensed under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or any later version.

## Versioning

At the moment, this project does not have a stable versioning scheme.

Changes will be documented in the [Changelog](CHANGELOG.md) on a best-effort basis.

See the [tags](https://github.com/owenthewizard/esp32-wifi-bridge/tags) for available releases.

## Authors

See [the list of contributors](https://github.com/owenthewizard/i3lockr/contributors).

## License

Copyright (C) 2024 Owen Walpole

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <http://www.gnu.org/licenses/>.

## Acknowledgments

[esp-rs](https://github.com/esp-rs/) and [Espressif](https://www.espressif.com/) for their stellar support of Rust.
