use std::collections::BTreeSet;
use std::fmt;
use std::fs;
#[cfg(target_os = "linux")]
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Mutex, TryLockError};
#[cfg(target_os = "linux")]
use std::thread;
use std::time::{Duration, Instant};

use cp0_radio_protocol::{
    MAX_LORA_PAYLOAD_BYTES, RadioCommand, RadioErrorCode, RadioProtocolError, RadioRequest,
    RadioResponse, decode_payload, read_request, write_response,
};

pub const DEFAULT_RADIO_CONFIG: &str = "/etc/cardputerzero/lora.conf";
pub const LORA_SPI_DEVICE: &str = "/dev/spidev0.1";
pub const LORA_BANDWIDTH_HZ: u32 = 125_000;
pub const LORA_SPREADING_FACTOR: u8 = 7;
pub const LORA_CODING_RATE_DENOMINATOR: u8 = 5;
pub const LORA_TX_POWER_DBM: u8 = 14;
pub const LORA_SYNC_WORD: u8 = 0x12;
pub const MIN_TRANSMIT_INTERVAL: Duration = Duration::from_secs(15);
const MAX_CONFIG_BYTES: u64 = 4096;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(4);
#[cfg(target_os = "linux")]
const TX_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(target_os = "linux")]
const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioRegion {
    Cn470,
    Eu868,
    Us915,
    Au915,
    As923,
    In865,
    Kr920,
    Ru864,
}

impl RadioRegion {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "cn470" => Some(Self::Cn470),
            "eu868" => Some(Self::Eu868),
            "us915" => Some(Self::Us915),
            "au915" => Some(Self::Au915),
            "as923" => Some(Self::As923),
            "in865" => Some(Self::In865),
            "kr920" => Some(Self::Kr920),
            "ru864" => Some(Self::Ru864),
            _ => None,
        }
    }

    pub const fn contains(self, frequency_hz: u32) -> bool {
        let (minimum, maximum) = match self {
            Self::Cn470 => (470_000_000, 510_000_000),
            Self::Eu868 => (863_000_000, 870_000_000),
            Self::Us915 | Self::Au915 => (902_000_000, 928_000_000),
            Self::As923 => (915_000_000, 928_000_000),
            Self::In865 => (865_000_000, 867_000_000),
            Self::Kr920 => (920_000_000, 923_000_000),
            Self::Ru864 => (864_000_000, 870_000_000),
        };
        frequency_hz >= minimum && frequency_hz <= maximum
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioConfig {
    Disabled,
    Enabled {
        region: RadioRegion,
        frequency_hz: u32,
    },
}

#[derive(Debug)]
pub enum RadioConfigError {
    Io(io::Error),
    InsecureFile,
    TooLarge,
    InvalidLine(usize),
    DuplicateKey(&'static str),
    MissingKey(&'static str),
    InvalidValue(&'static str),
    FrequencyOutsideRegion,
}

impl fmt::Display for RadioConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "radio config I/O error: {error}"),
            Self::InsecureFile => formatter.write_str(
                "radio config must be a root-owned regular file that is not writable by group or others",
            ),
            Self::TooLarge => write!(formatter, "radio config exceeds {MAX_CONFIG_BYTES} bytes"),
            Self::InvalidLine(line) => write!(formatter, "invalid radio config line {line}"),
            Self::DuplicateKey(key) => write!(formatter, "duplicate radio config key {key}"),
            Self::MissingKey(key) => write!(formatter, "missing radio config key {key}"),
            Self::InvalidValue(key) => write!(formatter, "invalid radio config value for {key}"),
            Self::FrequencyOutsideRegion => {
                formatter.write_str("radio frequency is outside the configured region")
            }
        }
    }
}

impl std::error::Error for RadioConfigError {}

