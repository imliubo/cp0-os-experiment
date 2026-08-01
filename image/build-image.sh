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
image_profile=${CP0_IMAGE_PROFILE:-product}
access_profile=${CP0_ACCESS_PROFILE:-development}

case "$image_profile" in
    product | recovery) ;;
    *)
        echo "error: CP0_IMAGE_PROFILE must be product or recovery" >&2
        exit 2
        ;;
esac

case "$access_profile" in
    development | production) ;;
    *)
        echo "error: CP0_ACCESS_PROFILE must be development or production" >&2
        exit 2
        ;;
esac
if [[ $image_profile == recovery && $access_profile != development ]]; then
    echo "error: recovery images require the development access profile" >&2
    exit 2
fi

if [[ $(uname -s) == Darwin && -n "$apt_proxy" ]]; then
    apt_proxy=${apt_proxy/127.0.0.1/host.docker.internal}
    apt_proxy=${apt_proxy/localhost/host.docker.internal}
fi

if [[ $access_profile == production ]]; then
    if [[ -n $password ]]; then
        echo "error: production images reject CP0_FIRST_USER_PASSWORD" >&2
        exit 2
    fi
    if [[ -n ${CP0_SSH_PUBLIC_KEY:-} ]]; then
        echo "error: production images reject CP0_SSH_PUBLIC_KEY" >&2
        exit 2
    fi
    command -v openssl >/dev/null
    password=$(openssl rand -hex 32)
elif [[ -z $password ]]; then
    echo "error: CP0_FIRST_USER_PASSWORD is required for the development access profile" >&2
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
install -m 0755 "$repo_root/image/pi-gen/export-image-prerun.sh" \
    "$pi_gen_dir/export-image/prerun.sh"

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
    "$pi_gen_dir/stage0/00-configure-apt/files/debian.sources" \
    "$pi_gen_dir/stage0/00-configure-apt/files/raspi.sources"; do
    sed -i.bak \
        -e 's|http://deb.debian.org|https://deb.debian.org|g' \
        -e 's|http://archive.raspberrypi.com|https://archive.raspberrypi.com|g' \
        "$apt_source"
    rm -f "${apt_source}.bak"
done
printf '%s\n' \
    'Acquire::http { Proxy "APT_PROXY"; };' \
    'Acquire::https { Proxy "APT_PROXY"; };' \
    >"$pi_gen_dir/stage0/00-configure-apt/files/51cache"

rm -rf "$pi_gen_dir/stage-cardputerzero-os"
cp -R "$repo_root/image/pi-gen/stage-cardputerzero-os" \
    "$pi_gen_dir/stage-cardputerzero-os"
printf '%s\n' "$image_profile" \
    >"$pi_gen_dir/stage-cardputerzero-os/image-profile"
printf '%s\n' "$access_profile" \
    >"$pi_gen_dir/stage-cardputerzero-os/access-profile"
mkdir -p "$pi_gen_dir/stage-cardputerzero-os/01-compositor/system-shell"
cp "$repo_root/system-shell/include/cp0_ui.h" \
    "$repo_root/system-shell/include/cp0_json.h" \
    "$repo_root/system-shell/include/cp0_appd_client.h" \
    "$repo_root/system-shell/include/cp0_screenshot_store.h" \
    "$repo_root/system-shell/include/cp0_store_client.h" \
    "$repo_root/system-shell/include/cp0_system_info.h" \
    "$repo_root/system-shell/src/ui.c" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/appd_client.c" \
    "$repo_root/system-shell/src/screenshot_store.c" \
    "$repo_root/system-shell/src/store_client.c" \
    "$repo_root/system-shell/src/system_info.c" \
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
mkdir -p "$platform_payload/systemd" "$platform_payload/hello/bin" \
    "$platform_payload/trust/store" \
    "$platform_payload/diagnostics"
