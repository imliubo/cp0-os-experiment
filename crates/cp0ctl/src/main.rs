use std::env;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

use cp0_appd::{
    APPD_PROTOCOL_VERSION, AppdCommand, AppdRequest, BROKER_PROTOCOL_VERSION, BrokerCommand,
    BrokerOutcome, BrokerRequest, DeviceMode, MAX_APP_LIST_PAGE, MAX_LOG_LINES, PermissionChoice,
    ResponseOutcome, read_broker_response, read_response, write_broker_request, write_request,
};
use cp0_manifest::Permission;

mod package;
mod project;
mod remote;
mod store;
mod store_client;
mod store_submission;
mod store_submit;

const APPD_SOCKET: &str = "/run/cardputerzero-appd/control.sock";
const BROKER_SOCKET: &str = "/run/cardputerzero-broker/runtime.sock";
const REQUEST_ID: u64 = 1;
const TIMEOUT: Duration = Duration::from_secs(5);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(60);

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
        [command, rest @ ..] if command == "run" => project::run_project(rest),
        [key, command, secret, public] if key == "key" && command == "generate" => {
            package::generate_key(secret, public)
        }
        [command, developer_public, ssh_public, host_label, flag, device]
            if command == "pair" && flag == "--device" =>
        {
            remote::pair(developer_public, ssh_public, host_label, device)
        }
        [command] if command == "dev-session" => remote::session(),
        [command, project] if command == "package" => {
            package::default_package_path(project).and_then(|output| {
                package::package_project(project, &output.to_string_lossy())
            })
        }
        [command, project, output] if command == "package" => {
            package::package_project(project, output)
        }
        [command, role, input, output, secret] if command == "sign" => {
            package::sign_package(role, input, output, secret)
        }
        [command, input] if command == "verify" => package::verify_package(input, None),
        [command, input, store_public] if command == "verify" => {
            package::verify_package(input, Some(store_public))
        }
        [store_command, publish, submissions, reviews, output, base_url, sequence, published, expires, secret]
            if store_command == "store" && publish == "publish" =>
        {
            store::publish(store::PublishOptions {
                submissions,
                reviews,
                output,
                base_url,
                sequence,
                published,
                expires,
                secret,
            })
        }
        [store_command, command, package, listing]
            if store_command == "store" && command == "validate" =>
        {
            store_submission::validate_submission(package, listing)
        }
        [store_command, command, package, listing]
            if store_command == "store" && command == "submit" =>
        {
            store_submit::submit(package, listing)
        }
        [store_command, command] if store_command == "store" && command == "list" => {
            store_client::send(cp0_store_protocol::StoreCommand::List)
        }
        [store_command, command, category]
            if store_command == "store" && command == "browse" =>
        {
            parse_store_browse(category, None, None).and_then(store_client::send)
        }
        [store_command, command, category, offset, limit]
            if store_command == "store" && command == "browse" =>
        {
            parse_store_browse(category, Some(offset), Some(limit)).and_then(store_client::send)
        }
        [store_command, command, query]
            if store_command == "store" && command == "search" =>
        {
            parse_store_search(query, None, None).and_then(store_client::send)
        }
        [store_command, command, query, offset, limit]
            if store_command == "store" && command == "search" =>
        {
            parse_store_search(query, Some(offset), Some(limit)).and_then(store_client::send)
        }
        [store_command, command] if store_command == "store" && command == "refresh" => {
            store_client::send(cp0_store_protocol::StoreCommand::Refresh)
        }
        [store_command, command] if store_command == "store" && command == "metrics" => {
            store_client::send(cp0_store_protocol::StoreCommand::GetMetrics)
        }
        [store_command, command, app_id, approval]
            if store_command == "store"
                && command == "install"
                && approval == "--approve-permissions" =>
        {
            if cp0_manifest::is_valid_app_id(app_id) {
                store_client::install(vec![app_id.clone()])
            } else {
                Err("store install application ID is invalid".into())
            }
        }
        [store_command, command, app_id]
            if store_command == "store"
                && matches!(command.as_str(), "pause" | "resume" | "cancel") =>
        {
            parse_store_control(command, app_id).and_then(store_client::send)
        }
        [store_command, command, approval, app_ids @ ..]
            if store_command == "store"
                && command == "install-batch"
                && approval == "--approve-permissions" =>
        {
            parse_store_install_batch(app_ids).and_then(|command| {
                let cp0_store_protocol::StoreCommand::InstallBatch { app_ids, .. } = command else {
                    unreachable!("install batch parser returned another command")
                };
                store_client::install(app_ids)
            })
        }
        [command, input] if command == "install" => install_package(input),
        [command, input, flag, device] if command == "install" && flag == "--device" => {
            remote::install(input, device)
        }
        [command, app_id] if command == "logs" => send_app_command(AppdCommand::Logs {
            app_id: app_id.clone(),
            limit: 50,
        }),
        [command, app_id, limit] if command == "logs" => limit
            .parse::<u16>()
            .ok()
            .filter(|limit| (1..=MAX_LOG_LINES).contains(limit))
            .ok_or_else(|| "log line limit must be between 1 and 100".to_owned())
            .and_then(|limit| {
                send_app_command(AppdCommand::Logs {
                    app_id: app_id.clone(),
                    limit,
                })
            }),
        [command, app_id, flag, device] if command == "logs" && flag == "--device" => {
            remote::logs(device, app_id, 50)
        }
        [command, app_id, limit, flag, device]
            if command == "logs" && flag == "--device" =>
        {
            limit
                .parse::<u16>()
                .ok()
                .filter(|limit| (1..=MAX_LOG_LINES).contains(limit))
                .ok_or_else(|| "log line limit must be between 1 and 100".to_owned())
                .and_then(|limit| remote::logs(device, app_id, limit))
        }
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
        [app, command, app_id, flag, device]
            if app == "app" && command == "start" && flag == "--device" =>
        {
            remote::start(device, app_id)
        }
        [app, command, app_id] if app == "app" && command == "stop" => {
            send_app_command(AppdCommand::Stop {
                app_id: app_id.clone(),
            })
        }
        [app, command, app_id, flag, device]
            if app == "app" && command == "stop" && flag == "--device" =>
        {
            remote::stop(device, app_id)
        }
        [app, command, app_id, flag, device]
            if app == "app" && command == "uninstall" && flag == "--device" =>
        {
            remote::uninstall(device, app_id)
        }
        [app, command, app_id] if app == "app" && command == "rollback" => {
            send_app_command(AppdCommand::Rollback {
                app_id: app_id.clone(),
            })
        }
        [device, command] if device == "device" && command == "status" => {
            send_app_command(AppdCommand::GetDeviceSettings)
        }
        [device, command, flag, target]
            if device == "device" && command == "remote-status" && flag == "--device" =>
        {
            remote::status(target)
        }
        [device, mode, enabled] if device == "device" => parse_device_mode(mode).and_then(|mode| {
            parse_enabled(enabled).and_then(|enabled| {
                send_app_command(AppdCommand::SetDeviceMode { mode, enabled })
            })
        }),
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
            "usage: cp0ctl new <directory> <app-id> <display-name> | build <directory> | run <directory> [--duration ms] [--permissions allow|deny] [--keys comma-list] [--media-actions comma-list] [--output frame.ppm] [--profile profile.json] | package <directory> [output.capp] | key generate <secret-key> <public-key> | sign <developer|store> <input.capp> <output.capp> <secret-key> | verify <package.capp> [store-public-key] | pair <developer-public-key> <ssh-public-key> <host-label> --device owner@host | store validate <developer-signed.capp> <store/listing.json> | store submit <developer-signed.capp> <store/listing.json> | store publish <submissions-dir> <reviews-dir> <output-dir> <base-url> <sequence> <published-unix> <expires-unix> <store-secret-key> | store list | store browse <all|category> [offset limit] | store search <query> [offset limit] | store refresh | store metrics | store install <app-id> --approve-permissions | store install-batch --approve-permissions <app-id>... | store pause|resume|cancel <app-id> | install <package.capp> [--device user@host] | logs <app-id> [lines] [--device user@host] | manifest validate <app.json> | app ping | app list [offset limit] | app start <app-id> [--device user@host] | app stop <app-id> [--device user@host] | app uninstall <app-id> --device user@host | app rollback <app-id> | device status | device remote-status --device user@host | device <developer|recovery> <on|off> | permission pending | permission resolve <prompt-id> <once|always|deny> | permission reset <app-id> <capability> | notification take | broker notify <title> <body>"
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

