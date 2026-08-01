#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
api="$repo_root/schemas/store-control-v1.openapi.json"

jq -e '
  .openapi == "3.1.0" and
  .info.version == "1.0.0" and
  .paths["/oauth/device/code"].post.operationId == "createDeviceCode" and
  .paths["/oauth/device/authorize"].post.operationId == "authorizeDeviceCode" and
  .paths["/oauth/token"].post.operationId == "exchangeDeviceCode" and
  .paths["/oauth/device/code"].post.security == [] and
  .paths["/oauth/token"].post.security == [] and
  (.paths["/oauth/device/authorize"].post | has("security") | not) and
  (.paths["/oauth/token"].post.responses | has("428") | not) and
  .paths["/v1/teams/{team_id}"].get.operationId == "getTeam" and
  .paths["/v1/teams/{team_id}/members/{member_id}:set-role"].post.operationId == "setTeamMemberRole" and
  .paths["/v1/apps/{app_id}/submissions"].post.operationId == "createSubmission" and
  .paths["/v1/submissions/{submission_id}:finalize"].post.operationId == "finalizeSubmission" and
  .paths["/v1/submissions/{submission_id}:withdraw"].post.operationId == "withdrawSubmission" and
  .paths["/v1/submissions/{submission_id}/messages"].post.operationId == "postReviewMessage" and
  .paths["/v1/review/submissions"].get.operationId == "listReviewQueue" and
  .paths["/v1/review/submissions/{submission_id}:begin"].post.operationId == "beginReview" and
  .paths["/v1/review/submissions/{submission_id}/decisions"].post.operationId == "decideReview" and
  .paths["/v1/review/submissions"].get.responses["200"].content["application/json"].schema.properties.items.items["$ref"] == "#/components/schemas/ReviewQueueItem" and
  .paths["/v1/releases"].post.operationId == "createRelease" and
  .paths["/v1/releases/{release_id}"].get.operationId == "getRelease" and
  .paths["/v1/releases/{release_id}:schedule"].post.operationId == "scheduleRelease" and
  .paths["/v1/releases/{release_id}:publish"].post.operationId == "publishRelease" and
  .paths["/v1/releases/{release_id}:pause"].post.operationId == "pauseRelease" and
  .paths["/v1/releases/{release_id}:resume"].post.operationId == "resumeRelease" and
  .paths["/v1/releases/{release_id}:remove"].post.operationId == "removeRelease" and
  .components.schemas.SubmissionState.enum == [
    "draft", "uploading", "processing", "ready-for-review", "in-review",
    "pending-secondary-review", "needs-changes", "approved", "rejected", "withdrawn"
  ] and
  .components.schemas.ReleaseState.enum == [
    "ready", "scheduled", "publishing", "publish-failed", "published", "paused", "removed"
  ] and
  all(
    .components.schemas[
      "Problem", "DeviceCodeRequest", "DeviceCodeResponse",
      "DeviceAuthorizationDecisionRequest", "DeviceTokenRequest", "DeviceTokenResponse",
      "SetTeamMemberRoleRequest", "TeamMember", "Team", "ReviewQueueItem",
      "CreateAppRequest", "App", "AssetDescriptor",
      "CreateSubmissionRequest", "FinalizeSubmissionRequest", "Submission",
      "ReviewMessageRequest", "ReviewMessage", "ReviewDecisionRequest",
      "CreateReleaseRequest", "ScheduleReleaseRequest", "RemovalRequest", "Release"
    ];
    .additionalProperties == false
  ) and
  ([.. | objects | .operationId? // empty] | length) ==
    ([.. | objects | .operationId? // empty] | unique | length)
' "$api" >/dev/null

jq -e '
  def refs($operation): [($operation.parameters // [])[] | .["$ref"]];
  (refs(.paths["/oauth/device/authorize"].post) |
    index("#/components/parameters/IdempotencyKey") != null) and
  (refs(.paths["/v1/teams/{team_id}/members/{member_id}:set-role"].post) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") != null) and
  .paths["/v1/teams/{team_id}/members/{member_id}:set-role"].post.requestBody.required == true and
  (.paths["/v1/teams/{team_id}/members/{member_id}:set-role"].post.responses["200"] |
    .headers.ETag["$ref"] == "#/components/headers/ETag" and
    .content["application/json"].schema["$ref"] == "#/components/schemas/Team") and
  (refs(.paths["/v1/review/submissions/{submission_id}:begin"].post) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") != null) and
  (refs(.paths["/v1/review/submissions/{submission_id}/decisions"].post) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") != null) and
  (refs(.paths["/v1/submissions/{submission_id}/messages"].post) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") == null) and
  (refs(.paths["/v1/submissions/{submission_id}:withdraw"].post) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") != null) and
  (.paths["/v1/submissions/{submission_id}:withdraw"].post | has("requestBody") | not) and
  (.paths["/v1/submissions/{submission_id}:withdraw"].post.responses["200"] |
    .headers.ETag["$ref"] == "#/components/headers/ETag" and
    .content["application/json"].schema["$ref"] == "#/components/schemas/Submission") and
  all([
    .paths["/v1/releases/{release_id}:schedule"].post,
    .paths["/v1/releases/{release_id}:publish"].post,
    .paths["/v1/releases/{release_id}:pause"].post,
    .paths["/v1/releases/{release_id}:resume"].post,
    .paths["/v1/releases/{release_id}:remove"].post
  ][]; refs(.) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") != null) and
  .paths["/v1/releases/{release_id}:schedule"].post.requestBody.required == true and
  .paths["/v1/releases/{release_id}:remove"].post.requestBody.required == true and
  (.paths["/v1/releases/{release_id}:publish"].post | has("requestBody") | not) and
  (.paths["/v1/releases/{release_id}:pause"].post | has("requestBody") | not) and
  (.paths["/v1/releases/{release_id}:resume"].post | has("requestBody") | not)
' "$api" >/dev/null

jq -e '
  [
    .paths | to_entries[] | .key as $path | .value | to_entries[] |
    select(.key == "post" or .key == "put") |
    select($path | startswith("/v1/")) |
    [(.value.parameters // [])[] | .["$ref"]] |
    index("#/components/parameters/IdempotencyKey") != null
  ] | all
' "$api" >/dev/null

echo "PASS Store control API contract"
