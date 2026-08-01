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

pub fn install(app_ids: Vec<String>) -> Result<(), String> {
    let socket = env::var_os("CP0_STORE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(STORE_SOCKET));
    let catalog = exchange(&socket, StoreCommand::List, TIMEOUT)?;
    let (sequence, expected_apps) = match catalog.outcome {
        StoreOutcome::Ok {
            data: StoreResponseData::Catalog { sequence, apps, .. },
        } => {
            let expected = app_ids
                .iter()
                .map(|app_id| {
                    apps.iter()
                        .find(|app| &app.app_id == app_id)
                        .map(|app| {
                            (
                                app.app_id.clone(),
                                app.version.clone(),
                                app.permissions.clone(),
                            )
                        })
                        .ok_or_else(|| format!("Store application {app_id} was not found"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            (sequence, expected)
        }
        _ => return Err("Store list response did not contain a catalog".into()),
    };
    let preflight = exchange(
        &socket,
        StoreCommand::PreflightInstall {
            app_ids: app_ids.clone(),
            catalog_sequence: sequence,
        },
        TIMEOUT,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&preflight)
            .map_err(|error| format!("cannot encode Store preflight: {error}"))?
    );
    let authorization_id = match preflight.outcome {
        StoreOutcome::Ok {
            data:
                StoreResponseData::InstallPreflight {
                    authorization_id,
                    apps,
                    ..
                },
        } if apps.len() == expected_apps.len()
            && apps.iter().zip(&expected_apps).all(
                |(preflight, (app_id, version, permissions))| {
                    &preflight.app_id == app_id
                        && &preflight.version == version
                        && &preflight.permissions == permissions
                },
            ) =>
        {
            authorization_id
        }
        _ => return Err("Store response did not contain an install preflight".into()),
    };
    let command = if app_ids.len() == 1 {
        StoreCommand::Install {
            app_id: app_ids[0].clone(),
            authorization_id,
        }
    } else {
        StoreCommand::InstallBatch {
            app_ids,
            authorization_id,
        }
    };
    send_with_socket(&socket, command)
}

fn send_with_socket(socket: &Path, command: StoreCommand) -> Result<(), String> {
    let response = exchange(socket, command, TIMEOUT)?;
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
            StoreCommand::Install {
                app_id: requested, ..
            },
            StoreResponseData::InstallAccepted { app_id, .. },
        ) => requested == app_id,
        (
            StoreCommand::PreflightInstall {
                app_ids,
                catalog_sequence,
            },
            StoreResponseData::InstallPreflight {
                apps,
                catalog_sequence: response_sequence,
                ..
            },
        ) => {
            catalog_sequence == response_sequence
                && app_ids.len() == apps.len()
                && app_ids
                    .iter()
                    .zip(apps)
                    .all(|(requested, accepted)| requested == &accepted.app_id)
        }
        (
            StoreCommand::Control {
                app_id: requested_app,
                action: requested_action,
            },
            StoreResponseData::OperationAccepted { app_id, action, .. },
        ) => requested_app == app_id && requested_action == action,
        (
            StoreCommand::InstallBatch { app_ids, .. },
            StoreResponseData::InstallBatchAccepted { apps },
        ) => {
            app_ids.len() == apps.len()
                && app_ids
                    .iter()
                    .zip(apps)
                    .all(|(requested, accepted)| requested == &accepted.app_id)
        }
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
                authorization_id: 7,
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
            exchange_stream(
                stream,
                StoreCommand::Install {
                    app_id: requested,
                    authorization_id: 7,
                },
                TIMEOUT,
            )
            .is_err()
        );
        worker.join().unwrap();

        let requested = StoreCommand::InstallBatch {
            app_ids: vec![
                "dev.cardputerzero.alpha".into(),
                "dev.cardputerzero.beta".into(),
            ],
            authorization_id: 8,
        };
        let (stream, worker) = serve_once(
            requested.clone(),
            StoreResponse::success(
                REQUEST_ID,
                StoreResponseData::InstallBatchAccepted {
                    apps: vec![cp0_store_protocol::StoreInstallAccepted {
                        app_id: "dev.cardputerzero.alpha".into(),
                        version: "1.0.0".into(),
                    }],
                },
            ),
        );
        assert!(exchange_stream(stream, requested, TIMEOUT).is_err());
        worker.join().unwrap();

        let requested = StoreCommand::Control {
            app_id: "dev.cardputerzero.requested".into(),
            action: cp0_store_protocol::StoreControlAction::Pause,
        };
        let (stream, worker) = serve_once(
            requested.clone(),
            StoreResponse::success(
                REQUEST_ID,
                StoreResponseData::OperationAccepted {
                    app_id: "dev.cardputerzero.requested".into(),
                    version: "1.0.0".into(),
                    action: cp0_store_protocol::StoreControlAction::Cancel,
                },
            ),
        );
        assert!(exchange_stream(stream, requested, TIMEOUT).is_err());
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
