#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
skill="$repo_root/skills/cardputerzero-build-app"

test "$(sed -n '1p' "$skill/SKILL.md")" = "---"
grep -qx 'name: cardputerzero-build-app' "$skill/SKILL.md"
grep -q '^description: .*CardputerZero' "$skill/SKILL.md"
if grep -R -E '\[TODO\]|TODO:' "$skill"; then
    echo "error: App development Skill contains a placeholder" >&2
    exit 1
fi
grep -Fq '$cardputerzero-build-app' "$skill/agents/openai.yaml"
grep -Fq 'release-ready project workflow is Rust' "$skill/SKILL.md"
grep -Fq 'advanced SDK integration preview' "$skill/references/workflows.md"
grep -Fq -- '--media-actions' "$skill/references/workflows.md"
grep -Fq 'cp0ctl store submit' "$skill/references/store-submission.md"
grep -Fq 'Pair New Computer' "$skill/references/developer-mode.md"
grep -Fq 'Owner SSH Shell may remain Off' "$skill/SKILL.md"
grep -Fq 'simulation-first' "$skill/references/platform-contract.md"
grep -Fq 'first physical key' "$skill/references/platform-contract.md"
grep -Fq 'photos::list_page' "$skill/references/photos.md"
grep -Fq 'cannot change its parent shell' "$skill/SKILL.md"
grep -Fq 'inherits the active Cargo toolchain' "$skill/references/workflows.md"
grep -Fq 'Manifest v1 has no packaged Launcher icon field' \
    "$skill/references/platform-contract.md"
for reference in platform-contract workflows distribution troubleshooting store-submission developer-mode photos; do
    test -s "$skill/references/$reference.md"
    grep -Fq "references/$reference.md" "$skill/SKILL.md"
done
bash -n "$skill/scripts/doctor.sh" "$skill/scripts/verify-app.sh"
"$skill/scripts/doctor.sh" "$repo_root" rust >/dev/null

echo "PASS CardputerZero App development Skill"
