use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cp0_store_metadata::{ImageAsset, ReleaseState, SubmissionState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_PACKAGE_BYTES: u64 = 8_392_704;
pub const MAX_LISTING_BYTES: u64 = 32_768;
pub const MAX_ASSET_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TeamRole {
    Owner,
    Developer,
    ReleaseManager,
    Viewer,
}

impl TeamRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Developer => "developer",
            Self::ReleaseManager => "release-manager",
            Self::Viewer => "viewer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMember {
    pub member_id: String,
    pub email: String,
    pub role: TeamRole,
    pub two_factor_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamRecord {
    pub team_id: String,
    pub name: String,
    pub members: BTreeMap<String, TeamMember>,
    pub resource_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserIdentity {
    pub team_id: String,
    pub member_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRole {
    Scanner,
    Reviewer,
    Publisher,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIdentity {
    pub service_id: String,
    pub role: ServiceRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationContext {
    pub idempotency_key: String,
    pub request_id: String,
    pub now_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppRecord {
    pub app_id: String,
    pub owner_team_id: String,
    pub default_locale: String,
    pub resource_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionSpec {
    pub version: String,
    pub package_sha256: String,
    pub package_bytes: u64,
    pub listing_sha256: String,
    pub listing_bytes: u64,
    pub assets: Vec<ImageAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionRecord {
    pub submission_id: String,
    pub app_id: String,
    pub version: String,
    pub revision: u32,
    pub state: SubmissionState,
    pub package_sha256: String,
    pub package_bytes: u64,
    pub listing_sha256: String,
    pub listing_bytes: u64,
    pub assets: Vec<ImageAsset>,
    pub resource_version: u64,
    pub created_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRecord {
    pub release_id: String,
    pub submission_id: String,
    pub app_id: String,
    pub version: String,
    pub state: ReleaseState,
    pub rollout_percent: u8,
    pub scheduled_unix_seconds: Option<u64>,
    pub catalog_sequence: Option<u64>,
    pub resource_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseCommand {
    Schedule { publish_unix_seconds: u64 },
    CancelSchedule,
    Publish,
    Pause,
    Resume,
    Remove { reason_code: String, note: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "resource", rename_all = "kebab-case")]
pub enum MutationResult {
    Team(TeamRecord),
    App(AppRecord),
    Submission(SubmissionRecord),
    Release(ReleaseRecord),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEvent {
    pub sequence: u64,
    pub occurred_unix_seconds: u64,
    pub actor_id: String,
    pub action: String,
    pub object_kind: String,
    pub object_id: String,
    pub before_state: Option<String>,
    pub after_state: Option<String>,
    pub resource_version: u64,
    pub request_id: String,
    pub request_sha256: String,
    pub idempotency_key_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboxEvent {
    pub event_id: String,
    pub topic: String,
    pub aggregate_kind: String,
    pub aggregate_id: String,
    pub aggregate_version: u64,
    pub request_sha256: String,
    pub created_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdempotencyRecord {
    request_sha256: String,
    result: MutationResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    InvalidInput(&'static str),
    NotFound,
    Conflict,
    Forbidden,
    TwoFactorRequired,
    PreconditionFailed,
    InvalidTransition,
    IdempotencyConflict,
}

impl ControlError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid-request",
            Self::NotFound => "not-found",
            Self::Conflict => "conflict",
            Self::Forbidden => "forbidden",
            Self::TwoFactorRequired => "two-factor-required",
            Self::PreconditionFailed => "precondition-failed",
            Self::InvalidTransition => "invalid-transition",
            Self::IdempotencyConflict => "idempotency-conflict",
        }
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => {
                write!(formatter, "invalid Store control request: {message}")
            }
            other => formatter.write_str(other.code()),
        }
    }
}

impl std::error::Error for ControlError {}

pub struct ControlPlane {
    teams: BTreeMap<String, TeamRecord>,
    apps: BTreeMap<String, AppRecord>,
    submissions: BTreeMap<String, SubmissionRecord>,
    releases: BTreeMap<String, ReleaseRecord>,
    idempotency: BTreeMap<(String, String), IdempotencyRecord>,
    audit: Vec<AuditEvent>,
    outbox: Vec<OutboxEvent>,
    next_object_id: u128,
    next_event_sequence: u64,
}

impl ControlPlane {
    pub fn bootstrap(
        team_id: &str,
        team_name: &str,
        owner: TeamMember,
    ) -> Result<Self, ControlError> {
        if !valid_prefixed_id(team_id, "team_") {
            return Err(ControlError::InvalidInput("team_id is not canonical"));
        }
        if !valid_name(team_name, 1, 80) || !valid_member(&owner) || owner.role != TeamRole::Owner {
            return Err(ControlError::InvalidInput("owner team metadata is invalid"));
        }
        let mut members = BTreeMap::new();
        members.insert(owner.member_id.clone(), owner);
        let team = TeamRecord {
            team_id: team_id.to_owned(),
            name: team_name.to_owned(),
            members,
            resource_version: 1,
        };
        Ok(Self {
            teams: BTreeMap::from([(team_id.to_owned(), team)]),
            apps: BTreeMap::new(),
            submissions: BTreeMap::new(),
            releases: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            audit: Vec::new(),
            outbox: Vec::new(),
            next_object_id: 1,
            next_event_sequence: 1,
        })
    }

    pub fn team(&self, team_id: &str) -> Option<&TeamRecord> {
        self.teams.get(team_id)
    }

    pub fn app(&self, app_id: &str) -> Option<&AppRecord> {
        self.apps.get(app_id)
    }

    pub fn submission(&self, submission_id: &str) -> Option<&SubmissionRecord> {
        self.submissions.get(submission_id)
    }

    pub fn release(&self, release_id: &str) -> Option<&ReleaseRecord> {
        self.releases.get(release_id)
    }

    pub fn audit_events(&self) -> &[AuditEvent] {
        &self.audit
    }

    pub fn pending_outbox(&self) -> &[OutboxEvent] {
        &self.outbox
    }

    pub fn add_or_update_member(
        &mut self,
        actor: &UserIdentity,
        context: &MutationContext,
        member: TeamMember,
        expected_version: u64,
    ) -> Result<TeamRecord, ControlError> {
        self.require_user(actor, &[TeamRole::Owner])?;
        if !valid_member(&member) {
            return Err(ControlError::InvalidInput("team member is invalid"));
        }
        let mut request = RequestDigest::new("team.member.upsert.v1");
        request.add(&actor.team_id);
        request.add(&member.member_id);
        request.add(&member.email);
        request.add(member.role.as_str());
        request.add(member.two_factor_enabled.to_string());
        request.add(expected_version.to_string());
        let request_sha256 = request.finish();
        if let Some(result) = self.replay(&actor.member_id, context, &request_sha256)? {
            return expect_team(result);
        }
        let current = self
            .teams
            .get(&actor.team_id)
            .ok_or(ControlError::NotFound)?;
        if current.resource_version != expected_version {
            return Err(ControlError::PreconditionFailed);
        }
        let owner_count = current
            .members
            .values()
            .filter(|existing| {
                existing.member_id != member.member_id && existing.role == TeamRole::Owner
            })
            .count()
            + usize::from(member.role == TeamRole::Owner);
        if owner_count == 0
            || current.members.values().any(|existing| {
                existing.member_id != member.member_id && existing.email == member.email
            })
        {
            return Err(ControlError::Conflict);
        }
        let team = self
            .teams
            .get_mut(&actor.team_id)
            .ok_or(ControlError::NotFound)?;
        team.members.insert(member.member_id.clone(), member);
        team.resource_version += 1;
        let updated = team.clone();
        self.commit(
            &actor.member_id,
            context,
            request_sha256,
            "team.member-upserted",
            "team",
            &updated.team_id,
            None,
            None,
            updated.resource_version,
            MutationResult::Team(updated.clone()),
        );
        Ok(updated)
    }

    pub fn register_app(
        &mut self,
        actor: &UserIdentity,
        context: &MutationContext,
        app_id: &str,
        default_locale: &str,
    ) -> Result<AppRecord, ControlError> {
        self.require_user(actor, &[TeamRole::Owner, TeamRole::Developer])?;
        if !cp0_manifest::is_valid_app_id(app_id) || !is_valid_locale(default_locale) {
            return Err(ControlError::InvalidInput("App ID or locale is invalid"));
        }
        let request_sha256 = register_app_request_sha256(&actor.team_id, app_id, default_locale);
        if let Some(result) = self.replay(&actor.member_id, context, &request_sha256)? {
            return expect_app(result);
        }
        if self.apps.contains_key(app_id) {
            return Err(ControlError::Conflict);
        }
        let app = AppRecord {
            app_id: app_id.to_owned(),
            owner_team_id: actor.team_id.clone(),
            default_locale: default_locale.to_owned(),
            resource_version: 1,
        };
        self.apps.insert(app_id.to_owned(), app.clone());
        self.commit(
            &actor.member_id,
            context,
            request_sha256,
            "app.registered",
            "app",
            app_id,
            None,
            None,
            app.resource_version,
            MutationResult::App(app.clone()),
        );
        Ok(app)
    }

    pub fn create_submission(
        &mut self,
        actor: &UserIdentity,
        context: &MutationContext,
        app_id: &str,
        spec: SubmissionSpec,
    ) -> Result<SubmissionRecord, ControlError> {
        self.require_user(actor, &[TeamRole::Owner, TeamRole::Developer])?;
        let app = self.apps.get(app_id).ok_or(ControlError::NotFound)?;
        if app.owner_team_id != actor.team_id {
            return Err(ControlError::Forbidden);
        }
        validate_submission_spec(&spec)?;
        let request_sha256 = create_submission_request_sha256(app_id, &spec);
        if let Some(result) = self.replay(&actor.member_id, context, &request_sha256)? {
            return expect_submission(result);
        }
        let revision = self
            .submissions
            .values()
            .filter(|record| record.app_id == app_id && record.version == spec.version)
            .map(|record| record.revision)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ControlError::Conflict)?;
        let submission_id = self.next_id("sub_");
        let submission = SubmissionRecord {
            submission_id: submission_id.clone(),
            app_id: app_id.to_owned(),
            version: spec.version,
            revision,
            state: SubmissionState::Uploading,
            package_sha256: spec.package_sha256,
            package_bytes: spec.package_bytes,
            listing_sha256: spec.listing_sha256,
            listing_bytes: spec.listing_bytes,
            assets: spec.assets,
            resource_version: 1,
            created_unix_seconds: context.now_unix_seconds,
        };
        self.submissions
            .insert(submission_id.clone(), submission.clone());
        self.commit(
            &actor.member_id,
            context,
            request_sha256,
            "submission.created",
            "submission",
            &submission_id,
            None,
            Some(submission.state.as_str()),
            submission.resource_version,
            MutationResult::Submission(submission.clone()),
        );
        Ok(submission)
    }

    pub fn finalize_submission(
        &mut self,
        actor: &UserIdentity,
        context: &MutationContext,
        submission_id: &str,
        expected_version: u64,
    ) -> Result<SubmissionRecord, ControlError> {
        self.require_user(actor, &[TeamRole::Owner, TeamRole::Developer])?;
        self.require_submission_owner(actor, submission_id)?;
        self.transition_submission(
            &actor.member_id,
            context,
            submission_id,
            expected_version,
            SubmissionState::Processing,
            "submission.finalized",
            "submission.scan-requested",
        )
    }

    pub fn withdraw_submission(
        &mut self,
        actor: &UserIdentity,
        context: &MutationContext,
        submission_id: &str,
        expected_version: u64,
    ) -> Result<SubmissionRecord, ControlError> {
        self.require_user(actor, &[TeamRole::Owner, TeamRole::Developer])?;
        self.require_submission_owner(actor, submission_id)?;
        self.transition_submission(
            &actor.member_id,
            context,
            submission_id,
            expected_version,
            SubmissionState::Withdrawn,
            "submission.withdrawn",
            "submission.withdrawn",
        )
    }

    pub fn scan_submission(
        &mut self,
        actor: &ServiceIdentity,
        context: &MutationContext,
        submission_id: &str,
        expected_version: u64,
        target: SubmissionState,
    ) -> Result<SubmissionRecord, ControlError> {
        if !valid_service(actor, ServiceRole::Scanner)
            || !matches!(
                target,
                SubmissionState::ReadyForReview
                    | SubmissionState::NeedsChanges
                    | SubmissionState::Rejected
            )
        {
            return Err(ControlError::Forbidden);
        }
        self.transition_submission(
            &actor.service_id,
            context,
            submission_id,
            expected_version,
            target,
            "submission.scan-completed",
            "submission.scan-completed",
        )
    }

    pub fn begin_review(
        &mut self,
        actor: &ServiceIdentity,
        context: &MutationContext,
        submission_id: &str,
        expected_version: u64,
    ) -> Result<SubmissionRecord, ControlError> {
        if !valid_service(actor, ServiceRole::Reviewer) {
            return Err(ControlError::Forbidden);
        }
        self.transition_submission(
            &actor.service_id,
            context,
            submission_id,
            expected_version,
            SubmissionState::InReview,
            "submission.review-begun",
            "submission.review-begun",
        )
    }

    pub fn decide_review(
        &mut self,
        actor: &ServiceIdentity,
        context: &MutationContext,
        submission_id: &str,
        expected_version: u64,
        decision: SubmissionState,
    ) -> Result<SubmissionRecord, ControlError> {
        if !valid_service(actor, ServiceRole::Reviewer)
            || !matches!(
                decision,
                SubmissionState::Approved
                    | SubmissionState::NeedsChanges
                    | SubmissionState::Rejected
            )
        {
            return Err(ControlError::Forbidden);
        }
        self.transition_submission(
            &actor.service_id,
            context,
            submission_id,
            expected_version,
            decision,
            "submission.review-decided",
            "submission.review-decided",
        )
    }

    pub fn create_release(
        &mut self,
        actor: &UserIdentity,
        context: &MutationContext,
        submission_id: &str,
        rollout_percent: u8,
    ) -> Result<ReleaseRecord, ControlError> {
        self.require_user(actor, &[TeamRole::Owner, TeamRole::ReleaseManager])?;
        if !(1..=100).contains(&rollout_percent) {
            return Err(ControlError::InvalidInput(
                "rollout_percent is outside 1-100",
            ));
        }
        let submission = self
            .submissions
            .get(submission_id)
            .cloned()
            .ok_or(ControlError::NotFound)?;
        let app = self
            .apps
            .get(&submission.app_id)
            .ok_or(ControlError::NotFound)?;
        if app.owner_team_id != actor.team_id {
            return Err(ControlError::Forbidden);
        }
        if submission.state != SubmissionState::Approved {
            return Err(ControlError::InvalidTransition);
        }
        let mut request = RequestDigest::new("release.create.v1");
        request.add(submission_id);
        request.add(rollout_percent.to_string());
        let request_sha256 = request.finish();
        if let Some(result) = self.replay(&actor.member_id, context, &request_sha256)? {
            return expect_release(result);
        }
        if self.releases.values().any(|release| {
            release.submission_id == submission_id && release.state != ReleaseState::Removed
        }) {
            return Err(ControlError::Conflict);
        }
        let release_id = self.next_id("rel_");
        let release = ReleaseRecord {
            release_id: release_id.clone(),
            submission_id: submission_id.to_owned(),
            app_id: submission.app_id.clone(),
            version: submission.version.clone(),
            state: ReleaseState::Ready,
            rollout_percent,
            scheduled_unix_seconds: None,
            catalog_sequence: None,
            resource_version: 1,
        };
        self.releases.insert(release_id.clone(), release.clone());
        self.commit(
            &actor.member_id,
            context,
            request_sha256,
            "release.created",
            "release",
            &release_id,
            None,
            Some(release.state.as_str()),
            release.resource_version,
            MutationResult::Release(release.clone()),
        );
        Ok(release)
    }

    pub fn mutate_release(
        &mut self,
        actor: &UserIdentity,
        context: &MutationContext,
        release_id: &str,
        expected_version: u64,
        command: ReleaseCommand,
    ) -> Result<ReleaseRecord, ControlError> {
        self.require_user(actor, &[TeamRole::Owner, TeamRole::ReleaseManager])?;
        let existing = self
            .releases
            .get(release_id)
            .ok_or(ControlError::NotFound)?;
        let app = self
            .apps
            .get(&existing.app_id)
            .ok_or(ControlError::NotFound)?;
        if app.owner_team_id != actor.team_id {
            return Err(ControlError::Forbidden);
        }
        validate_release_command(&command, context.now_unix_seconds)?;
        let (target, action, topic) = release_command_transition(&command);
        let mut request = RequestDigest::new("release.mutate.v1");
        request.add(release_id);
        request.add(expected_version.to_string());
        match &command {
            ReleaseCommand::Schedule {
                publish_unix_seconds,
            } => {
                request.add("schedule");
                request.add(publish_unix_seconds.to_string());
            }
            ReleaseCommand::CancelSchedule => request.add("cancel-schedule"),
            ReleaseCommand::Publish => request.add("publish"),
            ReleaseCommand::Pause => request.add("pause"),
            ReleaseCommand::Resume => request.add("resume"),
            ReleaseCommand::Remove { reason_code, note } => {
                request.add("remove");
                request.add(reason_code);
                request.add(note);
            }
        }
        let request_sha256 = request.finish();
        if let Some(result) = self.replay(&actor.member_id, context, &request_sha256)? {
            return expect_release(result);
        }
        let release = self
            .releases
            .get_mut(release_id)
            .ok_or(ControlError::NotFound)?;
        if release.resource_version != expected_version {
            return Err(ControlError::PreconditionFailed);
        }
        if !release.state.can_transition_to(target) {
            return Err(ControlError::InvalidTransition);
        }
        let before = release.state;
        release.state = target;
        match command {
            ReleaseCommand::Schedule {
                publish_unix_seconds,
            } => release.scheduled_unix_seconds = Some(publish_unix_seconds),
            ReleaseCommand::CancelSchedule => release.scheduled_unix_seconds = None,
            ReleaseCommand::Publish => release.scheduled_unix_seconds = None,
            ReleaseCommand::Pause | ReleaseCommand::Resume | ReleaseCommand::Remove { .. } => {}
        }
        release.resource_version += 1;
        let updated = release.clone();
        self.commit(
            &actor.member_id,
            context,
            request_sha256,
            action,
            "release",
            release_id,
            Some(before.as_str()),
            Some(updated.state.as_str()),
            updated.resource_version,
            MutationResult::Release(updated.clone()),
        );
        if topic != action {
            self.outbox.last_mut().unwrap().topic = topic.to_owned();
        }
        Ok(updated)
    }

    pub fn complete_publication(
        &mut self,
        actor: &ServiceIdentity,
        context: &MutationContext,
        release_id: &str,
        expected_version: u64,
        published: bool,
        catalog_sequence: Option<u64>,
    ) -> Result<ReleaseRecord, ControlError> {
        if !valid_service(actor, ServiceRole::Publisher)
            || (published != catalog_sequence.is_some())
        {
            return Err(ControlError::Forbidden);
        }
        let target = if published {
            ReleaseState::Published
        } else {
            ReleaseState::PublishFailed
        };
        let mut request = RequestDigest::new("release.publication-complete.v1");
        request.add(release_id);
        request.add(expected_version.to_string());
        request.add(published.to_string());
        request.add(catalog_sequence.unwrap_or(0).to_string());
        let request_sha256 = request.finish();
        if let Some(result) = self.replay(&actor.service_id, context, &request_sha256)? {
            return expect_release(result);
        }
        if catalog_sequence.is_some_and(|sequence| {
            sequence == 0
                || self
                    .releases
                    .values()
                    .filter_map(|release| release.catalog_sequence)
                    .any(|existing| existing >= sequence)
        }) {
            return Err(ControlError::Conflict);
        }
        let release = self
            .releases
            .get_mut(release_id)
            .ok_or(ControlError::NotFound)?;
        if release.resource_version != expected_version {
            return Err(ControlError::PreconditionFailed);
        }
        if !release.state.can_transition_to(target) {
            return Err(ControlError::InvalidTransition);
        }
        let before = release.state;
        release.state = target;
        release.catalog_sequence = catalog_sequence;
        release.resource_version += 1;
        let updated = release.clone();
        self.commit(
            &actor.service_id,
            context,
            request_sha256,
            "release.publication-completed",
            "release",
            release_id,
            Some(before.as_str()),
            Some(updated.state.as_str()),
            updated.resource_version,
            MutationResult::Release(updated.clone()),
        );
        Ok(updated)
    }

    fn require_user(
        &self,
        actor: &UserIdentity,
        roles: &[TeamRole],
    ) -> Result<&TeamMember, ControlError> {
        let team = self
            .teams
            .get(&actor.team_id)
            .ok_or(ControlError::Forbidden)?;
        let member = team
            .members
            .get(&actor.member_id)
            .ok_or(ControlError::Forbidden)?;
        if !roles.contains(&member.role) {
            return Err(ControlError::Forbidden);
        }
        if !member.two_factor_enabled {
            return Err(ControlError::TwoFactorRequired);
        }
        Ok(member)
    }

    fn require_submission_owner(
        &self,
        actor: &UserIdentity,
        submission_id: &str,
    ) -> Result<(), ControlError> {
        let submission = self
            .submissions
            .get(submission_id)
            .ok_or(ControlError::NotFound)?;
        let app = self
            .apps
            .get(&submission.app_id)
            .ok_or(ControlError::NotFound)?;
        if app.owner_team_id != actor.team_id {
            return Err(ControlError::Forbidden);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_submission(
        &mut self,
        actor_id: &str,
        context: &MutationContext,
        submission_id: &str,
        expected_version: u64,
        target: SubmissionState,
        action: &str,
        topic: &str,
    ) -> Result<SubmissionRecord, ControlError> {
        let mut request = RequestDigest::new("submission.transition.v1");
        request.add(submission_id);
        request.add(expected_version.to_string());
        request.add(target.as_str());
        let request_sha256 = request.finish();
        if let Some(result) = self.replay(actor_id, context, &request_sha256)? {
            return expect_submission(result);
        }
        let submission = self
            .submissions
            .get_mut(submission_id)
            .ok_or(ControlError::NotFound)?;
        if submission.resource_version != expected_version {
            return Err(ControlError::PreconditionFailed);
        }
        if !submission.state.can_transition_to(target) {
            return Err(ControlError::InvalidTransition);
        }
        let before = submission.state;
        submission.state = target;
        submission.resource_version += 1;
        let updated = submission.clone();
        self.commit(
            actor_id,
            context,
            request_sha256,
            action,
            "submission",
            submission_id,
            Some(before.as_str()),
            Some(updated.state.as_str()),
            updated.resource_version,
            MutationResult::Submission(updated.clone()),
        );
        if topic != action {
            self.outbox.last_mut().unwrap().topic = topic.to_owned();
        }
        Ok(updated)
    }

    fn replay(
        &self,
        actor_id: &str,
        context: &MutationContext,
        request_sha256: &str,
    ) -> Result<Option<MutationResult>, ControlError> {
        validate_context(context)?;
        let key_sha256 = sha256_hex(context.idempotency_key.as_bytes());
        match self.idempotency.get(&(actor_id.to_owned(), key_sha256)) {
            Some(record) if record.request_sha256 == request_sha256 => {
                Ok(Some(record.result.clone()))
            }
            Some(_) => Err(ControlError::IdempotencyConflict),
            None => Ok(None),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn commit(
        &mut self,
        actor_id: &str,
        context: &MutationContext,
        request_sha256: String,
        action: &str,
        object_kind: &str,
        object_id: &str,
        before_state: Option<&str>,
        after_state: Option<&str>,
        resource_version: u64,
        result: MutationResult,
    ) {
        let key_sha256 = sha256_hex(context.idempotency_key.as_bytes());
        let sequence = self.next_event_sequence;
        self.next_event_sequence += 1;
        self.audit.push(AuditEvent {
            sequence,
            occurred_unix_seconds: context.now_unix_seconds,
            actor_id: actor_id.to_owned(),
            action: action.to_owned(),
            object_kind: object_kind.to_owned(),
            object_id: object_id.to_owned(),
            before_state: before_state.map(str::to_owned),
            after_state: after_state.map(str::to_owned),
            resource_version,
            request_id: context.request_id.clone(),
            request_sha256: request_sha256.clone(),
            idempotency_key_sha256: key_sha256.clone(),
        });
        self.outbox.push(OutboxEvent {
            event_id: format!("evt_{sequence:032x}"),
            topic: action.to_owned(),
            aggregate_kind: object_kind.to_owned(),
            aggregate_id: object_id.to_owned(),
            aggregate_version: resource_version,
            request_sha256: request_sha256.clone(),
            created_unix_seconds: context.now_unix_seconds,
        });
        self.idempotency.insert(
            (actor_id.to_owned(), key_sha256),
            IdempotencyRecord {
                request_sha256,
                result,
            },
        );
    }

    fn next_id(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}{:032x}", self.next_object_id);
        self.next_object_id += 1;
        id
    }
}

pub fn etag(resource_version: u64) -> String {
    format!("\"{resource_version}\"")
}

fn expect_team(result: MutationResult) -> Result<TeamRecord, ControlError> {
    match result {
        MutationResult::Team(value) => Ok(value),
        _ => Err(ControlError::IdempotencyConflict),
    }
}

fn expect_app(result: MutationResult) -> Result<AppRecord, ControlError> {
    match result {
        MutationResult::App(value) => Ok(value),
        _ => Err(ControlError::IdempotencyConflict),
    }
}

fn expect_submission(result: MutationResult) -> Result<SubmissionRecord, ControlError> {
    match result {
        MutationResult::Submission(value) => Ok(value),
        _ => Err(ControlError::IdempotencyConflict),
    }
}

fn expect_release(result: MutationResult) -> Result<ReleaseRecord, ControlError> {
    match result {
        MutationResult::Release(value) => Ok(value),
        _ => Err(ControlError::IdempotencyConflict),
    }
}

fn validate_context(context: &MutationContext) -> Result<(), ControlError> {
    if !(16..=128).contains(&context.idempotency_key.len())
        || !context
            .idempotency_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._~-".contains(&byte))
        || context.request_id.is_empty()
        || context.request_id.len() > 128
        || !context
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_graphic())
        || context.request_id.contains(&context.idempotency_key)
        || context.now_unix_seconds == 0
    {
        return Err(ControlError::InvalidInput("mutation context is invalid"));
    }
    Ok(())
}

pub fn validate_submission_spec(spec: &SubmissionSpec) -> Result<(), ControlError> {
    if !cp0_manifest::is_valid_app_version(&spec.version)
        || !valid_sha256(&spec.package_sha256)
        || !valid_sha256(&spec.listing_sha256)
        || !(1..=MAX_PACKAGE_BYTES).contains(&spec.package_bytes)
        || !(1..=MAX_LISTING_BYTES).contains(&spec.listing_bytes)
        || !(2..=6).contains(&spec.assets.len())
    {
        return Err(ControlError::InvalidInput(
            "submission descriptor is invalid",
        ));
    }
    let mut paths = BTreeSet::new();
    if spec.assets.iter().any(|asset| {
        !paths.insert(asset.path.as_str())
            || asset.path.is_empty()
            || asset.path.len() > 128
            || asset.path.starts_with('/')
            || asset.path.contains("..")
            || asset.path.contains('\\')
            || !valid_sha256(&asset.sha256)
            || !(1..=MAX_ASSET_BYTES).contains(&asset.bytes)
            || !(1..=320).contains(&asset.width)
            || !(1..=170).contains(&asset.height)
    }) {
        return Err(ControlError::InvalidInput("submission asset is invalid"));
    }
    Ok(())
}

pub fn create_submission_request_sha256(app_id: &str, spec: &SubmissionSpec) -> String {
    let mut request = RequestDigest::new("submission.create.v1");
    request.add(app_id);
    request.add(&spec.version);
    request.add(&spec.package_sha256);
    request.add(spec.package_bytes.to_string());
    request.add(&spec.listing_sha256);
    request.add(spec.listing_bytes.to_string());
    for asset in &spec.assets {
        request.add(&asset.path);
        request.add(&asset.sha256);
        request.add(asset.bytes.to_string());
        request.add(asset.width.to_string());
        request.add(asset.height.to_string());
    }
    request.finish()
}

fn validate_release_command(command: &ReleaseCommand, now: u64) -> Result<(), ControlError> {
    match command {
        ReleaseCommand::Schedule {
            publish_unix_seconds,
        } if *publish_unix_seconds <= now => Err(ControlError::InvalidInput(
            "release schedule must be in the future",
        )),
        ReleaseCommand::Remove { reason_code, note }
            if !valid_reason(reason_code) || !valid_prose(note, 1, 2000) =>
        {
            Err(ControlError::InvalidInput("removal reason is invalid"))
        }
        _ => Ok(()),
    }
}

fn release_command_transition(
    command: &ReleaseCommand,
) -> (ReleaseState, &'static str, &'static str) {
    match command {
        ReleaseCommand::Schedule { .. } => (
            ReleaseState::Scheduled,
            "release.scheduled",
            "release.scheduled",
        ),
        ReleaseCommand::CancelSchedule => (
            ReleaseState::Ready,
            "release.schedule-cancelled",
            "release.schedule-cancelled",
        ),
        ReleaseCommand::Publish => (
            ReleaseState::Publishing,
            "release.publish-requested",
            "release.publish-requested",
        ),
        ReleaseCommand::Pause => (
            ReleaseState::Paused,
            "release.paused",
            "catalog.rebuild-requested",
        ),
        ReleaseCommand::Resume => (
            ReleaseState::Published,
            "release.resumed",
            "catalog.rebuild-requested",
        ),
        ReleaseCommand::Remove { .. } => (
            ReleaseState::Removed,
            "release.removed",
            "catalog.rebuild-requested",
        ),
    }
}

fn valid_member(member: &TeamMember) -> bool {
    valid_prefixed_id(&member.member_id, "member_")
        && member.email.len() <= 254
        && member
            .email
            .split_once('@')
            .is_some_and(|(local, host)| !local.is_empty() && host.contains('.'))
}

fn valid_prefixed_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

pub fn is_valid_locale(locale: &str) -> bool {
    if locale.len() < 2 || locale.len() > 16 || !locale.is_ascii() {
        return false;
    }
    let parts = locale.split('-').collect::<Vec<_>>();
    if parts.is_empty()
        || parts.len() > 3
        || !(2..=3).contains(&parts[0].len())
        || !parts[0].bytes().all(|byte| byte.is_ascii_lowercase())
    {
        return false;
    }
    let mut index = 1;
    if parts.get(index).is_some_and(|part| {
        part.len() == 4
            && part.as_bytes()[0].is_ascii_uppercase()
            && part.as_bytes()[1..].iter().all(u8::is_ascii_lowercase)
    }) {
        index += 1;
    }
    if let Some(region) = parts.get(index) {
        if !((region.len() == 2 && region.bytes().all(|byte| byte.is_ascii_uppercase()))
            || (region.len() == 3 && region.bytes().all(|byte| byte.is_ascii_digit())))
        {
            return false;
        }
        index += 1;
    }
    index == parts.len()
}

pub fn register_app_request_sha256(team_id: &str, app_id: &str, default_locale: &str) -> String {
    let mut request = RequestDigest::new("app.register.v1");
    request.add(team_id);
    request.add(app_id);
    request.add(default_locale);
    request.finish()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_name(value: &str, min: usize, max: usize) -> bool {
    let count = value.chars().count();
    (min..=max).contains(&count) && value.trim() == value && !value.chars().any(char::is_control)
}

fn valid_prose(value: &str, min: usize, max: usize) -> bool {
    let count = value.chars().count();
    (min..=max).contains(&count)
        && value.trim() == value
        && !value
            .chars()
            .any(|character| character.is_control() && character != '\n')
}

fn valid_reason(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_service(actor: &ServiceIdentity, expected_role: ServiceRole) -> bool {
    actor.role == expected_role
        && (1..=64).contains(&actor.service_id.len())
        && actor.service_id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || (index > 0 && byte == b'-')
        })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

struct RequestDigest(Sha256);

impl RequestDigest {
    fn new(domain: &str) -> Self {
        let mut digest = Sha256::new();
        digest.update(domain.as_bytes());
        digest.update([0]);
        Self(digest)
    }

    fn add(&mut self, value: impl AsRef<str>) {
        let bytes = value.as_ref().as_bytes();
        self.0.update((bytes.len() as u64).to_be_bytes());
        self.0.update(bytes);
    }

    fn finish(self) -> String {
        let digest = self.0.finalize();
        let mut encoded = String::with_capacity(64);
        for byte in digest {
            encoded.push_str(&format!("{byte:02x}"));
        }
        encoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEAM_ID: &str = "team_35d07f00000000000000000000000001";
    const OWNER_ID: &str = "member_00000000000000000000000000000001";

    fn owner_member() -> TeamMember {
        TeamMember {
            member_id: OWNER_ID.into(),
            email: "owner@example.dev".into(),
            role: TeamRole::Owner,
            two_factor_enabled: true,
        }
    }

    fn owner() -> UserIdentity {
        UserIdentity {
            team_id: TEAM_ID.into(),
            member_id: OWNER_ID.into(),
        }
    }

    fn context(key: &str, now: u64) -> MutationContext {
        MutationContext {
            idempotency_key: key.into(),
            request_id: format!("req-{now:016x}"),
            now_unix_seconds: now,
        }
    }

    fn plane() -> ControlPlane {
        ControlPlane::bootstrap(TEAM_ID, "M5 Labs", owner_member()).unwrap()
    }

    fn assets() -> Vec<ImageAsset> {
        vec![
            ImageAsset {
                path: "assets/icon.png".into(),
                sha256: "a".repeat(64),
                bytes: 512,
                width: 48,
                height: 48,
            },
            ImageAsset {
                path: "assets/screen.png".into(),
                sha256: "b".repeat(64),
                bytes: 1024,
                width: 320,
                height: 170,
            },
        ]
    }

    fn spec(version: &str) -> SubmissionSpec {
        SubmissionSpec {
            version: version.into(),
            package_sha256: "c".repeat(64),
            package_bytes: 4096,
            listing_sha256: "d".repeat(64),
            listing_bytes: 1024,
            assets: assets(),
        }
    }

    fn register_app(plane: &mut ControlPlane) -> AppRecord {
        plane
            .register_app(
                &owner(),
                &context("register-app-0001", 100),
                "dev.cardputerzero.notes",
                "en-US",
            )
            .unwrap()
    }

    fn approved_submission(plane: &mut ControlPlane) -> SubmissionRecord {
        register_app(plane);
        let created = plane
            .create_submission(
                &owner(),
                &context("create-submission-01", 101),
                "dev.cardputerzero.notes",
                spec("1.0.0"),
            )
            .unwrap();
        let processing = plane
            .finalize_submission(
                &owner(),
                &context("finalize-submit-001", 102),
                &created.submission_id,
                1,
            )
            .unwrap();
        let scanner = ServiceIdentity {
            service_id: "scanner-primary".into(),
            role: ServiceRole::Scanner,
        };
        let ready = plane
            .scan_submission(
                &scanner,
                &context("scan-complete-0001", 103),
                &created.submission_id,
                processing.resource_version,
                SubmissionState::ReadyForReview,
            )
            .unwrap();
        let reviewer = ServiceIdentity {
            service_id: "reviewer-primary".into(),
            role: ServiceRole::Reviewer,
        };
        let reviewing = plane
            .begin_review(
                &reviewer,
                &context("begin-review-0001", 104),
                &created.submission_id,
                ready.resource_version,
            )
            .unwrap();
        plane
            .decide_review(
                &reviewer,
                &context("decide-review-0001", 105),
                &created.submission_id,
                reviewing.resource_version,
                SubmissionState::Approved,
            )
            .unwrap()
    }

    #[test]
    fn app_ids_are_permanent_and_idempotent_replay_is_exact() {
        let mut plane = plane();
        let ctx = context("register-app-0001", 100);
        let first = plane
            .register_app(&owner(), &ctx, "dev.cardputerzero.notes", "en-US")
            .unwrap();
        let replay = plane
            .register_app(&owner(), &ctx, "dev.cardputerzero.notes", "en-US")
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(plane.audit_events().len(), 1);
        assert_eq!(plane.pending_outbox().len(), 1);
        assert_eq!(
            plane.register_app(&owner(), &ctx, "dev.cardputerzero.other", "en-US"),
            Err(ControlError::IdempotencyConflict)
        );
        assert_eq!(
            plane.register_app(
                &owner(),
                &context("register-app-0002", 101),
                "dev.cardputerzero.notes",
                "en-US"
            ),
            Err(ControlError::Conflict)
        );
    }

    #[test]
    fn roles_and_two_factor_are_loaded_from_team_state() {
        let mut plane = plane();
        let member = TeamMember {
            member_id: "member_00000000000000000000000000000002".into(),
            email: "release@example.dev".into(),
            role: TeamRole::Developer,
            two_factor_enabled: false,
        };
        plane
            .add_or_update_member(
                &owner(),
                &context("add-member-000001", 100),
                member.clone(),
                1,
            )
            .unwrap();
        let identity = UserIdentity {
            team_id: TEAM_ID.into(),
            member_id: member.member_id.clone(),
        };
        assert_eq!(
            plane.register_app(
                &identity,
                &context("register-denied-01", 101),
                "dev.cardputerzero.denied",
                "en-US"
            ),
            Err(ControlError::TwoFactorRequired)
        );
        let mut enabled = member;
        enabled.two_factor_enabled = true;
        plane
            .add_or_update_member(&owner(), &context("enable-member-0001", 102), enabled, 2)
            .unwrap();
        assert!(
            plane
                .register_app(
                    &identity,
                    &context("register-allowed-01", 103),
                    "dev.cardputerzero.allowed",
                    "en-US"
                )
                .is_ok()
        );
        let audit_count = plane.audit_events().len();
        let demoted_owner = TeamMember {
            role: TeamRole::Viewer,
            ..owner_member()
        };
        assert_eq!(
            plane.add_or_update_member(
                &owner(),
                &context("demote-last-owner", 104),
                demoted_owner,
                3
            ),
            Err(ControlError::Conflict)
        );
        assert_eq!(plane.audit_events().len(), audit_count);
    }

    #[test]
    fn only_approved_immutable_revisions_can_create_releases() {
        let mut plane = plane();
        register_app(&mut plane);
        let submission = plane
            .create_submission(
                &owner(),
                &context("create-submission-01", 101),
                "dev.cardputerzero.notes",
                spec("1.0.0"),
            )
            .unwrap();
        assert_eq!(
            plane.create_release(
                &owner(),
                &context("release-too-early1", 102),
                &submission.submission_id,
                100
            ),
            Err(ControlError::InvalidTransition)
        );
        let approved = {
            let processing = plane
                .finalize_submission(
                    &owner(),
                    &context("finalize-submit-001", 103),
                    &submission.submission_id,
                    1,
                )
                .unwrap();
            let scanner = ServiceIdentity {
                service_id: "scanner-primary".into(),
                role: ServiceRole::Scanner,
            };
            let ready = plane
                .scan_submission(
                    &scanner,
                    &context("scan-complete-0001", 104),
                    &submission.submission_id,
                    processing.resource_version,
                    SubmissionState::ReadyForReview,
                )
                .unwrap();
            let reviewer = ServiceIdentity {
                service_id: "reviewer-primary".into(),
                role: ServiceRole::Reviewer,
            };
            let reviewing = plane
                .begin_review(
                    &reviewer,
                    &context("begin-review-0001", 105),
                    &submission.submission_id,
                    ready.resource_version,
                )
                .unwrap();
            plane
                .decide_review(
                    &reviewer,
                    &context("decide-review-0001", 106),
                    &submission.submission_id,
                    reviewing.resource_version,
                    SubmissionState::Approved,
                )
                .unwrap()
        };
        let release = plane
            .create_release(
                &owner(),
                &context("create-release-0001", 107),
                &approved.submission_id,
                25,
            )
            .unwrap();
        assert_eq!(release.state, ReleaseState::Ready);
        assert_eq!(release.rollout_percent, 25);
        assert_eq!(approved.revision, 1);
        let second = plane
            .create_submission(
                &owner(),
                &context("create-submission-02", 108),
                "dev.cardputerzero.notes",
                spec("1.0.0"),
            )
            .unwrap();
        assert_eq!(second.revision, 2);
    }

    #[test]
    fn preconditions_fail_without_partial_audit_or_outbox_writes() {
        let mut plane = plane();
        let approved = approved_submission(&mut plane);
        let release = plane
            .create_release(
                &owner(),
                &context("create-release-0001", 106),
                &approved.submission_id,
                100,
            )
            .unwrap();
        let before_audit = plane.audit_events().len();
        let before_outbox = plane.pending_outbox().len();
        assert_eq!(
            plane.mutate_release(
                &owner(),
                &context("publish-release-001", 107),
                &release.release_id,
                99,
                ReleaseCommand::Publish
            ),
            Err(ControlError::PreconditionFailed)
        );
        assert_eq!(plane.audit_events().len(), before_audit);
        assert_eq!(plane.pending_outbox().len(), before_outbox);
        let publishing = plane
            .mutate_release(
                &owner(),
                &context("publish-release-002", 108),
                &release.release_id,
                1,
                ReleaseCommand::Publish,
            )
            .unwrap();
        assert_eq!(
            plane.pending_outbox().last().unwrap().topic,
            "release.publish-requested"
        );
        let publisher = ServiceIdentity {
            service_id: "publisher-primary".into(),
            role: ServiceRole::Publisher,
        };
        let published = plane
            .complete_publication(
                &publisher,
                &context("finish-release-0001", 109),
                &release.release_id,
                publishing.resource_version,
                true,
                Some(18_000_000_001),
            )
            .unwrap();
        assert_eq!(published.state, ReleaseState::Published);
        assert_eq!(published.catalog_sequence, Some(18_000_000_001));
        assert_eq!(etag(published.resource_version), "\"3\"");
        assert_eq!(
            plane.complete_publication(
                &publisher,
                &context("finish-release-0002", 110),
                &release.release_id,
                publishing.resource_version,
                true,
                Some(18_000_000_001)
            ),
            Err(ControlError::Conflict)
        );
    }

    #[test]
    fn audit_records_hash_but_never_raw_idempotency_key() {
        let mut plane = plane();
        register_app(&mut plane);
        let encoded = serde_json::to_string(plane.audit_events()).unwrap();
        assert!(!encoded.contains("register-app-0001"));
        assert!(encoded.contains("idempotency_key_sha256"));
        assert_eq!(plane.audit_events()[0].idempotency_key_sha256.len(), 64);
    }

    #[test]
    fn rejects_unbounded_assets_and_unsafe_service_identity() {
        let mut plane = plane();
        register_app(&mut plane);
        let mut oversized = spec("1.0.0");
        oversized.assets[1].width = 321;
        assert!(matches!(
            plane.create_submission(
                &owner(),
                &context("invalid-submission1", 101),
                "dev.cardputerzero.notes",
                oversized
            ),
            Err(ControlError::InvalidInput(_))
        ));
        let created = plane
            .create_submission(
                &owner(),
                &context("valid-submission-01", 102),
                "dev.cardputerzero.notes",
                spec("1.0.0"),
            )
            .unwrap();
        let processing = plane
            .finalize_submission(
                &owner(),
                &context("finalize-submit-002", 103),
                &created.submission_id,
                1,
            )
            .unwrap();
        let audit_count = plane.audit_events().len();
        let unsafe_scanner = ServiceIdentity {
            service_id: "scanner\nforged".into(),
            role: ServiceRole::Scanner,
        };
        assert_eq!(
            plane.scan_submission(
                &unsafe_scanner,
                &context("unsafe-scanner-0001", 104),
                &created.submission_id,
                processing.resource_version,
                SubmissionState::ReadyForReview
            ),
            Err(ControlError::Forbidden)
        );
        assert_eq!(plane.audit_events().len(), audit_count);
    }
}