fn parse_store_control(
    command: &str,
    app_id: &str,
) -> Result<cp0_store_protocol::StoreCommand, String> {
    if !cp0_manifest::is_valid_app_id(app_id) {
        return Err("store control application ID is invalid".into());
    }
    let action = match command {
        "pause" => cp0_store_protocol::StoreControlAction::Pause,
        "resume" => cp0_store_protocol::StoreControlAction::Resume,
        "cancel" => cp0_store_protocol::StoreControlAction::Cancel,
        _ => return Err("store control action is invalid".into()),
    };
    Ok(cp0_store_protocol::StoreCommand::Control {
        app_id: app_id.into(),
        action,
    })
}

fn parse_store_install_batch(
    app_ids: &[String],
) -> Result<cp0_store_protocol::StoreCommand, String> {
    if app_ids.is_empty() || app_ids.len() > cp0_store_protocol::MAX_INSTALL_BATCH_APPS {
        return Err("store install batch count is outside limits".into());
    }
    let mut app_ids = app_ids.to_vec();
    if app_ids
        .iter()
        .any(|app_id| !cp0_manifest::is_valid_app_id(app_id))
    {
        return Err("store install batch application ID is invalid".into());
    }
    app_ids.sort();
    if app_ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("store install batch application IDs are duplicated".into());
    }
    Ok(cp0_store_protocol::StoreCommand::InstallBatch {
        app_ids,
        authorization_id: 1,
    })
}