impl From<io::Error> for RadioConfigError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn load_config(path: impl AsRef<Path>) -> Result<RadioConfig, RadioConfigError> {
    let path = path.as_ref();
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return Err(if metadata.len() > MAX_CONFIG_BYTES {
            RadioConfigError::TooLarge
        } else {
            RadioConfigError::InsecureFile
        });
    }
    #[cfg(target_os = "linux")]
    if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(RadioConfigError::InsecureFile);
    }
    parse_config(&fs::read_to_string(path)?)
}

pub fn parse_config(input: &str) -> Result<RadioConfig, RadioConfigError> {
    if input.len() as u64 > MAX_CONFIG_BYTES {
        return Err(RadioConfigError::TooLarge);
    }
    let mut enabled = None;
    let mut region = None;
    let mut frequency_hz = None;
    for (index, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(RadioConfigError::InvalidLine(index + 1))?;
        if key.trim() != key || value.trim() != value || key.is_empty() || value.is_empty() {
            return Err(RadioConfigError::InvalidLine(index + 1));
        }
        match key {
            "enabled" => {
                if enabled.is_some() {
                    return Err(RadioConfigError::DuplicateKey("enabled"));
                }
                enabled = Some(match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err(RadioConfigError::InvalidValue("enabled")),
                });
            }
            "region" => {
                if region.is_some() {
                    return Err(RadioConfigError::DuplicateKey("region"));
                }
                region = Some(
                    RadioRegion::parse(value).ok_or(RadioConfigError::InvalidValue("region"))?,
                );
            }
            "frequency_hz" => {
                if frequency_hz.is_some() {
                    return Err(RadioConfigError::DuplicateKey("frequency_hz"));
                }
                if !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(RadioConfigError::InvalidValue("frequency_hz"));
                }
                frequency_hz = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| RadioConfigError::InvalidValue("frequency_hz"))?,
                );
            }
            _ => return Err(RadioConfigError::InvalidLine(index + 1)),
        }
    }
    let enabled = enabled.ok_or(RadioConfigError::MissingKey("enabled"))?;
    if !enabled {
        if region.is_some() || frequency_hz.is_some() {
            return Err(RadioConfigError::InvalidValue("enabled"));
        }
        return Ok(RadioConfig::Disabled);
    }
    let region = region.ok_or(RadioConfigError::MissingKey("region"))?;
    let frequency_hz = frequency_hz.ok_or(RadioConfigError::MissingKey("frequency_hz"))?;
    if !region.contains(frequency_hz) {
        return Err(RadioConfigError::FrequencyOutsideRegion);
    }
    Ok(RadioConfig::Enabled {
        region,
        frequency_hz,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedPacket {
    pub payload: Vec<u8>,
    pub rssi_dbm: i16,
    pub snr_quarter_db: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioDeviceError {
    Disabled,
    Busy,
    RateLimited,
    Unavailable,
    Device,
    TimedOut,
    Internal,
}

impl fmt::Display for RadioDeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => formatter.write_str("LoRa radio is disabled"),
            Self::Busy => formatter.write_str("LoRa radio is busy"),
            Self::RateLimited => formatter.write_str("LoRa transmission is rate limited"),
            Self::Unavailable => formatter.write_str("LoRa radio is unavailable"),
            Self::Device => formatter.write_str("LoRa radio operation failed"),
            Self::TimedOut => formatter.write_str("LoRa radio operation timed out"),
            Self::Internal => formatter.write_str("LoRa radio service failed internally"),
        }
    }
}

impl std::error::Error for RadioDeviceError {}

pub trait RadioBackend {
    fn send_lora(&self, payload: &[u8]) -> Result<(), RadioDeviceError>;
    fn receive_lora(&self, timeout: Duration) -> Result<Option<ReceivedPacket>, RadioDeviceError>;
}

#[derive(Debug)]
pub enum ConfiguredRadio {
    Disabled,
    Sx127x(Sx127xRadio),
}

impl ConfiguredRadio {
    pub fn new(config: RadioConfig) -> Self {
        match config {
            RadioConfig::Disabled => Self::Disabled,
            RadioConfig::Enabled { frequency_hz, .. } => {
                Self::Sx127x(Sx127xRadio::new(frequency_hz))
            }
        }
    }
}

