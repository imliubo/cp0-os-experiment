#!/bin/sh
set -eu

stage=/run/cp0-hot-update
backup=/run/cp0-hot-update-backup
provisiond=$stage/cp0-provisiond
system_shell=$stage/cardputerzero-system-shell
installed_provisiond=/usr/libexec/cardputerzero/cp0-provisiond
installed_shell=/usr/bin/cardputerzero-system-shell

for artifact in "$provisiond" "$system_shell"; do
    [ -f "$artifact" ] && [ ! -L "$artifact" ] && [ -x "$artifact" ] || {
        echo "cardputerzero-hot-update: invalid artifact: $artifact" >&2
        exit 1
    }
done

install -d -o root -g root -m 0700 "$backup"
cp -p "$installed_provisiond" "$backup/cp0-provisiond"
cp -p "$installed_shell" "$backup/cardputerzero-system-shell"

rollback() {
    echo "cardputerzero-hot-update: activation failed; restoring previous binaries" >&2
    install -o root -g root -m 0755 \
        "$backup/cp0-provisiond" "$installed_provisiond"
    install -o root -g root -m 0755 \
        "$backup/cardputerzero-system-shell" "$installed_shell"
    systemctl restart cardputerzero-provisiond.socket
    systemctl restart cardputerzero-system-shell.service
}
trap rollback HUP INT TERM

systemctl stop cardputerzero-provisiond.service 2>/dev/null || true
install -o root -g root -m 0755 "$provisiond" "$installed_provisiond"
install -o root -g root -m 0755 "$system_shell" "$installed_shell"
if ! systemctl restart cardputerzero-provisiond.socket ||
   ! systemctl restart cardputerzero-system-shell.service; then
    rollback
    exit 1
fi
sleep 2
if ! systemctl is-active --quiet cardputerzero-provisiond.socket ||
   ! systemctl is-active --quiet cardputerzero-system-shell.service; then
    rollback
    exit 1
fi
trap - HUP INT TERM
rm -f "$provisiond" "$system_shell"
echo "cardputerzero-hot-update: first-boot services updated for this boot"
