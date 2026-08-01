#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verifier="$repo_root/scripts/verify-store-key-ceremony.sh"
schema="$repo_root/schemas/store-key-ceremony-v1.schema.json"
test_root="$repo_root/target/test-tmp/store-key-ceremony"
case "$test_root" in
    "$repo_root"/target/test-tmp/*) ;;
    *) echo "error: unsafe ceremony test directory" >&2; exit 1 ;;
esac
rm -rf -- "$test_root"
mkdir -p "$test_root"
trap 'rm -rf -- "$test_root"' EXIT

jq empty "$schema"
bash -n "$verifier"

valid="$test_root/valid.json"
jq -n '{
  schema_version: 1,
  ceremony_id: "keycer_20260802T010000Z_0123456789abcdef",
  environment: "production",
  operation: "rotate",
  algorithm: "ed25519",
  started_unix_seconds: 1785622800,
  completed_unix_seconds: 1785623400,
  old_key_id: ("a" * 64),
  new_key_id: ("b" * 64),
  catalog_sequence_before: 500,
  catalog_sequence_after: 501,
  hsm_attestation_sha256: ("c" * 64),
  trust_update_sha256: ("d" * 64),
  approvals: [
    {
      actor_id: "operator_11111111111111111111111111111111",
      role: "key-custodian",
      evidence_sha256: ("e" * 64)
    },
    {
      actor_id: "operator_22222222222222222222222222222222",
      role: "security-officer",
      evidence_sha256: ("f" * 64)
    }
  ],
  outcome: "approved"
}' >"$valid"
"$verifier" "$valid" >/dev/null

expect_reject() {
    local label=$1 filter=$2 output
    output="$test_root/$label.json"
    jq "$filter" "$valid" >"$output"
    if "$verifier" "$output" >/dev/null 2>&1; then
        echo "error: invalid ceremony evidence passed: $label" >&2
        exit 1
    fi
}

expect_reject extra-field '.private_key = "must-never-be-recorded"'
expect_reject reused-key '.new_key_id = .old_key_id'
expect_reject sequence-reuse '.catalog_sequence_after = .catalog_sequence_before'
expect_reject duplicate-actor '.approvals[1].actor_id = .approvals[0].actor_id'
expect_reject missing-custodian '.approvals[0].role = "auditor"'
expect_reject long-ceremony '.completed_unix_seconds = .started_unix_seconds + 28801'
expect_reject no-trust-update '.trust_update_sha256 = null'

ln -s "$valid" "$test_root/evidence-link.json"
if "$verifier" "$test_root/evidence-link.json" >/dev/null 2>&1; then
    echo "error: symbolic ceremony evidence passed" >&2
    exit 1
fi

echo "PASS Store key ceremony evidence contract"
