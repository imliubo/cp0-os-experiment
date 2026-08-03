#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
    echo "usage: $0 DEVICE_IP CP0_PROVISIOND SYSTEM_SHELL" >&2
    exit 2
fi

device_ip=$1
provisiond=$2
system_shell=$3
for artifact in "$provisiond" "$system_shell"; do
    [ -f "$artifact" ] && [ ! -L "$artifact" ] || {
        echo "error: artifact is not a regular file: $artifact" >&2
        exit 1
    }
done

run_ssh() {
    if [ -n "${CP0_SSH_IDENTITY:-}" ]; then
        ssh -i "$CP0_SSH_IDENTITY" -o BatchMode=yes \
            -o StrictHostKeyChecking=accept-new "$@"
    else
        ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new "$@"
    fi
}

run_scp() {
    if [ -n "${CP0_SSH_IDENTITY:-}" ]; then
        scp -i "$CP0_SSH_IDENTITY" -o BatchMode=yes \
            -o StrictHostKeyChecking=accept-new "$@"
    else
        scp -o BatchMode=yes -o StrictHostKeyChecking=accept-new "$@"
    fi
}

stage=/run/cp0-hot-update
run_ssh "root@$device_ip" \
    "install -d -o root -g root -m 0700 $stage"
run_scp "$provisiond" \
    "root@$device_ip:$stage/cp0-provisiond"
run_scp "$system_shell" \
    "root@$device_ip:$stage/cardputerzero-system-shell"
run_ssh "root@$device_ip" \
    "chmod 0700 $stage/cp0-provisiond $stage/cardputerzero-system-shell"
run_ssh "root@$device_ip" \
    /usr/libexec/cardputerzero/hot-update-firstboot.sh
