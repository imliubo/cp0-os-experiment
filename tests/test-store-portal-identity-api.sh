#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
api="$repo_root/schemas/store-portal-identity-v1.openapi.json"
design="$repo_root/docs/STORE-PORTAL-IDENTITY-V1.md"

jq -e '
  .openapi == "3.1.0" and
  .info.version == "1.0.0" and
  .components.securitySchemes.portalSession.name == "__Host-cp0_portal" and
  .paths["/portal/auth/login"].get.security == [] and
  .paths["/portal/auth/callback"].get.security == [] and
  .paths["/portal/v1/invitations:inspect"].post.security == [] and
  .paths["/portal/v1/session"].get.operationId == "getPortalSession" and
  .paths["/portal/v1/session:logout"].post.operationId == "logoutPortalSession" and
  .paths["/portal/v1/session:step-up"].post.operationId == "beginPortalStepUp" and
  .paths["/portal/v1/identity-links"].get.operationId == "listPortalIdentityLinks" and
  .paths["/portal/v1/identity-links"].post.operationId == "beginPortalIdentityLink" and
  .paths["/portal/v1/identity-links/{link_id}:remove"].post.operationId == "removePortalIdentityLink" and
  .paths["/portal/v1/teams/{team_id}/invitations"].post.operationId == "createTeamInvitation" and
  .paths["/portal/v1/invitations/{invitation_id}:cancel"].post.operationId == "cancelTeamInvitation" and
  .paths["/portal/v1/invitations:accept"].post.operationId == "acceptTeamInvitation" and
  .components.schemas.InviteRole.enum == ["developer", "release-manager", "viewer"] and
  .components.schemas.InvitationState.enum == ["pending", "accepted", "cancelled", "expired"] and
  .components.schemas.IdentityLinkList.properties.items.maxItems == 8 and
  .components.schemas.InvitationList.properties.items.maxItems == 100 and
  (.paths["/portal/v1/teams/{team_id}/invitations"].get.responses["200"].headers.ETag["$ref"] ==
    "#/components/headers/ETag") and
  (.paths["/portal/v1/teams/{team_id}/invitations"].post.responses["201"].headers.ETag["$ref"] ==
    "#/components/headers/ETag") and
  (.paths["/portal/v1/invitations/{invitation_id}:cancel"].post.responses["200"].headers.ETag["$ref"] ==
    "#/components/headers/ETag") and
  (.paths["/portal/v1/invitations:accept"].post.responses["200"].headers.ETag["$ref"] ==
    "#/components/headers/ETag") and
  (.components.schemas.Invitation.required | index("team_resource_version") != null) and
  (.components.schemas.IdentityLink.properties | has("subject") | not) and
  (.components.schemas.PortalSession.properties | has("access_token") | not) and
  (.components.schemas.PortalSession.properties | has("refresh_token") | not) and
  ([.. | objects | .operationId? // empty] | length) ==
    ([.. | objects | .operationId? // empty] | unique | length)
' "$api" >/dev/null

jq -e '
  def refs($operation): [($operation.parameters // [])[] | .["$ref"]];
  all([
    .paths["/portal/v1/session:logout"].post,
    .paths["/portal/v1/session:step-up"].post,
    .paths["/portal/v1/identity-links"].post,
    .paths["/portal/v1/identity-links/{link_id}:remove"].post,
    .paths["/portal/v1/teams/{team_id}/invitations"].post,
    .paths["/portal/v1/invitations/{invitation_id}:cancel"].post,
    .paths["/portal/v1/invitations:accept"].post
  ][]; refs(.) |
    index("#/components/parameters/CsrfToken") != null and
    index("#/components/parameters/IdempotencyKey") != null) and
  all([
    .paths["/portal/v1/session:step-up"].post,
    .paths["/portal/v1/identity-links"].post,
    .paths["/portal/v1/identity-links/{link_id}:remove"].post,
    .paths["/portal/v1/teams/{team_id}/invitations"].post,
    .paths["/portal/v1/invitations/{invitation_id}:cancel"].post
  ][]; refs(.) | index("#/components/parameters/IfMatch") != null) and
  .paths["/portal/v1/invitations:accept"].post.requestBody.content["application/json"].schema["$ref"] ==
    "#/components/schemas/InvitationTokenRequest" and
  .paths["/portal/v1/invitations:inspect"].post.requestBody.content["application/json"].schema["$ref"] ==
    "#/components/schemas/InvitationTokenRequest"
' "$api" >/dev/null

if jq -r '.paths | keys[]' "$api" | grep -Eq '\{(token|invitation_token)\}'; then
    echo "error: invitation secrets must never appear in a URL" >&2
    exit 1
fi

grep -q '30-minute idle lifetime' "$design"
grep -q 'eight-hour' "$design"
grep -q 'no more than five minutes old' "$design"
grep -q 'expires after seven days' "$design"
grep -q 'stores only its SHA-256 digest' "$design"
grep -q 'Invitation roles are exactly' "$design"
