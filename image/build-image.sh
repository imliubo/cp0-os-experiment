#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source "$repo_root/image/pi-gen/upstream.env"

pi_gen_dir=${PI_GEN_DIR:-$repo_root/target/pi-gen}
password=${CP0_FIRST_USER_PASSWORD:-}
container_name=${CP0_BUILD_CONTAINER:-cardputerzero-pigen}
container_image="cardputerzero-pigen:${PI_GEN_COMMIT:0:12}"
deploy_dir="$repo_root/deploy"
apt_proxy=${CP0_APT_PROXY:-${http_proxy:-}}
resume_build=${CP0_RESUME_BUILD:-0}
image_name=${CP0_IMAGE_NAME:-}

if [[ $(uname -s) == Darwin && -n "$apt_proxy" ]]; then
    apt_proxy=${apt_proxy/127.0.0.1/host.docker.internal}
    apt_proxy=${apt_proxy/localhost/host.docker.internal}
fi

if [[ -z "$password" ]]; then
    echo "error: CP0_FIRST_USER_PASSWORD is required for the development image" >&2
    exit 1
fi

if [[ ! -d "$pi_gen_dir/.git" ]]; then
    mkdir -p "$(dirname "$pi_gen_dir")"
    git clone --filter=blob:none --branch "$PI_GEN_BRANCH" \
        "$PI_GEN_REPOSITORY" "$pi_gen_dir"
fi
if ! git -C "$pi_gen_dir" cat-file -e "$PI_GEN_COMMIT^{commit}" 2>/dev/null; then
    git -C "$pi_gen_dir" fetch origin "$PI_GEN_BRANCH"
fi
git -C "$pi_gen_dir" checkout --detach "$PI_GEN_COMMIT"
test "$(git -C "$pi_gen_dir" rev-parse HEAD)" = "$PI_GEN_COMMIT"

# CM0 uses only the arm64 rpi-v8 kernel. Avoid installing the Pi 5 kernel and
# headers in stage0 merely to purge them again in the board-specific stage.
firmware_packages="$pi_gen_dir/stage0/02-firmware/01-packages"
for package in linux-image-rpi-2712 linux-headers-rpi-2712; do
    sed -i.bak "/^${package}$/d" "$firmware_packages"
    rm -f "${firmware_packages}.bak"
done
if grep -q -- '-rpi-2712$' "$firmware_packages"; then
    echo "error: Pi 5 kernel package remains in stage0" >&2
    exit 1
fi

for apt_source in \
    "$pi_gen_dir/stage0/prerun.sh" \
    "$pi_gen_dir/stage0/00-configure-apt/files/debian.sources"; do
    sed -i.bak 's|http://deb.debian.org|https://deb.debian.org|g' "$apt_source"
    rm -f "${apt_source}.bak"
done
printf '%s\n' \
    'Acquire::http { Proxy "APT_PROXY"; };' \
    'Acquire::https { Proxy "APT_PROXY"; };' \
    >"$pi_gen_dir/stage0/00-configure-apt/files/51cache"

rm -rf "$pi_gen_dir/stage-cardputerzero-os"
cp -R "$repo_root/image/pi-gen/stage-cardputerzero-os" \
    "$pi_gen_dir/stage-cardputerzero-os"
mkdir -p "$pi_gen_dir/stage-cardputerzero-os/01-compositor/system-shell"
cp "$repo_root/system-shell/include/cp0_ui.h" \
    "$repo_root/system-shell/src/ui.c" \
    "$repo_root/system-shell/src/main.c" \
    "$pi_gen_dir/stage-cardputerzero-os/01-compositor/system-shell/"
mkdir -p "$pi_gen_dir/stage-cardputerzero-os/01-compositor/policy"
cp "$repo_root/compositor-policy/cardputerzero-policy.c" \
    "$repo_root/protocols/cardputerzero-system-shell-v1.xml" \
    "$pi_gen_dir/stage-cardputerzero-os/01-compositor/policy/"

"$repo_root/scripts/build-appd.sh"
"$repo_root/scripts/build-app-runtime.sh"
"$repo_root/scripts/build-example-app.sh"
platform_payload="$pi_gen_dir/stage-cardputerzero-os/02-app-platform/payload"
mkdir -p "$platform_payload/systemd" "$platform_payload/hello/bin"
cp "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-appd" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0ctl" \
    "$repo_root/target/app-runtime-aarch64/cardputerzero-app-runtime" \
    "$platform_payload/"
cp "$repo_root/appd/systemd/"* "$platform_payload/systemd/"
cp "$repo_root/target/apps/dev.cardputerzero.hello/0.1.0/app.json" \
    "$platform_payload/hello/"
cp "$repo_root/target/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm" \
    "$platform_payload/hello/bin/"
# The stage1 user already exists. Installing userconf-pi after sizing the
# image adds a large Raspberry Pi utility dependency set and can fill rootfs.
touch "$pi_gen_dir/export-image/01-user-rename/SKIP"

