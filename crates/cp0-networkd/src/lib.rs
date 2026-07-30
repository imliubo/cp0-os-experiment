use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, BufReader};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

use cp0_network_protocol::{
    MAX_NETWORK_BODY_BYTES, NetworkCommand, NetworkErrorCode, NetworkProtocolError, NetworkRequest,
    NetworkResponse, read_request, write_response,
};
use ureq::config::Config;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};
use ureq::{Agent, Error as UreqError};

pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_REDIRECTS: u32 = 2;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResult {
    pub status_code: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchError {
    pub code: NetworkErrorCode,
    pub message: &'static str,
}

pub trait HttpFetcher: fmt::Debug + Send + Sync + 'static {
    fn get(&self, url: &str) -> Result<HttpResult, FetchError>;
}

#[derive(Debug, Clone)]
pub struct UreqFetcher {
    agent: Agent,
}

impl Default for UreqFetcher {
    fn default() -> Self {
        let config = network_config();
        Self {
            agent: Agent::with_parts(
                config,
                DefaultConnector::default(),
                PublicResolver::default(),
            ),
        }
    }
}

impl HttpFetcher for UreqFetcher {
    fn get(&self, url: &str) -> Result<HttpResult, FetchError> {
        validate_destination_url(url)?;
        let mut response = self.agent.get(url).call().map_err(map_ureq_error)?;
        let status_code = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_NETWORK_BODY_BYTES as u64 + 1)
            .read_to_vec()
            .map_err(map_ureq_error)?;
        if body.len() > MAX_NETWORK_BODY_BYTES {
            return Err(FetchError {
                code: NetworkErrorCode::ResponseTooLarge,
                message: "HTTPS response body exceeds 2048 bytes",
            });
        }
        Ok(HttpResult { status_code, body })
    }
}

fn validate_destination_url(url: &str) -> Result<(), FetchError> {
    let invalid = || FetchError {
        code: NetworkErrorCode::InvalidRequest,
        message: "invalid or non-HTTPS URL",
    };
    let blocked = || FetchError {
        code: NetworkErrorCode::BlockedAddress,
        message: "destination resolved to a non-public address",
    };
    let uri: ureq::http::Uri = url.parse().map_err(|_| invalid())?;
    if uri.scheme_str() != Some("https") {
        return Err(invalid());
    }
    let authority = uri.authority().ok_or_else(invalid)?;
    if authority.as_str().contains('@') {
        return Err(invalid());
    }
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err(invalid());
    }
    if let Ok(address) = host.parse::<IpAddr>() {
        return is_public_address(address).then_some(()).ok_or_else(blocked);
    }
    if !host.contains('.')
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host == "home.arpa"
        || host.ends_with(".home.arpa")
    {
        return Err(blocked());
    }
    Ok(())
}

fn network_config() -> Config {
    Config::builder()
        .https_only(true)
        .proxy(None)
        .http_status_as_error(false)
        .max_redirects(MAX_REDIRECTS)
        .max_redirects_will_error(true)
        .timeout_global(Some(NETWORK_TIMEOUT))
        .max_response_header_size(16 * 1024)
        .max_idle_connections(1)
        .max_idle_connections_per_host(1)
        .build()
}

fn map_ureq_error(error: UreqError) -> FetchError {
    let (code, message) = match &error {
        UreqError::BadUri(_) | UreqError::RequireHttpsOnly(_) | UreqError::Http(_) => {
            (NetworkErrorCode::InvalidRequest, "invalid or non-HTTPS URL")
        }
        UreqError::Other(source) if source.downcast_ref::<BlockedAddress>().is_some() => (
            NetworkErrorCode::BlockedAddress,
            "destination resolved to a non-public address",
        ),
        UreqError::Timeout(_) => (NetworkErrorCode::Timeout, "HTTPS request timed out"),
        UreqError::TooManyRedirects | UreqError::RedirectFailed => (
            NetworkErrorCode::TooManyRedirects,
            "HTTPS redirect limit was exceeded",
        ),
        UreqError::BodyExceedsLimit(_) => (
            NetworkErrorCode::ResponseTooLarge,
            "HTTPS response body exceeds 2048 bytes",
        ),
        UreqError::Tls(_) | UreqError::Rustls(_) | UreqError::TlsRequired => (
            NetworkErrorCode::Tls,
            "HTTPS certificate or TLS validation failed",
        ),
        UreqError::HostNotFound
        | UreqError::Io(_)
        | UreqError::Protocol(_)
        | UreqError::ConnectionFailed
        | UreqError::ConnectProxyFailed(_) => (
            NetworkErrorCode::Unavailable,
            "HTTPS destination is unavailable",
        ),
        _ => (
            NetworkErrorCode::Internal,
            "network service could not complete the request",
        ),
    };
    FetchError { code, message }
}