cp "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-appd" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-audiod" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-camerad" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-documentd" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-gpiod" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-networkd" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-radiod" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-recovery" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-storaged" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0-stored" \
    "$repo_root/target/aarch64-unknown-linux-gnu/release/cp0ctl" \
    "$repo_root/target/app-runtime-aarch64/cardputerzero-app-runtime" \
    "$platform_payload/"
cp "$repo_root/appd/systemd/"* "$platform_payload/systemd/"
cp "$repo_root/appd/lora.conf" "$platform_payload/"
cp "$repo_root/appd/store.conf" "$platform_payload/"
cp "$repo_root/appd/device-policy.json" "$platform_payload/"
cp "$repo_root/appd/device-policy-production.json" "$platform_payload/"
if [[ -n ${CP0_STORE_PUBLIC_KEY:-} ]]; then
    if [[ $image_profile == recovery ]]; then
        echo "error: a recovery image cannot embed a Store trust key" >&2
        exit 2
    fi
    if [[ ! -f $CP0_STORE_PUBLIC_KEY ]] || [[ $(wc -c <"$CP0_STORE_PUBLIC_KEY") -ne 32 ]]; then
        echo "error: CP0_STORE_PUBLIC_KEY must name a 32-byte raw Ed25519 public key" >&2
        exit 1
    fi
    store_key_id=$(shasum -a 256 "$CP0_STORE_PUBLIC_KEY" | awk '{print $1}')
    cp "$CP0_STORE_PUBLIC_KEY" "$platform_payload/trust/store/$store_key_id.pub"
fi
cp "$repo_root/scripts/device-core-recovery.sh" \
    "$repo_root/scripts/device-capability-acceptance.sh" \
    "$repo_root/scripts/device-factory-acceptance.sh" \
    "$repo_root/scripts/device-performance-acceptance.sh" \
    "$repo_root/scripts/device-recovery-data.sh" \
    "$repo_root/scripts/device-stability-monitor.sh" \
    "$repo_root/scripts/device-store-acceptance.sh" \
    "$repo_root/scripts/device-support-bundle.sh" \
    "$platform_payload/diagnostics/"
cp "$repo_root/target/apps/dev.cardputerzero.hello/0.1.0/app.json" \
    "$platform_payload/hello/"
cp "$repo_root/target/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm" \
    "$platform_payload/hello/bin/"
# The stage1 user already exists. Installing userconf-pi after sizing the
# image adds a large Raspberry Pi utility dependency set and can fill rootfs.
touch "$pi_gen_dir/export-image/01-user-rename/SKIP"

# Upstream finalise generates the initramfs and then unmounts and compresses the
# image in the same substage. Inject the release gate immediately before that
# unmount; a later numbered substage would only see an empty mount point.
finalise_script="$pi_gen_dir/export-image/05-finalise/01-run.sh"
rootfs_verifier="$pi_gen_dir/export-image/cardputerzero-verify-rootfs.sh"
install -m 0755 "$repo_root/tests/test-built-rootfs-profile.sh" \
    "$rootfs_verifier"
rm -rf "$pi_gen_dir/export-image/06-cardputerzero-verify"
if [[ $(grep -c '^ROOT_DEV=' "$finalise_script") -ne 1 ]]; then
    echo "error: pi-gen finalise unmount marker changed" >&2
    exit 1
fi
if ! grep -Fq 'cardputerzero-verify-rootfs.sh' "$finalise_script"; then
    sed -i.bak \
        '/^ROOT_DEV=/i\
"${EXPORT_CONFIG_DIR}/cardputerzero-verify-rootfs.sh" "${ROOTFS_DIR}"\
' "$finalise_script"
    rm -f "${finalise_script}.bak"
fi
grep -Fqx \
    '"${EXPORT_CONFIG_DIR}/cardputerzero-verify-rootfs.sh" "${ROOTFS_DIR}"' \
    "$finalise_script"

mkdir -p "$repo_root/target/image-build"
config_file=$(mktemp "$repo_root/target/image-build/cp0-pigen-config.XXXXXX")
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
