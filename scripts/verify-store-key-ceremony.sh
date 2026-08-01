#!/usr/bin/env bash
set -euo pipefail

if (($# != 1)); then
    echo "usage: verify-store-key-ceremony EVIDENCE.json" >&2
    exit 2
fi
evidence=$1
if [[ ! -f $evidence || -L $evidence ]]; then
    echo "error: ceremony evidence must be a regular non-symbolic file" >&2
    exit 1
fi
bytes=$(wc -c <"$evidence" | tr -d '[:space:]')
if [[ ! $bytes =~ ^[1-9][0-9]*$ ]] || ((bytes > 32768)); then
    echo "error: ceremony evidence is empty or exceeds 32 KiB" >&2
    exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq is required" >&2
    exit 1
fi

if ! jq -e '
    def hex64: type == "string" and test("^[0-9a-f]{64}$");
    def nullable_hex64: . == null or hex64;
    def sequence: type == "number" and . >= 1 and . <= 9007199254740991 and floor == .;
    def nullable_sequence: . == null or sequence;
    def exact_keys($wanted): (keys | sort) == ($wanted | sort);
    exact_keys([
      "schema_version", "ceremony_id", "environment", "operation", "algorithm",
      "started_unix_seconds", "completed_unix_seconds", "old_key_id", "new_key_id",
      "catalog_sequence_before", "catalog_sequence_after", "hsm_attestation_sha256",
      "trust_update_sha256", "approvals", "outcome"
    ]) and
    .schema_version == 1 and
    (.ceremony_id | type == "string" and
      test("^keycer_[0-9]{8}T[0-9]{6}Z_[0-9a-f]{16}$")) and
    (.environment == "staging" or .environment == "production") and
    (.operation == "generate" or .operation == "rotate" or
      .operation == "revoke" or .operation == "destroy") and
    .algorithm == "ed25519" and
    (.started_unix_seconds | sequence) and
    (.completed_unix_seconds | sequence) and
    .completed_unix_seconds >= .started_unix_seconds and
    .completed_unix_seconds - .started_unix_seconds <= 28800 and
    (.old_key_id | nullable_hex64) and
    (.new_key_id | nullable_hex64) and
    (.catalog_sequence_before | nullable_sequence) and
    (.catalog_sequence_after | nullable_sequence) and
    (.hsm_attestation_sha256 | hex64) and
    (.trust_update_sha256 == null or (.trust_update_sha256 | hex64)) and
    (.outcome == "approved" or .outcome == "aborted") and
    (.approvals | type == "array" and length >= 2 and length <= 4) and
    (all(.approvals[];
      exact_keys(["actor_id", "role", "evidence_sha256"]) and
      (.actor_id | type == "string" and test("^operator_[0-9a-f]{32}$")) and
      (.role == "key-custodian" or .role == "security-officer" or
        .role == "release-operator" or .role == "auditor") and
      (.evidence_sha256 | hex64))) and
    ([.approvals[].actor_id] | unique | length) == (.approvals | length) and
    ([.approvals[].role] | index("key-custodian")) != null and
    ([.approvals[].role] | index("security-officer")) != null and
    (if .operation == "generate" then
       .old_key_id == null and .new_key_id != null and
       .catalog_sequence_before == null and .catalog_sequence_after == null and
       .trust_update_sha256 != null
     elif .operation == "rotate" then
       .old_key_id != null and .new_key_id != null and .old_key_id != .new_key_id and
       (.catalog_sequence_before | sequence) and
       (.catalog_sequence_after | sequence) and
       .catalog_sequence_after > .catalog_sequence_before and
       .trust_update_sha256 != null
     elif .operation == "revoke" then
       .old_key_id != null and
       (.new_key_id == null or .new_key_id != .old_key_id) and
       (.catalog_sequence_before | sequence) and
       (if .new_key_id == null then
          .catalog_sequence_after == null
        else
          (.catalog_sequence_after | sequence) and
          .catalog_sequence_after > .catalog_sequence_before
        end) and
       .trust_update_sha256 != null
     else
       .old_key_id != null and .new_key_id == null and
       (.catalog_sequence_before | sequence) and
       .catalog_sequence_after == .catalog_sequence_before and
       .trust_update_sha256 == null
     end)
' "$evidence" >/dev/null; then
    echo "error: ceremony evidence is malformed or violates the operation policy" >&2
    exit 1
fi

operation=$(jq -r '.operation' "$evidence")
outcome=$(jq -r '.outcome' "$evidence")
printf 'PASS verified Store key ceremony evidence: operation=%s outcome=%s\n' \
    "$operation" "$outcome"
