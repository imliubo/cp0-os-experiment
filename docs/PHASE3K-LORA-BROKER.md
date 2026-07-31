# Phase 3K: Restricted LoRa broker

CardputerZero V0.6 does not contain an onboard LoRa radio. This phase supports
an external SX1276-family module on SPI0 chip select 1. The display remains on
chip select 0; applications cannot select either endpoint.

## Frozen API

The `radio.lora` capability exposes only bounded packet send and receive:

- payloads contain 1 through 64 bytes;
- receive waits from 1 through 1000 milliseconds;
- received metadata contains signed RSSI in dBm and signed SNR in quarter-dB;
- no raw SPI path, GPIO, register, frequency, power or modulation setting is
  accepted from an application;
- at least 15 seconds must pass between successful transmissions.

The radio parameters are fixed to 125 kHz bandwidth, spreading factor 7,
coding rate 4/5, CRC enabled, an 8-symbol preamble, private sync word `0x12`
and 14 dBm transmit power. Rust, C11, C++17 and WIT SDK contracts expose the
same bounds. Hello Card's `L` action is receive-only.

## Trust path

```text
WASM radio SDK call
  -> Runtime validates linear-memory ranges and fixed bounds
  -> appd binds peer UID/cgroup to the running installed application
  -> appd verifies the root-owned manifest and radio.lora decision
  -> root-only cp0-radiod socket accepts only appd
  -> cp0-radiod serializes operations on fixed /dev/spidev0.1
  -> SX1276 fixed-register driver
```

`cp0-radiod` runs as the dedicated `cp0-radio` account with supplementary
membership in `spi`. Its systemd unit uses `DevicePolicy=closed` and allows
only `/dev/spidev0.1`; it has no capabilities, network access or writable
system paths. Applications remain inside their existing Unix-only sandbox and
never receive the device descriptor.

## Regulatory configuration

The image installs `/etc/cardputerzero/lora.conf` as `0644 root:root` with:

```text
enabled=false
```

Enabling the radio requires both a supported region and a frequency inside its
compiled range, for example:

```text
enabled=true
region=eu868
frequency_hz=868100000
```

Supported region identifiers are `cn470`, `eu868`, `us915`, `au915`, `as923`,
`in865`, `kr920` and `ru864`. This range validation does not replace local duty
cycle, channel plan, antenna or certification requirements. Production setup
must choose the legally applicable region and frequency.

## Verification

The workspace tests cover strict protocol framing, canonical Base64, payload
and timeout bounds, region/frequency validation, rate limiting, packet
metadata, broker authorization routing, Runtime JSON decoding, SDK compilation
and image/service hardening. The Linux SX1276 path is also cross-compiled for
AArch64.

Physical receive/transmit acceptance remains open until an SX1276 module is
connected and the applicable legal frequency is confirmed. The default image
cannot transmit because the service configuration is disabled.
