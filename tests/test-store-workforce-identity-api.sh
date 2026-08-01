#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
api="$repo_root/schemas/store-workforce-identity-v1.openapi.json"
design="$repo_root/docs/STORE-WORKFORCE-IDENTITY-V1.md"

jq -e '
  .openapi == "3.1.0" and
  .info.version == "1.0.0" and
  .components.securitySchemes.reviewSession.name == "__Host-cp0_review" and
  .components.securitySchemes.operationsSession.name == "__Host-cp0_operations" and
  .paths["/review/auth/login"].get.security == [] and
  .paths["/review/auth/callback"].get.security == [] and
  .paths["/operations/auth/login"].get.security == [] and
  .paths["/operations/auth/callback"].get.security == [] and
  .paths["/review/v1/session"].get.security == [{"reviewSession": []}] and
  .paths["/operations/v1/session"].get.security == [{"operationsSession": []}] and
  .paths["/review/v1/token"].post.operationId == "issueReviewControlToken" and
  .paths["/operations/v1/token"].post.operationId == "issueOperationsControlToken" and
  .paths["/review/v1/session:logout"].post.operationId == "logoutReviewSession" and
  .paths["/operations/v1/session:logout"].post.operationId == "logoutOperationsSession" and
  .components.schemas.ReviewSession.properties.audience.const == "review" and
  .components.schemas.OperationsSession.properties.audience.const == "operations" and
  .components.schemas.ReviewControlToken.properties.expires_in.maximum == 300 and
  .components.schemas.OperationsControlToken.properties.expires_in.maximum == 300 and
  .components.schemas.ReviewControlToken.properties.scope.const == "store.review" and
  .components.schemas.OperationsControlToken.properties.scope.enum == ["store.editorial", "store.moderation"] and
  .components.schemas.OperationsTokenRequest.properties.scope.enum == ["store.editorial", "store.moderation"] and
  ([.. | objects | .operationId? // empty] | length) ==
    ([.. | objects | .operationId? // empty] | unique | length)
' "$api" >/dev/null

jq -e '
  def refs($operation): [($operation.parameters // [])[] | .["$ref"]];
  all([
    .paths["/review/v1/token"].post,
    .paths["/review/v1/session:logout"].post,
    .paths["/operations/v1/token"].post,
    .paths["/operations/v1/session:logout"].post
  ][]; refs(.) |
    index("#/components/parameters/CsrfToken") != null and
    index("#/components/parameters/IdempotencyKey") != null) and
  (.paths["/review/v1/token"].post.responses["200"].headers["Cache-Control"]["$ref"] ==
    "#/components/headers/NoStore") and
  (.paths["/operations/v1/token"].post.responses["200"].headers["Cache-Control"]["$ref"] ==
    "#/components/headers/NoStore")
' "$api" >/dev/null

if grep -Eq 'refresh_token|oidc_subject|"subject"' "$api"; then
    echo "error: workforce contract must not expose refresh tokens or OIDC subjects" >&2
    exit 1
fi

grep -q '15-minute idle lifetime' "$design"
grep -q 'eight-hour absolute lifetime' "$design"
grep -q 'no more than five minutes' "$design"
grep -q 'Review and Operations audiences never share' "$design"
grep -q 'immediately revokes every bound control token' "$design"
