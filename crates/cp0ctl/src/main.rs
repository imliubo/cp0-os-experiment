use std::env;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

use cp0_appd::{
    APPD_PROTOCOL_VERSION, AppdCommand, AppdRequest, BROKER_PROTOCOL_VERSION, BrokerCommand,
    BrokerOutcome, BrokerRequest, MAX_APP_LIST_PAGE, PermissionChoice, ResponseOutcome,
    read_broker_response, read_response, write_broker_request, write_request,
};
use cp0_manifest::Permission;

mod project;

const APPD_SOCKET: &str = "/run/cardputerzero-appd/control.sock";
const BROKER_SOCKET: &str = "/run/cardputerzero-broker/runtime.sock";
const REQUEST_ID: u64 = 1;
const TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();

    let result = match arguments.as_slice() {
        [manifest, validate, path] if manifest == "manifest" && validate == "validate" => {
            validate_manifest(path)
        }
        [command, path, app_id, name] if command == "new" => {
            project::new_project(path, app_id, name)
        }
        [command, path] if command == "build" => project::build_project(path).map(|_| ()),
        [app, command] if app == "app" && command == "ping" => send_app_command(AppdCommand::Ping),
        [app, command] if app == "app" && command == "list" => send_app_command(
            AppdCommand::List {
                offset: 0,
                limit: MAX_APP_LIST_PAGE,
            },
        ),
        [app, command, offset, limit] if app == "app" && command == "list" => {
            let offset = offset
                .parse::<u16>()
                .map_err(|_| "list offset must be an unsigned 16-bit integer".to_owned());
            let limit = limit
                .parse::<u8>()
                .map_err(|_| "list limit must be an integer between 1 and 8".to_owned());
            offset.and_then(|offset| {
                limit.and_then(|limit| {
                    send_app_command(AppdCommand::List { offset, limit })
                })
            })
        }
        [app, command, app_id] if app == "app" && command == "start" => {
            send_app_command(AppdCommand::Start {
                app_id: app_id.clone(),
            })
        }
        [app, command, app_id] if app == "app" && command == "stop" => {
            send_app_command(AppdCommand::Stop {
                app_id: app_id.clone(),
            })
        }
        [permission, command] if permission == "permission" && command == "pending" => {
            send_app_command(AppdCommand::GetPermissionPrompt)
        }
        [permission, command, prompt_id, choice]
            if permission == "permission" && command == "resolve" =>
        {
            parse_prompt_id(prompt_id).and_then(|prompt_id| {
                parse_permission_choice(choice).and_then(|choice| {
                    send_app_command(AppdCommand::ResolvePermission { prompt_id, choice })
                })
            })
        }
        [permission, command, app_id, capability]
            if permission == "permission" && command == "reset" =>
        {
            parse_permission(capability).and_then(|permission| {
                send_app_command(AppdCommand::ResetPermission {
                    app_id: app_id.clone(),
                    permission,
                })
            })
        }
        [notification, command] if notification == "notification" && command == "take" => {
            send_app_command(AppdCommand::TakeNotification)
        }
        [broker, command, title, body] if broker == "broker" && command == "notify" => {
            send_broker_command(BrokerCommand::PostNotification {
                title: title.clone(),
                body: body.clone(),
            })
        }
        _ => Err(
            "usage: cp0ctl new <directory> <app-id> <display-name> | build <directory> | manifest validate <app.json> | app ping | app list [offset limit] | app start <app-id> | app stop <app-id> | permission pending | permission resolve <prompt-id> <once|always|deny> | permission reset <app-id> <capability> | notification take | broker notify <title> <body>"
                .into(),
        ),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cp0ctl: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_prompt_id(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|prompt_id| *prompt_id != 0)
        .ok_or_else(|| "permission prompt ID must be a non-zero integer".into())
}

fn parse_permission_choice(value: &str) -> Result<PermissionChoice, String> {
    match value {
        "once" => Ok(PermissionChoice::AllowOnce),
        "always" => Ok(PermissionChoice::AllowAlways),
        "deny" => Ok(PermissionChoice::Deny),
        _ => Err("permission choice must be once, always or deny".into()),
    }
}

