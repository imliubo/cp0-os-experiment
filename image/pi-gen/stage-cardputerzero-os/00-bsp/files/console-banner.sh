#!/bin/sh
set -u

display=FAIL
keyboard=FAIL

if find /sys/class/drm -maxdepth 1 -type l -name 'card*-SPI-*' 2>/dev/null |
    grep -q .; then
    display='OK 320x170'
fi

if grep -q 'Name="tca8418c"' /proc/bus/input/devices 2>/dev/null; then
    keyboard=OK
fi

ipv4=$(ip -4 -o address show up scope global 2>/dev/null |
    awk '{ split($4, address, "/"); print address[1] }' |
    paste -sd ' ' -)
if [ -z "$ipv4" ]; then
    ipv4='not assigned'
fi

printf '\nCardputerZero OS DEV\n'
printf 'Boot:     READY\n'
printf 'LCD:      %s\n' "$display"
printf 'Keyboard: %s\n' "$keyboard"
printf 'IPv4:     %s\n' "$ipv4"
printf 'Login:    pi\n\n'