impl RadioBackend for ConfiguredRadio {
    fn send_lora(&self, payload: &[u8]) -> Result<(), RadioDeviceError> {
        match self {
            Self::Disabled => Err(RadioDeviceError::Disabled),
            Self::Sx127x(radio) => radio.send_lora(payload),
        }
    }

    fn receive_lora(&self, timeout: Duration) -> Result<Option<ReceivedPacket>, RadioDeviceError> {
        match self {
            Self::Disabled => Err(RadioDeviceError::Disabled),
            Self::Sx127x(radio) => radio.receive_lora(timeout),
        }
    }
}

#[derive(Debug)]
struct RadioState {
    last_transmit: Option<Instant>,
}

#[derive(Debug)]
pub struct Sx127xRadio {
    frequency_hz: u32,
    state: Mutex<RadioState>,
}

impl Sx127xRadio {
    pub fn new(frequency_hz: u32) -> Self {
        Self {
            frequency_hz,
            state: Mutex::new(RadioState {
                last_transmit: None,
            }),
        }
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, RadioState>, RadioDeviceError> {
        match self.state.try_lock() {
            Ok(state) => Ok(state),
            Err(TryLockError::WouldBlock) => Err(RadioDeviceError::Busy),
            Err(TryLockError::Poisoned(_)) => Err(RadioDeviceError::Internal),
        }
    }
}

impl RadioBackend for Sx127xRadio {
    fn send_lora(&self, payload: &[u8]) -> Result<(), RadioDeviceError> {
        if payload.is_empty() || payload.len() > MAX_LORA_PAYLOAD_BYTES {
            return Err(RadioDeviceError::Device);
        }
        let mut state = self.lock_state()?;
        let now = Instant::now();
        if !transmit_allowed(state.last_transmit, now) {
            return Err(RadioDeviceError::RateLimited);
        }
        let mut device = Sx127xDevice::open(self.frequency_hz)?;
        device.transmit(payload)?;
        state.last_transmit = Some(Instant::now());
        Ok(())
    }

    fn receive_lora(&self, timeout: Duration) -> Result<Option<ReceivedPacket>, RadioDeviceError> {
        let _state = self.lock_state()?;
        let mut device = Sx127xDevice::open(self.frequency_hz)?;
        device.receive(timeout)
    }
}

fn transmit_allowed(last_transmit: Option<Instant>, now: Instant) -> bool {
    last_transmit.is_none_or(|last| now.saturating_duration_since(last) >= MIN_TRANSMIT_INTERVAL)
}

#[derive(Debug)]
pub struct RadioServer<B> {
    backend: B,
    trusted_uids: BTreeSet<u32>,
}

impl<B: RadioBackend> RadioServer<B> {
    pub fn new(backend: B, trusted_uids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            backend,
            trusted_uids: trusted_uids.into_iter().collect(),
        }
    }

