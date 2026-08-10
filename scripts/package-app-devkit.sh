#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:-$repo_root/target/app-devkit}
output=$(mkdir -p "$output" && cd "$output" && pwd -P)

pinned_rust=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
    "$repo_root/devkit/toolchain.toml")
if [[ -z $pinned_rust ]] || ! command -v rustup >/dev/null 2>&1; then
    echo "error: pinned Rust toolchain or rustup is unavailable" >&2
    exit 1
fi
if ! rustup run "$pinned_rust" rustc --version >/dev/null 2>&1; then
    echo "error: install the pinned Rust $pinned_rust toolchain before packaging" >&2
    exit 1
fi
cargo=(rustup run "$pinned_rust" cargo)
rustc=(rustup run "$pinned_rust" rustc)

version=$("${cargo[@]}" metadata --manifest-path "$repo_root/Cargo.toml" \
    --format-version 1 --no-deps | jq -r '
        .packages[] | select(.name == "cp0-sdk") | .version
    ')
declared_version=$(awk -F '"' '$1 ~ /^devkit_version = / { print $2 }' \
    "$repo_root/devkit/toolchain.toml")
host=$("${rustc[@]}" -vV | awk -F ': ' '$1 == "host" { print $2 }')
if [[ -z $version || $version == null || -z $declared_version || -z $host ]]; then
    echo "error: cannot resolve DevKit version or host target" >&2
    exit 1
fi
if [[ $version != "$declared_version" ]]; then
    echo "error: SDK version $version does not match DevKit version $declared_version" >&2
    exit 1
fi

bundle_name="cardputerzero-app-devkit-$version-$host"
staging_parent="$repo_root/target/app-devkit-staging"
bundle="$staging_parent/$bundle_name"
case "$bundle" in
    "$repo_root"/target/app-devkit-staging/cardputerzero-app-devkit-*) ;;
    *)
        echo "error: unsafe DevKit staging path" >&2
        exit 1
        ;;
esac
rm -rf -- "$bundle"
mkdir -p "$bundle/bin" "$bundle/docs" "$bundle/examples" "$bundle/schemas" "$bundle/skills"

"${cargo[@]}" build --manifest-path "$repo_root/Cargo.toml" --locked --release -p cp0ctl
install -m 0755 "$repo_root/target/release/cp0ctl" "$bundle/bin/cp0ctl"
cp -R "$repo_root/sdk" "$bundle/sdk"
cp -R "$repo_root/simulator" "$bundle/simulator"
cp -R "$repo_root/devkit" "$bundle/devkit"
cp -R "$repo_root/skills/cardputerzero-build-app" "$bundle/skills/"
examples=(
    hello-card calculator neon-snake camera gallery media-controls music-player notes
    stopwatch
)
for example in "${examples[@]}"; do
    mkdir -p "$bundle/examples/$example/src" "$bundle/examples/$example/assets"
    for file in Cargo.toml Cargo.lock app.json README.md README.zh-CN.md; do
        install -m 0644 "$repo_root/examples/$example/$file" \
            "$bundle/examples/$example/"
    done
    install -m 0644 "$repo_root/examples/$example/src/lib.rs" \
        "$bundle/examples/$example/src/"
    install -m 0644 "$repo_root/examples/$example/assets/screenshot.png" \
        "$bundle/examples/$example/assets/"
done
install -m 0644 "$repo_root/docs/DEVELOPER-GUIDE.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/DEVELOPER-GUIDE.zh-CN.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/APP-DEVKIT-DISTRIBUTION.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/APP-DEVKIT-DISTRIBUTION.zh-CN.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/DEVELOPER-ACCESS.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/DEVELOPER-ACCESS.zh-CN.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/PHOTO-LIBRARY-V2.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/PHOTO-LIBRARY-V2.zh-CN.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/APP-ICON-SPEC-V1.md" "$bundle/docs/"
install -m 0644 "$repo_root/docs/APP-ICON-SPEC-V1.zh-CN.md" "$bundle/docs/"
install -m 0644 "$repo_root/schemas/app-manifest-v1.schema.json" "$bundle/schemas/"
install -m 0644 "$repo_root/schemas/store-listing-v1.schema.json" "$bundle/schemas/"
printf '%s\n' "$version" >"$bundle/VERSION"

# Finder metadata is host state, not part of the versioned SDK payload.
find "$bundle" -type f -name .DS_Store -delete

emcc_version=unavailable
if command -v emcc >/dev/null 2>&1; then
    emcc_version=$(emcc --version | head -n 1)
fi
jq -n \
    --arg version "$version" \
    --arg host "$host" \
    --arg rust "$("${rustc[@]}" --version)" \
    --arg node "$(node --version)" \
    --arg emcc "$emcc_version" \
    '{
        schema_version: 1,
        name: "CardputerZero App DevKit",
        version: $version,
        host: $host,
        built_with: { rust: $rust, node: $node, emscripten: $emcc },
        bundled: [
          "cp0ctl", "sdk", "simulator", "skill", "app-manifest-schema",
          "store-listing-schema", "developer-access-doc", "photo-library-doc",
          "app-icon-doc", "hello-card", "calculator", "neon-snake", "camera",
          "gallery", "media-controls", "music-player", "notes", "stopwatch"
        ]
    }' >"$bundle/devkit.json"

(
    cd "$bundle"
    find . -type f ! -name SHA256SUMS -print0 \
        | sort -z \
        | xargs -0 shasum -a 256 >SHA256SUMS
)

archive="$output/$bundle_name.tar.xz"
COPYFILE_DISABLE=1 tar -C "$staging_parent" -cJf "$archive" "$bundle_name"
(
    cd "$output"
    shasum -a 256 "$bundle_name.tar.xz" >"$bundle_name.tar.xz.sha256"
)
printf 'DevKit: %s\nChecksum: %s.sha256\n' "$archive" "$archive"