#[derive(Default)]
pub struct PublicResolver {
    inner: DefaultResolver,
}

impl fmt::Debug for PublicResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PublicResolver")
    }
}

impl Resolver for PublicResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, UreqError> {
        let resolved = self.inner.resolve(uri, config, timeout)?;
        let mut filtered = self.empty();
        let mut blocked = false;
        for address in resolved.iter().copied() {
            if is_public_address(address.ip()) {
                filtered.push(address);
            } else {
                blocked = true;
            }
        }
        if filtered.is_empty() {
            if blocked {
                Err(UreqError::Other(Box::new(BlockedAddress)))
            } else {
                Err(UreqError::HostNotFound)
            }
        } else {
            Ok(filtered)
        }
    }
}

#[derive(Debug)]
struct BlockedAddress;

impl fmt::Display for BlockedAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("destination address is not publicly routable")
    }
}

impl std::error::Error for BlockedAddress {}

pub fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                is_public_ipv4(mapped)
            } else {
                is_public_ipv6(address)
            }
        }
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !matches!(
        (a, b, c),
        (0, _, _)
            | (10, _, _)
            | (100, 64..=127, _)
            | (127, _, _)
            | (169, 254, _)
            | (172, 16..=31, _)
            | (192, 0, 0)
            | (192, 0, 2)
            | (192, 88, 99)
            | (192, 168, _)
            | (198, 18..=19, _)
            | (198, 51, 100)
            | (203, 0, 113)
            | (224..=255, _, _)
    )
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let ipv4_compatible = segments[..6].iter().all(|value| *value == 0);
    let nat64_well_known = segments[..6] == [0x0064, 0xff9b, 0, 0, 0, 0];
    let nat64_local_use = segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 1;
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && !ipv4_compatible
        && !nat64_well_known
        && !nat64_local_use
        && segments[0] & 0xfe00 != 0xfc00
        && segments[0] & 0xffc0 != 0xfe80
        && segments[0] & 0xffc0 != 0xfec0
        && !(segments[0] == 0x0100 && segments[1..].iter().all(|value| *value == 0))
        && !(segments[0] == 0x2001 && segments[1] == 0)
        && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
}

#[derive(Debug)]
pub struct NetworkServer<F> {
    fetcher: F,
    trusted_uids: BTreeSet<u32>,
}