    pub fn serve(&self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-radiod: rejected connection: {error}");
            }
        }
    }

    fn handle_connection(&self, mut stream: UnixStream) -> io::Result<()> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let uid = peer_uid(&stream)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => {
                write_response(
                    &mut stream,
                    &RadioResponse::error(
                        0,
                        RadioErrorCode::InvalidRequest,
                        "invalid radio service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-radiod: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            write_response(
                &mut stream,
                &RadioResponse::error(
                    request.request_id,
                    RadioErrorCode::Unauthorized,
                    "peer UID is not authorized for radio access",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        write_response(&mut stream, &self.dispatch(request)).map_err(protocol_io)
    }

    pub fn dispatch(&self, request: RadioRequest) -> RadioResponse {
        let request_id = request.request_id;
        match request.command {
            RadioCommand::SendLora { payload_base64 } => {
                let payload = match decode_payload(&payload_base64) {
                    Ok(payload) => payload,
                    Err(_) => {
                        return RadioResponse::error(
                            request_id,
                            RadioErrorCode::InvalidRequest,
                            "invalid bounded LoRa payload",
                        );
                    }
                };
                match self.backend.send_lora(&payload) {
                    Ok(()) => RadioResponse::lora_sent(request_id, payload.len() as u8),
                    Err(error) => device_error_response(request_id, error),
                }
            }
            RadioCommand::ReceiveLora { timeout_ms } => {
                match self
                    .backend
                    .receive_lora(Duration::from_millis(u64::from(timeout_ms)))
                {
                    Ok(Some(packet))
                        if !packet.payload.is_empty()
                            && packet.payload.len() <= MAX_LORA_PAYLOAD_BYTES =>
                    {
                        RadioResponse::lora_packet(
                            request_id,
                            &packet.payload,
                            packet.rssi_dbm,
                            packet.snr_quarter_db,
                        )
                    }
                    Ok(Some(_)) => RadioResponse::error(
                        request_id,
                        RadioErrorCode::Internal,
                        "radio device returned an invalid packet",
                    ),
                    Ok(None) => RadioResponse::no_lora_packet(request_id),
                    Err(error) => device_error_response(request_id, error),
                }
            }
        }
    }
}

fn device_error_response(request_id: u64, error: RadioDeviceError) -> RadioResponse {
    let (code, message) = match error {
        RadioDeviceError::Disabled => (RadioErrorCode::Disabled, "LoRa radio is disabled"),
        RadioDeviceError::Busy => (RadioErrorCode::Busy, "LoRa radio is busy"),
        RadioDeviceError::RateLimited => (
            RadioErrorCode::RateLimited,
            "LoRa transmission is rate limited",
        ),
        RadioDeviceError::Unavailable => (RadioErrorCode::Unavailable, "LoRa radio is unavailable"),
        RadioDeviceError::TimedOut => (RadioErrorCode::Device, "LoRa transmission timed out"),
        RadioDeviceError::Device => (RadioErrorCode::Device, "LoRa radio operation failed"),
        RadioDeviceError::Internal => (
            RadioErrorCode::Internal,
            "LoRa radio service failed internally",
        ),
    };
    RadioResponse::error(request_id, code, message)
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an unexpected size",
        ));
    }
    Ok(credentials.uid)
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "peer credentials are only implemented for the Linux target",
    ))
}