fn parse_store_search(
    query: &str,
    offset: Option<&str>,
    limit: Option<&str>,
) -> Result<cp0_store_protocol::StoreCommand, String> {
    cp0_store_protocol::validate_search_query(query).map_err(|_| {
        "store search query must be 1-32 characters and at most 96 bytes".to_owned()
    })?;
    let (offset, limit) = match (offset, limit) {
        (None, None) => (0, cp0_store_protocol::MAX_SEARCH_PAGE_APPS),
        (Some(offset), Some(limit)) => {
            let offset = offset
                .parse::<u16>()
                .ok()
                .filter(|offset| {
                    usize::from(*offset) <= cp0_store_protocol::MAX_SHARDED_CATALOG_APPS
                })
                .ok_or_else(|| "store search offset must be between 0 and 1024".to_owned())?;
            let limit = limit
                .parse::<u8>()
                .ok()
                .filter(|limit| (1..=cp0_store_protocol::MAX_SEARCH_PAGE_APPS).contains(limit))
                .ok_or_else(|| "store search limit must be between 1 and 8".to_owned())?;
            (offset, limit)
        }
        _ => return Err("store search pagination requires both offset and limit".into()),
    };
    Ok(cp0_store_protocol::StoreCommand::Search {
        query: query.into(),
        offset,
        limit,
    })
}