config_file=$(mktemp "${TMPDIR:-/tmp}/cp0-pigen-config.XXXXXX")
cleanup() {
    rm -f "$config_file"
}
trap cleanup EXIT

cp "$repo_root/image/pi-gen/config.example" "$config_file"
if [[ -n "$image_name" ]]; then
    if [[ ! "$image_name" =~ ^[a-zA-Z0-9._-]+$ ]]; then
        echo "error: CP0_IMAGE_NAME contains unsupported characters" >&2
        exit 2
    fi
    sed -i.bak "s/^IMG_NAME=.*/IMG_NAME=$image_name/" "$config_file"
    rm -f "${config_file}.bak"
fi
printf 'FIRST_USER_PASS=%q\n' "$password" >>"$config_file"

if [[ -n ${CP0_SSH_PUBLIC_KEY:-} ]]; then
    printf 'PUBKEY_SSH_FIRST_USER=%q\n' "$CP0_SSH_PUBLIC_KEY" >>"$config_file"
    printf 'PUBKEY_ONLY_SSH=1\n' >>"$config_file"
fi

printf 'STAGE_LIST=%q\n' \
    "/pi-gen/stage0 /pi-gen/stage1 /pi-gen/stage-cardputerzero-os" \
    >>"$config_file"
printf 'CONTAINER_NAME=%q\n' "$container_name" >>"$config_file"
if [[ -n "$apt_proxy" ]]; then
    printf 'APT_PROXY=%q\n' "$apt_proxy" >>"$config_file"
fi

if [[ $(uname -s) == Linux && ${CP0_USE_DOCKER:-1} == 0 ]]; then
    sed -i \
        "s|^STAGE_LIST=.*|STAGE_LIST=$(printf %q "$pi_gen_dir/stage0 $pi_gen_dir/stage1 $pi_gen_dir/stage-cardputerzero-os")|" \
        "$config_file"
    echo "Building CardputerZero OS natively with pi-gen $PI_GEN_COMMIT"
    "$pi_gen_dir/build.sh" -c "$config_file"
    exit 0
fi

command -v docker >/dev/null
docker info >/dev/null
container_exists=0
if docker container inspect "$container_name" >/dev/null 2>&1; then
    container_exists=1
fi
if ((container_exists == 1)) && [[ "$resume_build" != 1 ]]; then
    echo "error: Docker container $container_name already exists" >&2
    echo "retry with CP0_RESUME_BUILD=1 or remove it after preserving logs" >&2
    exit 1
fi
if ((container_exists == 0)) && [[ "$resume_build" == 1 ]]; then
    echo "error: no failed Docker build named $container_name to resume" >&2
    exit 1
fi

echo "Building CardputerZero OS in Docker with pi-gen $PI_GEN_COMMIT"
docker build --build-arg BASE_IMAGE=debian:trixie \
    -t "$container_image" "$pi_gen_dir"

run_container_name=$container_name
docker_run_args=(--name "$run_container_name")
if [[ "$resume_build" == 1 ]]; then
    run_container_name="${container_name}-continue"
    if docker container inspect "$run_container_name" >/dev/null 2>&1; then
        echo "error: previous continuation container $run_container_name exists" >&2
        exit 1
    fi
    docker_run_args=(
        --name "$run_container_name"
        --volumes-from "$container_name"
    )
    echo "Resuming build from volumes owned by $container_name"
fi

set +e
docker run "${docker_run_args[@]}" --privileged \
    --volume "$config_file:/config:ro" \
    -e "GIT_HASH=$PI_GEN_COMMIT" \
    "$container_image" \
    bash -e -o pipefail -c \
    'dpkg-reconfigure qemu-user-binfmt &&
     (mount binfmt_misc -t binfmt_misc /proc/sys/fs/binfmt_misc || true) &&
     cd /pi-gen && ./build.sh -c /config &&
     rsync -av work/*/build.log deploy/'
build_status=$?
set -e

mkdir -p "$deploy_dir"
{
    docker logs --timestamps "$container_name" 2>&1 || true
    if [[ "$run_container_name" != "$container_name" ]]; then
        docker logs --timestamps "$run_container_name" 2>&1 || true
    fi
} >"$deploy_dir/build-docker.log"
docker cp "$run_container_name:/pi-gen/deploy/." "$deploy_dir/" || true

if ((build_status != 0)); then
    echo "error: image build failed; build containers were preserved" >&2
    exit "$build_status"
fi

if [[ ${CP0_KEEP_BUILD_CONTAINER:-0} != 1 ]]; then
    docker rm -v "$run_container_name" >/dev/null
    if [[ "$run_container_name" != "$container_name" ]]; then
        docker rm -v "$container_name" >/dev/null
    fi
fi

(
    cd "$deploy_dir"
    shasum -a 256 -- *.img.xz >SHA256SUMS
)
ls -lh "$deploy_dir"