fn protocol_io(error: RadioProtocolError) -> io::Error {
    match error {
        RadioProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(target_os = "linux")]
const REG_FIFO: u8 = 0x00;
#[cfg(target_os = "linux")]
const REG_OP_MODE: u8 = 0x01;
#[cfg(target_os = "linux")]
const REG_FRF_MSB: u8 = 0x06;
#[cfg(target_os = "linux")]
const REG_PA_CONFIG: u8 = 0x09;
#[cfg(target_os = "linux")]
const REG_OCP: u8 = 0x0b;
#[cfg(target_os = "linux")]
const REG_LNA: u8 = 0x0c;
#[cfg(target_os = "linux")]
const REG_FIFO_ADDR_PTR: u8 = 0x0d;
#[cfg(target_os = "linux")]
const REG_FIFO_TX_BASE_ADDR: u8 = 0x0e;
#[cfg(target_os = "linux")]
const REG_FIFO_RX_BASE_ADDR: u8 = 0x0f;
#[cfg(target_os = "linux")]
const REG_FIFO_RX_CURRENT_ADDR: u8 = 0x10;
#[cfg(target_os = "linux")]
const REG_IRQ_FLAGS: u8 = 0x12;
#[cfg(target_os = "linux")]
const REG_RX_NB_BYTES: u8 = 0x13;
#[cfg(target_os = "linux")]
const REG_PKT_SNR_VALUE: u8 = 0x19;
#[cfg(target_os = "linux")]
const REG_PKT_RSSI_VALUE: u8 = 0x1a;
#[cfg(target_os = "linux")]
const REG_MODEM_CONFIG_1: u8 = 0x1d;
#[cfg(target_os = "linux")]
const REG_MODEM_CONFIG_2: u8 = 0x1e;
#[cfg(target_os = "linux")]
const REG_SYMB_TIMEOUT_LSB: u8 = 0x1f;
#[cfg(target_os = "linux")]
const REG_PREAMBLE_MSB: u8 = 0x20;
#[cfg(target_os = "linux")]
const REG_PREAMBLE_LSB: u8 = 0x21;
#[cfg(target_os = "linux")]
const REG_PAYLOAD_LENGTH: u8 = 0x22;
#[cfg(target_os = "linux")]
const REG_MAX_PAYLOAD_LENGTH: u8 = 0x23;
#[cfg(target_os = "linux")]
const REG_MODEM_CONFIG_3: u8 = 0x26;
#[cfg(target_os = "linux")]
const REG_SYNC_WORD: u8 = 0x39;
#[cfg(target_os = "linux")]
const REG_VERSION: u8 = 0x42;
#[cfg(target_os = "linux")]
const SX1276_VERSION: u8 = 0x12;
#[cfg(target_os = "linux")]
const MODE_LORA_SLEEP: u8 = 0x80;
#[cfg(target_os = "linux")]
const MODE_LORA_STANDBY: u8 = 0x81;
#[cfg(target_os = "linux")]
const MODE_LORA_TX: u8 = 0x83;
#[cfg(target_os = "linux")]
const MODE_LORA_RX_SINGLE: u8 = 0x86;
#[cfg(target_os = "linux")]
const IRQ_RX_TIMEOUT: u8 = 0x80;
#[cfg(target_os = "linux")]
const IRQ_RX_DONE: u8 = 0x40;
#[cfg(target_os = "linux")]
const IRQ_PAYLOAD_CRC_ERROR: u8 = 0x20;
#[cfg(target_os = "linux")]
const IRQ_TX_DONE: u8 = 0x08;
#[cfg(target_os = "linux")]
const SPI_SPEED_HZ: u32 = 8_000_000;
#[cfg(any(target_os = "linux", test))]
const SX127X_CRYSTAL_HZ: u64 = 32_000_000;

#[cfg(any(target_os = "linux", test))]
fn frequency_register(frequency_hz: u32) -> u32 {
    (((u64::from(frequency_hz) << 19) + SX127X_CRYSTAL_HZ / 2) / SX127X_CRYSTAL_HZ) as u32
}

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Debug, Default)]
struct SpiIocTransfer {
    tx_buf: u64,
    rx_buf: u64,
    len: u32,
    speed_hz: u32,
    delay_usecs: u16,
    bits_per_word: u8,
    cs_change: u8,
    tx_nbits: u8,
    rx_nbits: u8,
    word_delay_usecs: u8,
    pad: u8,
}

#[cfg(target_os = "linux")]
const fn ioc_write(kind: u8, number: u8, size: usize) -> libc::c_ulong {
    ((1_u64 << 30) | ((size as u64) << 16) | ((kind as u64) << 8) | number as u64) as libc::c_ulong
}

#[cfg(target_os = "linux")]
const SPI_IOC_WR_MODE: libc::c_ulong = ioc_write(b'k', 1, 1);
#[cfg(target_os = "linux")]
const SPI_IOC_WR_BITS_PER_WORD: libc::c_ulong = ioc_write(b'k', 3, 1);
#[cfg(target_os = "linux")]
const SPI_IOC_WR_MAX_SPEED_HZ: libc::c_ulong = ioc_write(b'k', 4, 4);
#[cfg(target_os = "linux")]
const SPI_IOC_MESSAGE_1: libc::c_ulong = ioc_write(b'k', 0, std::mem::size_of::<SpiIocTransfer>());

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct Sx127xDevice {
    file: File,
    frequency_hz: u32,
}

