#!/bin/sh
set -eu

ssh_dir=/etc/ssh

if [ ! -d "$ssh_dir" ] || [ -L "$ssh_dir" ]; then
    echo "cardputerzero-ssh: persistent SSH directory is invalid" >&2
    exit 1
fi

for key_file in \
    ssh_host_rsa_key ssh_host_rsa_key.pub \
    ssh_host_ecdsa_key ssh_host_ecdsa_key.pub \
    ssh_host_ed25519_key ssh_host_ed25519_key.pub; do
    if [ -L "$ssh_dir/$key_file" ]; then
        echo "cardputerzero-ssh: refusing symbolic host key: $key_file" >&2
        exit 1
    fi
done

umask 077
/usr/bin/ssh-keygen -A

if [ ! -s "$ssh_dir/ssh_host_ed25519_key" ] ||
   [ ! -s "$ssh_dir/ssh_host_ed25519_key.pub" ]; then
    echo "cardputerzero-ssh: ED25519 host identity was not generated" >&2
    exit 1
fi

/usr/sbin/sshd -t