fn parse_store_browse(
    category: &str,
    offset: Option<&str>,
    limit: Option<&str>,
) -> Result<cp0_store_protocol::StoreCommand, String> {
    let category = match category {
        "all" => None,
        "developer-tools" => Some(cp0_store_metadata::StoreCategory::DeveloperTools),
        "education" => Some(cp0_store_metadata::StoreCategory::Education),
        "entertainment" => Some(cp0_store_metadata::StoreCategory::Entertainment),
        "games" => Some(cp0_store_metadata::StoreCategory::Games),
        "hardware" => Some(cp0_store_metadata::StoreCategory::Hardware),
        "media" => Some(cp0_store_metadata::StoreCategory::Media),
        "productivity" => Some(cp0_store_metadata::StoreCategory::Productivity),
        "utilities" => Some(cp0_store_metadata::StoreCategory::Utilities),
        _ => return Err("store browse category is invalid".into()),
    };
    let (offset, limit) = match (offset, limit) {
        (None, None) => (0, cp0_store_protocol::MAX_SEARCH_PAGE_APPS),
        (Some(offset), Some(limit)) => {
            let offset = offset
                .parse::<u16>()
                .ok()
                .filter(|offset| {
                    usize::from(*offset) <= cp0_store_protocol::MAX_SHARDED_CATALOG_APPS
                })
                .ok_or_else(|| "store browse offset must be between 0 and 1024".to_owned())?;
            let limit = limit
                .parse::<u8>()
                .ok()
                .filter(|limit| (1..=cp0_store_protocol::MAX_SEARCH_PAGE_APPS).contains(limit))
                .ok_or_else(|| "store browse limit must be between 1 and 8".to_owned())?;
            (offset, limit)
        }
        _ => return Err("store browse pagination requires both offset and limit".into()),
    };
    Ok(cp0_store_protocol::StoreCommand::Browse {
        category,
        offset,
        limit,
    })
}

fn install_package(path: &str) -> Result<(), String> {
    let (name, staged) = package::stage_for_install(path)?;
    let result =
        send_app_command_with_timeout(AppdCommand::Install { package_name: name }, INSTALL_TIMEOUT);
    let cleanup = std::fs::remove_file(&staged)
        .map_err(|error| format!("cannot remove staged package {}: {error}", staged.display()));
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
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
    Permission::ALL
        .into_iter()
        .find(|permission| permission.as_str() == value)
        .ok_or_else(|| "unknown CardputerZero capability".into())
}

fn parse_device_mode(value: &str) -> Result<DeviceMode, String> {
    match value {
        "developer" => Ok(DeviceMode::Developer),
        "recovery" => Ok(DeviceMode::Recovery),
        _ => Err("device mode must be developer or recovery".into()),
    }
}

fn parse_enabled(value: &str) -> Result<bool, String> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err("device mode state must be on or off".into()),
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
    send_app_command_with_timeout(command, TIMEOUT)
}