#[cfg(target_os = "linux")]
impl Sx127xDevice {
    fn open(frequency_hz: u32) -> Result<Self, RadioDeviceError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(LORA_SPI_DEVICE)
            .map_err(map_device_io)?;
        let device = Self { file, frequency_hz };
        device.configure_spi()?;
        let mut device = device;
        device.initialize()?;
        Ok(device)
    }

    fn configure_spi(&self) -> Result<(), RadioDeviceError> {
        let mode = 0_u8;
        let bits = 8_u8;
        let speed = SPI_SPEED_HZ;
        for (request, value) in [
            (SPI_IOC_WR_MODE, (&raw const mode).cast::<libc::c_void>()),
            (
                SPI_IOC_WR_BITS_PER_WORD,
                (&raw const bits).cast::<libc::c_void>(),
            ),
            (
                SPI_IOC_WR_MAX_SPEED_HZ,
                (&raw const speed).cast::<libc::c_void>(),
            ),
        ] {
            if unsafe { libc::ioctl(self.file.as_raw_fd(), request, value) } != 0 {
                return Err(map_device_io(io::Error::last_os_error()));
            }
        }
        Ok(())
    }

    fn initialize(&mut self) -> Result<(), RadioDeviceError> {
        if self.read_register(REG_VERSION)? != SX1276_VERSION {
            return Err(RadioDeviceError::Unavailable);
        }
        self.write_register(REG_OP_MODE, MODE_LORA_SLEEP)?;
        thread::sleep(Duration::from_millis(10));
        self.write_register(REG_OP_MODE, MODE_LORA_STANDBY)?;

        let frequency = frequency_register(self.frequency_hz);
        self.write_register(REG_FRF_MSB, (frequency >> 16) as u8)?;
        self.write_register(REG_FRF_MSB + 1, (frequency >> 8) as u8)?;
        self.write_register(REG_FRF_MSB + 2, frequency as u8)?;
        self.write_register(REG_FIFO_TX_BASE_ADDR, 0)?;
        self.write_register(REG_FIFO_RX_BASE_ADDR, 0)?;
        let lna = self.read_register(REG_LNA)?;
        self.write_register(REG_LNA, lna | 0x03)?;
        self.write_register(REG_MODEM_CONFIG_1, 0x72)?;
        self.write_register(REG_MODEM_CONFIG_2, 0x77)?;
        self.write_register(REG_SYMB_TIMEOUT_LSB, 0xff)?;
        self.write_register(REG_MODEM_CONFIG_3, 0x04)?;
        self.write_register(REG_PREAMBLE_MSB, 0)?;
        self.write_register(REG_PREAMBLE_LSB, 8)?;
        self.write_register(REG_MAX_PAYLOAD_LENGTH, MAX_LORA_PAYLOAD_BYTES as u8)?;
        self.write_register(REG_SYNC_WORD, LORA_SYNC_WORD)?;
        self.write_register(REG_OCP, 0x2b)?;
        self.write_register(REG_PA_CONFIG, 0x80 | (LORA_TX_POWER_DBM - 2))?;
        self.write_register(REG_IRQ_FLAGS, 0xff)?;
        Ok(())
    }

    fn transmit(&mut self, payload: &[u8]) -> Result<(), RadioDeviceError> {
        self.write_register(REG_OP_MODE, MODE_LORA_STANDBY)?;
        self.write_register(REG_IRQ_FLAGS, 0xff)?;
        self.write_register(REG_FIFO_ADDR_PTR, 0)?;
        self.write_fifo(payload)?;
        self.write_register(REG_PAYLOAD_LENGTH, payload.len() as u8)?;
        self.write_register(REG_OP_MODE, MODE_LORA_TX)?;
        let deadline = Instant::now() + TX_TIMEOUT;
        loop {
            let flags = self.read_register(REG_IRQ_FLAGS)?;
            if flags & IRQ_TX_DONE != 0 {
                self.write_register(REG_IRQ_FLAGS, IRQ_TX_DONE)?;
                self.write_register(REG_OP_MODE, MODE_LORA_STANDBY)?;
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.write_register(REG_OP_MODE, MODE_LORA_STANDBY)?;
                return Err(RadioDeviceError::TimedOut);
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn receive(&mut self, timeout: Duration) -> Result<Option<ReceivedPacket>, RadioDeviceError> {
        self.write_register(REG_OP_MODE, MODE_LORA_STANDBY)?;
        self.write_register(REG_IRQ_FLAGS, 0xff)?;
        self.write_register(REG_FIFO_ADDR_PTR, 0)?;
        self.write_register(REG_OP_MODE, MODE_LORA_RX_SINGLE)?;
        let deadline = Instant::now() + timeout;
        loop {
            let flags = self.read_register(REG_IRQ_FLAGS)?;
            if flags & IRQ_RX_DONE != 0 {
                self.write_register(REG_IRQ_FLAGS, flags)?;
                self.write_register(REG_OP_MODE, MODE_LORA_STANDBY)?;
                if flags & IRQ_PAYLOAD_CRC_ERROR != 0 {
                    return Ok(None);
                }
                let length = usize::from(self.read_register(REG_RX_NB_BYTES)?);
                if length == 0 || length > MAX_LORA_PAYLOAD_BYTES {
                    return Err(RadioDeviceError::Device);
                }
                let address = self.read_register(REG_FIFO_RX_CURRENT_ADDR)?;
                self.write_register(REG_FIFO_ADDR_PTR, address)?;
                let payload = self.read_fifo(length)?;
                let snr_quarter_db = self.read_register(REG_PKT_SNR_VALUE)? as i8;
                let offset = if self.frequency_hz < 779_000_000 {
                    -164
                } else {
                    -157
                };
                let mut rssi_dbm = i16::from(self.read_register(REG_PKT_RSSI_VALUE)?) + offset;
                if snr_quarter_db < 0 {
                    rssi_dbm += i16::from(snr_quarter_db) / 4;
                }
                return Ok(Some(ReceivedPacket {
                    payload,
                    rssi_dbm,
                    snr_quarter_db,
                }));
            }
            if flags & IRQ_RX_TIMEOUT != 0 || Instant::now() >= deadline {
                self.write_register(REG_IRQ_FLAGS, flags | IRQ_RX_TIMEOUT)?;
                self.write_register(REG_OP_MODE, MODE_LORA_STANDBY)?;
                return Ok(None);
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn read_register(&mut self, register: u8) -> Result<u8, RadioDeviceError> {
        let mut bytes = [register & 0x7f, 0];
        self.transfer(&mut bytes)?;
        Ok(bytes[1])
    }

    fn write_register(&mut self, register: u8, value: u8) -> Result<(), RadioDeviceError> {
        let mut bytes = [register | 0x80, value];
        self.transfer(&mut bytes)
    }

    fn write_fifo(&mut self, payload: &[u8]) -> Result<(), RadioDeviceError> {
        let mut bytes = Vec::with_capacity(payload.len() + 1);
        bytes.push(REG_FIFO | 0x80);
        bytes.extend_from_slice(payload);
        self.transfer(&mut bytes)
    }

    fn read_fifo(&mut self, length: usize) -> Result<Vec<u8>, RadioDeviceError> {
        let mut bytes = vec![0_u8; length + 1];
        bytes[0] = REG_FIFO & 0x7f;
        self.transfer(&mut bytes)?;
        Ok(bytes[1..].to_vec())
    }

    fn transfer(&mut self, bytes: &mut [u8]) -> Result<(), RadioDeviceError> {
        let transmit = bytes.to_vec();
        let mut transfer = SpiIocTransfer {
            tx_buf: transmit.as_ptr() as u64,
            rx_buf: bytes.as_mut_ptr() as u64,
            len: bytes.len() as u32,
            speed_hz: SPI_SPEED_HZ,
            bits_per_word: 8,
            ..SpiIocTransfer::default()
        };
        let result =
            unsafe { libc::ioctl(self.file.as_raw_fd(), SPI_IOC_MESSAGE_1, &raw mut transfer) };
        if result < 0 {
            return Err(map_device_io(io::Error::last_os_error()));
        }
        if result as usize != bytes.len() {
            return Err(RadioDeviceError::Device);
        }
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
struct Sx127xDevice;

#[cfg(not(target_os = "linux"))]
impl Sx127xDevice {
    fn open(_frequency_hz: u32) -> Result<Self, RadioDeviceError> {
        Err(RadioDeviceError::Unavailable)
    }

    fn transmit(&mut self, _payload: &[u8]) -> Result<(), RadioDeviceError> {
        Err(RadioDeviceError::Unavailable)
    }

    fn receive(&mut self, _timeout: Duration) -> Result<Option<ReceivedPacket>, RadioDeviceError> {
        Err(RadioDeviceError::Unavailable)
    }
}

#[cfg(target_os = "linux")]
fn map_device_io(error: io::Error) -> RadioDeviceError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => RadioDeviceError::Unavailable,
        io::ErrorKind::WouldBlock => RadioDeviceError::Busy,
        _ => RadioDeviceError::Device,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use cp0_radio_protocol::RadioOutcome;

    use super::*;

    #[derive(Debug, Default)]
    struct MockRadio {
        sent: RefCell<Vec<u8>>,
        packet: RefCell<Option<ReceivedPacket>>,
    }

    impl RadioBackend for MockRadio {
        fn send_lora(&self, payload: &[u8]) -> Result<(), RadioDeviceError> {
            self.sent.borrow_mut().extend_from_slice(payload);
            Ok(())
        }

        fn receive_lora(
            &self,
            _timeout: Duration,
        ) -> Result<Option<ReceivedPacket>, RadioDeviceError> {
            Ok(self.packet.borrow_mut().take())
        }
    }

    #[test]
    fn config_is_disabled_by_default_and_region_bounded_when_enabled() {
        assert_eq!(
            parse_config("enabled=false\n").unwrap(),
            RadioConfig::Disabled
        );
        assert_eq!(
            parse_config("enabled=true\nregion=cn470\nfrequency_hz=470300000\n").unwrap(),
            RadioConfig::Enabled {
                region: RadioRegion::Cn470,
                frequency_hz: 470_300_000,
            }
        );
        assert!(parse_config("enabled=true\nregion=eu868\nfrequency_hz=470300000\n").is_err());
        assert!(parse_config("enabled=false\nfrequency_hz=470300000\n").is_err());
        assert!(parse_config("enabled=true\nregion=cn470\npath=/dev/spidev0.2\n").is_err());
    }

    #[test]
    fn frequency_and_rate_limits_are_stable() {
        assert_eq!(frequency_register(868_100_000), 0x00d9_0666);
        let now = Instant::now();
        assert!(transmit_allowed(None, now));
        assert!(!transmit_allowed(Some(now), now + Duration::from_secs(14)));
        assert!(transmit_allowed(Some(now), now + MIN_TRANSMIT_INTERVAL));
    }

    #[test]
    fn dispatches_bounded_send_receive_and_no_packet() {
        let server = RadioServer::new(
            MockRadio {
                sent: RefCell::new(Vec::new()),
                packet: RefCell::new(Some(ReceivedPacket {
                    payload: b"hello".to_vec(),
                    rssi_dbm: -92,
                    snr_quarter_db: 7,
                })),
            },
            [0],
        );
        assert_eq!(
            server.dispatch(RadioRequest::send_lora(1, b"ping")).outcome,
            RadioOutcome::LoraSent { bytes: 4 }
        );
        assert_eq!(&*server.backend.sent.borrow(), b"ping");
        let response = server.dispatch(RadioRequest::receive_lora(2, 100));
        let RadioOutcome::LoraPacket {
            payload_base64,
            rssi_dbm,
            snr_quarter_db,
        } = response.outcome
        else {
            panic!("expected LoRa packet")
        };
        assert_eq!(decode_payload(&payload_base64).unwrap(), b"hello");
        assert_eq!((rssi_dbm, snr_quarter_db), (-92, 7));
        assert_eq!(
            server.dispatch(RadioRequest::receive_lora(3, 100)).outcome,
            RadioOutcome::LoraNoPacket
        );
    }
}
