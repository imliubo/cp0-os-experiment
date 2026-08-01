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
  .paths["/v1/review/submissions/{submission_id}"].get.operationId == "getReviewSubmissionDetail" and
  .paths["/v1/review/submissions/{submission_id}:begin"].post.operationId == "beginReview" and
  .paths["/v1/review/submissions/{submission_id}/decisions"].post.operationId == "decideReview" and
  .paths["/v1/review/submissions"].get.responses["200"].content["application/json"].schema.properties.items.items["$ref"] == "#/components/schemas/ReviewQueueItem" and
  .components.schemas.ReviewQueueItem.properties.risk["$ref"] == "#/components/schemas/RiskAssessment" and
  .components.schemas.ReviewQueueItem.properties.app["$ref"] == "#/components/schemas/ReviewApp" and
  .components.schemas.ReviewSubmissionDetail.properties.scan["$ref"] == "#/components/schemas/ReviewScan" and
  .paths["/v1/releases"].post.operationId == "createRelease" and
  .paths["/v1/releases/{release_id}"].get.operationId == "getRelease" and
  .paths["/v1/releases/{release_id}:schedule"].post.operationId == "scheduleRelease" and
  .paths["/v1/releases/{release_id}:publish"].post.operationId == "publishRelease" and
  .paths["/v1/releases/{release_id}:pause"].post.operationId == "pauseRelease" and
  .paths["/v1/releases/{release_id}:resume"].post.operationId == "resumeRelease" and
  .paths["/v1/releases/{release_id}:remove"].post.operationId == "removeRelease" and
  .paths["/v1/editorial/releases"].get.operationId == "listPublishedEditorialReleases" and
  .paths["/v1/editorial/today"].get.operationId == "getTodayEditorial" and
  .paths["/v1/editorial/today"].post.operationId == "createTodayEditorial" and
  .paths["/v1/editorial/today"].put.operationId == "updateTodayEditorial" and
  .paths["/reports/v1/content"].post.operationId == "submitContentReport" and
  .paths["/reports/v1/content"].post.security == [] and
  .paths["/v1/moderation/reports"].get.operationId == "listModerationReports" and
  .paths["/v1/moderation/reports/{report_id}:decide"].post.operationId == "decideContentReport" and
  .paths["/v1/apps/{app_id}/moderation-notices"].get.operationId == "listDeveloperModerationNotices" and
  .paths["/v1/moderation/notices/{notice_id}:appeal"].post.operationId == "appealDeveloperNotice" and
  .paths["/v1/moderation/appeals/{appeal_id}:decide"].post.operationId == "decideModerationAppeal" and
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
      "SetTeamMemberRoleRequest", "TeamMember", "Team", "ReviewQueueItem", "ReviewApp",
      "ReviewScanFinding", "ReviewScan", "ReviewAssignment", "ReviewDecisionRecord",
      "ReviewDetailMessage", "ReviewAuditEvent", "ReviewSubmissionDetail", "RiskAssessment",
      "CreateAppRequest", "App", "AssetDescriptor",
      "CreateSubmissionRequest", "FinalizeSubmissionRequest", "Submission",
      "ReviewMessageRequest", "ReviewMessage", "ReviewDecisionRequest",
      "CreateReleaseRequest", "ScheduleReleaseRequest", "RemovalRequest", "Release",
      "EditorialLayoutRequest", "EditorialRelease", "EditorialReleaseList",
      "EditorialCollectionRequest", "EditorialItem",
      "EditorialCollection", "EditorialLayout", "ContentReportRequest", "ContentReport",
      "ModerationDecisionRequest",
      "DeveloperNotice", "ModerationDecision", "ModerationReportQueue",
      "DeveloperNoticeList", "AppealRequest", "AppealDecisionRequest", "ModerationAppeal"
    ];
    .additionalProperties == false
  ) and
  ([.. | objects | .operationId? // empty] | length) ==
    ([.. | objects | .operationId? // empty] | unique | length)
' "$api" >/dev/null

jq -e '
  def refs($operation): [($operation.parameters // [])[] | .["$ref"]];
  (refs(.paths["/reports/v1/content"].post) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") == null) and
  (.paths["/reports/v1/content"].post.requestBody.content["application/json"].schema["$ref"] ==
    "#/components/schemas/ContentReportRequest") and
  (.paths["/reports/v1/content"].post.responses["202"] |
    .headers.ETag["$ref"] == "#/components/headers/ETag" and
    .content["application/json"].schema["$ref"] == "#/components/schemas/ContentReport") and
  all([
    .paths["/v1/moderation/reports/{report_id}:decide"].post,
    .paths["/v1/moderation/notices/{notice_id}:appeal"].post,
    .paths["/v1/moderation/appeals/{appeal_id}:decide"].post
  ][]; refs(.) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") != null) and
  .components.schemas.ContentReportRequest.additionalProperties == false and
  (.components.schemas.ContentReportRequest.properties | keys) ==
    ["app_id", "reason_code", "release_id", "version"] and
  .components.schemas.ModerationDecisionRequest.properties.reason_codes.maxItems == 4 and
  .components.schemas.ModerationDecisionRequest.properties.reason_codes.uniqueItems == true and
  .components.schemas.AppealRequest.additionalProperties == false and
  .components.schemas.ModerationReportQueue.properties.items.maxItems == 50 and
  .components.schemas.DeveloperNoticeList.properties.items.maxItems == 50
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
  def refs($operation): [($operation.parameters // [])[] | .["$ref"]];
  (refs(.paths["/v1/editorial/today"].post) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") == null) and
  (refs(.paths["/v1/editorial/today"].put) |
    index("#/components/parameters/IdempotencyKey") != null and
    index("#/components/parameters/IfMatch") != null) and
  (.paths["/v1/editorial/today"].get.parameters // []) == [] and
  .paths["/v1/editorial/today"].post.requestBody.required == true and
  .paths["/v1/editorial/today"].put.requestBody.required == true and
  .paths["/v1/editorial/today"].post.requestBody.content["application/json"].schema["$ref"] ==
    "#/components/schemas/EditorialLayoutRequest" and
  .paths["/v1/editorial/today"].put.requestBody.content["application/json"].schema["$ref"] ==
    "#/components/schemas/EditorialLayoutRequest" and
  all([
    .paths["/v1/editorial/today"].get.responses["200"],
    .paths["/v1/editorial/today"].post.responses["201"],
    .paths["/v1/editorial/today"].put.responses["200"]
  ][];
    .headers.ETag["$ref"] == "#/components/headers/ETag" and
    .content["application/json"].schema["$ref"] == "#/components/schemas/EditorialLayout") and
  .components.schemas.EditorialLayoutRequest.properties.collections.minItems == 1 and
  .components.schemas.EditorialLayoutRequest.properties.collections.maxItems == 2 and
  .components.schemas.EditorialCollectionRequest.properties.release_ids.minItems == 1 and
  .components.schemas.EditorialCollectionRequest.properties.release_ids.maxItems == 4 and
  .components.schemas.EditorialCollectionRequest.properties.release_ids.uniqueItems == true and
  .components.schemas.EditorialReleaseList.properties.items.maxItems == 50 and
  .components.schemas.EditorialReleaseList.properties.next_cursor.maxLength == 53 and
  .paths["/v1/editorial/releases"].get.responses["200"].content["application/json"].schema["$ref"] ==
    "#/components/schemas/EditorialReleaseList" and
  .components.schemas.EditorialLayout.properties.layout_id.const == "today"
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