fn parse_permission(value: &str) -> Result<Permission, String> {
    match value {
        "network.client" => Ok(Permission::NetworkClient),
        "documents.open" => Ok(Permission::DocumentsOpen),
        "audio.playback" => Ok(Permission::AudioPlayback),
        "audio.capture" => Ok(Permission::AudioCapture),
        "camera.capture" => Ok(Permission::CameraCapture),
        "radio.lora" => Ok(Permission::RadioLora),
        "hardware.gpio" => Ok(Permission::HardwareGpio),
        "clipboard.read" => Ok(Permission::ClipboardRead),
        "notifications.post" => Ok(Permission::NotificationsPost),
        _ => Err("unknown CardputerZero capability".into()),
    }
}

fn validate_manifest(path: &str) -> Result<(), String> {
    match cp0_manifest::load_and_validate(path) {
        Ok(app) => {
            println!(
                "valid CardputerZero app manifest: {} {}",
                app.id, app.version
            );
            Ok(())
        }
        Err(error) => Err(format!("manifest validation failed:\n{error}")),
    }
}

fn send_app_command(command: AppdCommand) -> Result<(), String> {
    let socket = env::var("CP0_APPD_SOCKET").unwrap_or_else(|_| APPD_SOCKET.into());
    let mut stream = UnixStream::connect(&socket)
        .map_err(|error| format!("cannot connect to appd at {socket}: {error}"))?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|error| format!("cannot set appd timeout: {error}"))?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|error| format!("cannot set appd timeout: {error}"))?;
    let request = AppdRequest {
        protocol_version: APPD_PROTOCOL_VERSION,
        request_id: REQUEST_ID,
        command,
    };
    write_request(&mut stream, &request)
        .map_err(|error| format!("cannot send appd request: {error}"))?;
    let mut reader = BufReader::new(stream);
    let response = read_response(&mut reader)
        .map_err(|error| format!("cannot read appd response: {error}"))?
        .ok_or_else(|| "appd closed the connection without a response".to_owned())?;
    if response.request_id != REQUEST_ID {
        return Err("appd response request ID does not match".into());
    }
    match &response.outcome {
        ResponseOutcome::Ok { .. } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&response)
                    .map_err(|error| format!("cannot encode appd response: {error}"))?
            );
            Ok(())
        }
        ResponseOutcome::Error { code, message } => {
            Err(format!("appd returned {code:?}: {message}"))
        }
    }
}

fn send_broker_command(command: BrokerCommand) -> Result<(), String> {
    let socket = env::var("CP0_BROKER_SOCKET").unwrap_or_else(|_| BROKER_SOCKET.into());
    let mut stream = UnixStream::connect(&socket)
        .map_err(|error| format!("cannot connect to capability broker at {socket}: {error}"))?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|error| format!("cannot set broker timeout: {error}"))?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|error| format!("cannot set broker timeout: {error}"))?;
    let request = BrokerRequest {
        protocol_version: BROKER_PROTOCOL_VERSION,
        request_id: REQUEST_ID,
        command,
    };
    write_broker_request(&mut stream, &request)
        .map_err(|error| format!("cannot send broker request: {error}"))?;
    let mut reader = BufReader::new(stream);
    let response = read_broker_response(&mut reader)
        .map_err(|error| format!("cannot read broker response: {error}"))?
        .ok_or_else(|| "broker closed the connection without a response".to_owned())?;
    if response.request_id != REQUEST_ID {
        return Err("broker response request ID does not match".into());
    }
    match &response.outcome {
        BrokerOutcome::Ok { .. }
        | BrokerOutcome::HttpResponse { .. }
        | BrokerOutcome::PermissionPending { .. }
        | BrokerOutcome::DocumentSelectionPending { .. }
        | BrokerOutcome::DocumentOpened { .. }
        | BrokerOutcome::AudioPlayed { .. }
        | BrokerOutcome::AudioCaptured { .. }
        | BrokerOutcome::CameraCaptured { .. } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&response)
                    .map_err(|error| format!("cannot encode broker response: {error}"))?
            );
            Ok(())
        }
        BrokerOutcome::Error { code, message } => {
            Err(format!("broker returned {code:?}: {message}"))
        }
    }
}
