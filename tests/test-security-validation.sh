#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
threat_model="$repo_root/docs/THREAT-MODEL.md"
update_adr="$repo_root/docs/adr/0006-verified-updates-and-rollback.md"
fuzz_manifest="$repo_root/fuzz/Cargo.toml"
fuzz_script="$repo_root/scripts/fuzz-smoke.sh"
fuzz_ignore="$repo_root/fuzz/.gitignore"

bash -n "$fuzz_script"
for target in manifest package store_protocol appd_control recovery_backup; do
    grep -q "name = \"$target\"" "$fuzz_manifest"
    test -f "$repo_root/fuzz/fuzz_targets/$target.rs"
done
grep -qx '/artifacts/' "$fuzz_ignore"
grep -qx '/corpus/' "$fuzz_ignore"
grep -qx '/target/' "$fuzz_ignore"
grep -q 'fn byte_slice_verifier_matches_files_and_rejects_truncation' \
    "$repo_root/crates/cp0-recovery/src/lib.rs"

grep -q 'OverlayFS.*not.*integrity' "$threat_model"
grep -q 'development.*SSH' "$threat_model"
grep -q 'physical.*SD' "$threat_model"
grep -q 'third-party security review.*open' "$threat_model"
grep -q 'dm-verity' "$update_adr"
grep -q 'RAUC' "$update_adr"
grep -q 'U-Boot' "$update_adr"
grep -q 'irreversible' "$update_adr"

if grep -Eqi 'third-party (audit|security review).*(complete|passed|closed)' "$threat_model"; then
    echo "error: internal documentation claims an unperformed third-party review" >&2
    exit 1
fi