fn send_app_command_with_timeout(command: AppdCommand, timeout: Duration) -> Result<(), String> {
    let socket = env::var("CP0_APPD_SOCKET").unwrap_or_else(|_| APPD_SOCKET.into());
    let mut stream = UnixStream::connect(&socket)
        .map_err(|error| format!("cannot connect to appd at {socket}: {error}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("cannot set appd timeout: {error}"))?;
    stream
        .set_write_timeout(Some(timeout))
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
        | BrokerOutcome::CameraCaptured { .. }
        | BrokerOutcome::GpioValue { .. }
        | BrokerOutcome::GpioWritten { .. }
        | BrokerOutcome::LoraSent { .. }
        | BrokerOutcome::LoraPacket { .. }
        | BrokerOutcome::LoraNoPacket
        | BrokerOutcome::StorageStored { .. }
        | BrokerOutcome::StorageValue { .. }
        | BrokerOutcome::StorageNotFound
        | BrokerOutcome::StorageDeleted { .. }
        | BrokerOutcome::IntentAccepted { .. }
        | BrokerOutcome::IntentMessage { .. }
        | BrokerOutcome::IntentEmpty
        | BrokerOutcome::MediaSessionUpdated { .. }
        | BrokerOutcome::MediaAction { .. }
        | BrokerOutcome::MediaActionEmpty => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_permission_parser_uses_manifest_vocabulary() {
        for permission in Permission::ALL {
            assert_eq!(parse_permission(permission.as_str()), Ok(permission));
        }
        assert!(parse_permission("clipboard.read").is_err());
    }

    #[test]
    fn cli_device_mode_parser_is_closed() {
        assert_eq!(parse_device_mode("developer"), Ok(DeviceMode::Developer));
        assert_eq!(parse_device_mode("recovery"), Ok(DeviceMode::Recovery));
        assert!(parse_device_mode("factory").is_err());
        assert_eq!(parse_enabled("on"), Ok(true));
        assert_eq!(parse_enabled("off"), Ok(false));
        assert!(parse_enabled("yes").is_err());
    }

    #[test]
    fn cli_store_search_parser_is_bounded() {
        assert_eq!(
            parse_store_search("Notes", None, None),
            Ok(cp0_store_protocol::StoreCommand::Search {
                query: "Notes".into(),
                offset: 0,
                limit: cp0_store_protocol::MAX_SEARCH_PAGE_APPS,
            })
        );
        assert_eq!(
            parse_store_search("Notes", Some("8"), Some("4")),
            Ok(cp0_store_protocol::StoreCommand::Search {
                query: "Notes".into(),
                offset: 8,
                limit: 4,
            })
        );
        assert!(parse_store_search("", None, None).is_err());
        assert!(parse_store_search("Notes", Some("65"), Some("1")).is_ok());
        assert!(parse_store_search("Notes", Some("1025"), Some("1")).is_err());
        assert!(parse_store_search("Notes", Some("0"), Some("9")).is_err());
        assert!(parse_store_search("Notes", Some("0"), None).is_err());
    }

    #[test]
    fn cli_store_browse_parser_is_category_bound_and_paginated() {
        assert_eq!(
            parse_store_browse("utilities", None, None),
            Ok(cp0_store_protocol::StoreCommand::Browse {
                category: Some(cp0_store_metadata::StoreCategory::Utilities),
                offset: 0,
                limit: cp0_store_protocol::MAX_SEARCH_PAGE_APPS,
            })
        );
        assert_eq!(
            parse_store_browse("all", Some("1024"), Some("1")),
            Ok(cp0_store_protocol::StoreCommand::Browse {
                category: None,
                offset: 1024,
                limit: 1,
            })
        );
        assert!(parse_store_browse("unknown", None, None).is_err());
        assert!(parse_store_browse("games", Some("1025"), Some("1")).is_err());
        assert!(parse_store_browse("games", Some("0"), Some("9")).is_err());
        assert!(parse_store_browse("games", Some("0"), None).is_err());
    }

    #[test]
    fn cli_store_control_parser_is_closed() {
        for (name, action) in [
            ("pause", cp0_store_protocol::StoreControlAction::Pause),
            ("resume", cp0_store_protocol::StoreControlAction::Resume),
            ("cancel", cp0_store_protocol::StoreControlAction::Cancel),
        ] {
            assert_eq!(
                parse_store_control(name, "dev.cardputerzero.example"),
                Ok(cp0_store_protocol::StoreCommand::Control {
                    app_id: "dev.cardputerzero.example".into(),
                    action,
                })
            );
        }
        assert!(parse_store_control("stop", "dev.cardputerzero.example").is_err());
        assert!(parse_store_control("pause", "invalid").is_err());
    }

    #[test]
    fn cli_store_install_batch_parser_is_bounded_and_canonical() {
        let app_ids = vec![
            "dev.cardputerzero.beta".into(),
            "dev.cardputerzero.alpha".into(),
        ];
        assert_eq!(
            parse_store_install_batch(&app_ids),
            Ok(cp0_store_protocol::StoreCommand::InstallBatch {
                app_ids: vec![
                    "dev.cardputerzero.alpha".into(),
                    "dev.cardputerzero.beta".into(),
                ],
                authorization_id: 1,
            })
        );
        assert!(parse_store_install_batch(&[]).is_err());
        assert!(parse_store_install_batch(&[app_ids[0].clone(), app_ids[0].clone()]).is_err());
        assert!(parse_store_install_batch(&["invalid".into()]).is_err());
        assert!(
            parse_store_install_batch(
                &(0..=cp0_store_protocol::MAX_INSTALL_BATCH_APPS)
                    .map(|index| format!("dev.cardputerzero.batch{index}"))
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
    }
}