impl<F: HttpFetcher> NetworkServer<F> {
    pub fn new(fetcher: F, trusted_uids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            fetcher,
            trusted_uids: trusted_uids.into_iter().collect(),
        }
    }

    pub fn serve(&self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-networkd: rejected connection: {error}");
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
                    &NetworkResponse::error(
                        0,
                        NetworkErrorCode::InvalidRequest,
                        "invalid network service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-networkd: invalid request: {error}");
                return Ok(());
            }
        };
        let response = if self.trusted_uids.contains(&uid) {
            self.dispatch(request)
        } else {
            NetworkResponse::error(
                request.request_id,
                NetworkErrorCode::Unauthorized,
                "peer UID is not authorized to use the network service",
            )
        };
        write_response(&mut stream, &response).map_err(protocol_io)
    }

    pub fn dispatch(&self, request: NetworkRequest) -> NetworkResponse {
        let request_id = request.request_id;
        match request.command {
            NetworkCommand::HttpGet { url } => match self.fetcher.get(&url) {
                Ok(result)
                    if (100..=599).contains(&result.status_code)
                        && result.body.len() <= MAX_NETWORK_BODY_BYTES =>
                {
                    NetworkResponse::success(request_id, result.status_code, &result.body)
                }
                Ok(result) if result.body.len() > MAX_NETWORK_BODY_BYTES => NetworkResponse::error(
                    request_id,
                    NetworkErrorCode::ResponseTooLarge,
                    "HTTPS response body exceeds 2048 bytes",
                ),
                Ok(_) => NetworkResponse::error(
                    request_id,
                    NetworkErrorCode::Internal,
                    "network service produced an invalid HTTP status",
                ),
                Err(error) => NetworkResponse::error(request_id, error.code, error.message),
            },
        }
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials and length reference writable objects of the sizes
    // passed to getsockopt, and the stream owns a valid Unix socket descriptor.
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

fn protocol_io(error: NetworkProtocolError) -> io::Error {
    match error {
        NetworkProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use cp0_network_protocol::NetworkOutcome;

    use super::*;

    #[derive(Debug)]
    struct MockFetcher {
        result: Result<HttpResult, FetchError>,
    }

    impl HttpFetcher for MockFetcher {
        fn get(&self, _url: &str) -> Result<HttpResult, FetchError> {
            self.result.clone()
        }
    }

    #[test]
    fn rejects_local_special_and_documentation_addresses() {
        for address in [
            "0.0.0.0",
            "10.1.2.3",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.31.255.255",
            "192.168.1.1",
            "198.18.0.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "::2",
            "64:ff9b::a00:1",
            "64:ff9b:1::1",
            "fec0::1",
            "2001::1",
            "2001:db8::1",
        ] {
            let parsed = address.parse().unwrap();
            assert!(!is_public_address(parsed), "accepted {address}");
        }
        for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            let parsed = address.parse().unwrap();
            assert!(is_public_address(parsed), "rejected {address}");
        }
    }

    #[test]
    fn config_is_https_only_bounded_and_ignores_environment_proxy() {
        let config = network_config();
        assert!(config.https_only());
        assert!(!config.http_status_as_error());
        assert_eq!(config.max_redirects(), MAX_REDIRECTS);
        assert!(config.max_redirects_will_error());
        assert!(config.proxy().is_none());
    }

    #[test]
    fn dispatches_success_and_sanitized_failure() {
        let success = NetworkServer::new(
            MockFetcher {
                result: Ok(HttpResult {
                    status_code: 200,
                    body: b"hello".to_vec(),
                }),
            },
            [0],
        )
        .dispatch(NetworkRequest::http_get(41, "https://example.com"));
        assert_eq!(success.request_id, 41);
        assert!(matches!(success.outcome, NetworkOutcome::Ok { .. }));

        let failure = NetworkServer::new(
            MockFetcher {
                result: Err(FetchError {
                    code: NetworkErrorCode::BlockedAddress,
                    message: "destination resolved to a non-public address",
                }),
            },
            [0],
        )
        .dispatch(NetworkRequest::http_get(42, "https://internal.invalid"));
        assert!(matches!(
            failure.outcome,
            NetworkOutcome::Error {
                code: NetworkErrorCode::BlockedAddress,
                ..
            }
        ));
    }

    #[test]
    fn fetcher_blocks_literal_ssrf_and_plain_http_before_connecting() {
        let fetcher = UreqFetcher::default();
        for url in [
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://localhost/",
            "https://printer/",
            "https://device.local/",
            "https://service.home.arpa/",
        ] {
            let error = fetcher.get(url).unwrap_err();
            assert_eq!(error.code, NetworkErrorCode::BlockedAddress);
        }
        let error = fetcher.get("http://example.com/").unwrap_err();
        assert_eq!(error.code, NetworkErrorCode::InvalidRequest);
    }

    #[test]
    #[ignore = "requires live public DNS and HTTPS"]
    fn completes_a_live_tls_validated_request() {
        let result = UreqFetcher::default().get("https://example.com/").unwrap();
        assert_eq!(result.status_code, 200);
        assert!(!result.body.is_empty());
        assert!(result.body.len() <= MAX_NETWORK_BODY_BYTES);
    }
}
