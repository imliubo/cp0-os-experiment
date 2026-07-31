use std::env;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_store_protocol::{
    STORE_PROTOCOL_VERSION, StoreCommand, StoreOutcome, StoreRequest, StoreResponse,
    StoreResponseData, read_response, write_request,
};

const STORE_SOCKET: &str = "/run/cardputerzero-store/control.sock";
const REQUEST_ID: u64 = 1;
const TIMEOUT: Duration = Duration::from_secs(5);

pub fn send(command: StoreCommand) -> Result<(), String> {
    let socket = env::var_os("CP0_STORE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(STORE_SOCKET));
    let response = exchange(&socket, command, TIMEOUT)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response)
            .map_err(|error| format!("cannot encode Store response: {error}"))?
    );
    Ok(())
}

fn exchange(
    socket: &Path,
    command: StoreCommand,
    timeout: Duration,
) -> Result<StoreResponse, String> {
    let stream = UnixStream::connect(socket)
        .map_err(|error| format!("cannot connect to Store at {}: {error}", socket.display()))?;
    exchange_stream(stream, command, timeout)
}

fn exchange_stream(
    mut stream: UnixStream,
    command: StoreCommand,
    timeout: Duration,
) -> Result<StoreResponse, String> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("cannot set Store timeout: {error}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| format!("cannot set Store timeout: {error}"))?;
    let request = StoreRequest {
        protocol_version: STORE_PROTOCOL_VERSION,
        request_id: REQUEST_ID,
        command,
    };
    write_request(&mut stream, &request)
        .map_err(|error| format!("cannot send Store request: {error}"))?;
    let mut reader = BufReader::new(stream);
    let response = read_response(&mut reader)
        .map_err(|error| format!("cannot read Store response: {error}"))?
        .ok_or_else(|| "Store closed the connection without a response".to_owned())?;
    if response.request_id != REQUEST_ID {
        return Err("Store response request ID does not match".into());
    }
    match &response.outcome {
        StoreOutcome::Ok { data } => {
            if !response_matches(&request.command, data) {
                return Err("Store response does not match the request".into());
            }
            Ok(response)
        }
        StoreOutcome::Error { code, message } => Err(format!("Store returned {code:?}: {message}")),
    }
}

fn response_matches(command: &StoreCommand, data: &StoreResponseData) -> bool {
    match (command, data) {
        (StoreCommand::List, StoreResponseData::Catalog { .. })
        | (StoreCommand::Refresh, StoreResponseData::RefreshAccepted) => true,
        (
            StoreCommand::Search {
                query: requested_query,
                offset: requested_offset,
                limit: requested_limit,
            },
            StoreResponseData::SearchResults {
                query,
                offset,
                limit,
                ..
            },
        ) => requested_query == query && requested_offset == offset && requested_limit == limit,
        (
            StoreCommand::Install { app_id: requested },
            StoreResponseData::InstallAccepted { app_id, .. },
        ) => requested == app_id,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use cp0_store_protocol::{
        StoreErrorCode, StoreResponse, StoreResponseData, read_request, write_response,
    };

    use super::*;

    fn serve_once(
        expected: StoreCommand,
        response: StoreResponse,
    ) -> (UnixStream, thread::JoinHandle<()>) {
        let (client, mut server) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let request = read_request(&mut reader).unwrap().unwrap();
            assert_eq!(request.command, expected);
            write_response(&mut server, &response).unwrap();
        });
        (client, worker)
    }

    #[test]
    fn exchanges_only_the_response_bound_to_the_command() {
        let (stream, worker) = serve_once(
            StoreCommand::Refresh,
            StoreResponse::success(REQUEST_ID, StoreResponseData::RefreshAccepted),
        );
        let response = exchange_stream(stream, StoreCommand::Refresh, TIMEOUT).unwrap();
        assert!(matches!(
            response.outcome,
            StoreOutcome::Ok {
                data: StoreResponseData::RefreshAccepted
            }
        ));
        worker.join().unwrap();
    }

    #[test]
    fn rejects_mismatched_request_and_install_identity() {
        let (stream, worker) = serve_once(
            StoreCommand::List,
            StoreResponse::success(9, StoreResponseData::RefreshAccepted),
        );
        assert!(exchange_stream(stream, StoreCommand::List, TIMEOUT).is_err());
        worker.join().unwrap();

        let requested = "dev.cardputerzero.requested".to_owned();
        let (stream, worker) = serve_once(
            StoreCommand::Install {
                app_id: requested.clone(),
            },
            StoreResponse::success(
                REQUEST_ID,
                StoreResponseData::InstallAccepted {
                    app_id: "dev.cardputerzero.substituted".into(),
                    version: "1.0.0".into(),
                },
            ),
        );
        assert!(
            exchange_stream(stream, StoreCommand::Install { app_id: requested }, TIMEOUT).is_err()
        );
        worker.join().unwrap();
    }

    #[test]
    fn rejects_search_response_for_a_different_page() {
        let requested = StoreCommand::Search {
            query: "notes".into(),
            offset: 0,
            limit: 1,
        };
        let (stream, worker) = serve_once(
            requested.clone(),
            StoreResponse::success(
                REQUEST_ID,
                StoreResponseData::SearchResults {
                    query: "notes".into(),
                    offset: 1,
                    limit: 1,
                    total: 0,
                    next_offset: None,
                    sequence: 3,
                    expires_unix_seconds: 1_900_000_000,
                    stale: false,
                    apps: Vec::new(),
                },
            ),
        );
        assert!(exchange_stream(stream, requested, TIMEOUT).is_err());
        worker.join().unwrap();
    }

    #[test]
    fn maps_strict_service_errors() {
        let (stream, worker) = serve_once(
            StoreCommand::List,
            StoreResponse::error(
                REQUEST_ID,
                StoreErrorCode::Unconfigured,
                "Store endpoint is not configured",
            ),
        );
        let error = exchange_stream(stream, StoreCommand::List, TIMEOUT).unwrap_err();
        assert!(error.contains("Unconfigured"));
        worker.join().unwrap();
    }
}
