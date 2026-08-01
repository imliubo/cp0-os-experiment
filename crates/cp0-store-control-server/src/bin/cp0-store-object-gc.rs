use std::env;
use std::error::Error;

use cp0_store_control_server::{
    DEFAULT_OBJECT_GC_MINIMUM_AGE_SECONDS, ObjectGcMode, collect_content_objects, connect,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url =
        env::var("CP0_STORE_DATABASE_URL").map_err(|_| "CP0_STORE_DATABASE_URL is required")?;
    let object_root =
        env::var("CP0_STORE_OBJECT_ROOT").map_err(|_| "CP0_STORE_OBJECT_ROOT is required")?;
    let (mode, minimum_age_seconds) = parse_arguments(env::args().skip(1))?;
    require_safe_apply_age(mode, minimum_age_seconds)?;

    let pool = connect(&database_url, 2).await?;
    let report = collect_content_objects(&pool, object_root, mode, minimum_age_seconds).await?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<(ObjectGcMode, u64), Box<dyn Error>> {
    let mut mode = ObjectGcMode::DryRun;
    let mut minimum_age_seconds = DEFAULT_OBJECT_GC_MINIMUM_AGE_SECONDS;
    let mut mode_seen = false;
    let mut minimum_age_seen = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--dry-run" if !mode_seen => mode_seen = true,
            "--apply" if !mode_seen => {
                mode = ObjectGcMode::Apply;
                mode_seen = true;
            }
            "--minimum-age-seconds" if !minimum_age_seen => {
                let value = arguments
                    .next()
                    .ok_or("--minimum-age-seconds requires a value")?;
                minimum_age_seconds = value.parse::<u64>()?;
                minimum_age_seen = true;
            }
            _ => return Err(format!("unsupported or repeated argument: {argument}").into()),
        }
    }
    Ok((mode, minimum_age_seconds))
}

fn require_safe_apply_age(
    mode: ObjectGcMode,
    minimum_age_seconds: u64,
) -> Result<(), Box<dyn Error>> {
    if mode == ObjectGcMode::Apply && minimum_age_seconds < DEFAULT_OBJECT_GC_MINIMUM_AGE_SECONDS {
        return Err("apply mode requires a minimum age of at least 86400 seconds".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_a_safe_dry_run() {
        assert_eq!(
            parse_arguments(Vec::new()).unwrap(),
            (ObjectGcMode::DryRun, DEFAULT_OBJECT_GC_MINIMUM_AGE_SECONDS)
        );
    }

    #[test]
    fn rejects_ambiguous_or_unknown_arguments() {
        assert!(parse_arguments(["--apply".into(), "--apply".into()]).is_err());
        assert!(parse_arguments(["--dry-run".into(), "--apply".into()]).is_err());
        assert!(parse_arguments(["--dry-run".into(), "--dry-run".into()]).is_err());
        assert!(
            parse_arguments([
                "--minimum-age-seconds".into(),
                "86400".into(),
                "--minimum-age-seconds".into(),
                "86401".into(),
            ])
            .is_err()
        );
        assert!(parse_arguments(["--minimum-age-seconds".into()]).is_err());
        assert!(parse_arguments(["--unknown".into()]).is_err());
    }

    #[test]
    fn apply_never_uses_a_short_grace_period() {
        assert!(require_safe_apply_age(ObjectGcMode::Apply, 86_399).is_err());
        assert!(require_safe_apply_age(ObjectGcMode::Apply, 86_400).is_ok());
        assert!(require_safe_apply_age(ObjectGcMode::DryRun, 0).is_ok());
    }
}
