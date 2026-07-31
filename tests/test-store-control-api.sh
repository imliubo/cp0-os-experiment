#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
api="$repo_root/schemas/store-control-v1.openapi.json"

jq -e '
  .openapi == "3.1.0" and
  .info.version == "1.0.0" and
  .paths["/oauth/device/code"].post.operationId == "createDeviceCode" and
  .paths["/v1/apps/{app_id}/submissions"].post.operationId == "createSubmission" and
  .paths["/v1/submissions/{submission_id}:finalize"].post.operationId == "finalizeSubmission" and
  .paths["/v1/review/submissions/{submission_id}/decisions"].post.operationId == "decideReview" and
  .paths["/v1/releases/{release_id}:publish"].post.operationId == "publishRelease" and
  .components.schemas.SubmissionState.enum == [
    "draft", "uploading", "processing", "ready-for-review", "in-review",
    "needs-changes", "approved", "rejected", "withdrawn"
  ] and
  .components.schemas.ReleaseState.enum == [
    "ready", "scheduled", "publishing", "publish-failed", "published", "paused", "removed"
  ] and
  all(
    .components.schemas[
      "Problem", "DeviceCodeRequest", "DeviceCodeResponse", "DeviceTokenRequest",
      "DeviceTokenResponse", "CreateAppRequest", "App", "AssetDescriptor",
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
  [
    .paths | to_entries[] | .key as $path | .value | to_entries[] |
    select(.key == "post" or .key == "put") |
    select($path | startswith("/v1/")) |
    [(.value.parameters // [])[] | .["$ref"]] |
    index("#/components/parameters/IdempotencyKey") != null
  ] | all
' "$api" >/dev/null

echo "PASS Store control API contract"
