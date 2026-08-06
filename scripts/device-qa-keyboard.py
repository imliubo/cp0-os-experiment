#!/usr/bin/env python3
"""Inject deterministic QA key presses into the CardputerZero Weston seat."""

import argparse
import fcntl
import os
import struct
import time


UI_SET_EVBIT = 0x40045564
UI_SET_KEYBIT = 0x40045565
UI_SET_MSCBIT = 0x40045568
UI_DEV_CREATE = 0x5501
UI_DEV_DESTROY = 0x5502
UI_DEV_SETUP = 0x405C5503

EV_SYN = 0
EV_KEY = 1
EV_MSC = 4
EV_REP = 20
MSC_SCAN = 4
BUS_USB = 3

# udev classifies an input device as a keyboard only when the complete
# alphanumeric block is advertised. Weston intentionally ignores a generic
# ID_INPUT_KEY device, even when the individual QA keys are present.
KEYBOARD_CLASSIFIER_KEYS = range(1, 59)

KEYS = {
    "esc": 1,
    "1": 2,
    "2": 3,
    "3": 4,
    "4": 5,
    "5": 6,
    "6": 7,
    "7": 8,
    "8": 9,
    "9": 10,
    "0": 11,
    "minus": 12,
    "backspace": 14,
    "q": 16,
    "w": 17,
    "e": 18,
    "r": 19,
    "t": 20,
    "y": 21,
    "u": 22,
    "i": 23,
    "o": 24,
    "p": 25,
    "enter": 28,
    "a": 30,
    "s": 31,
    "d": 32,
    "f": 33,
    "g": 34,
    "h": 35,
    "j": 36,
    "k": 37,
    "l": 38,
    "z": 44,
    "x": 45,
    "c": 46,
    "v": 47,
    "b": 48,
    "n": 49,
    "m": 50,
    "space": 57,
    "f1": 59,
    "f2": 60,
    "f3": 61,
    "f4": 62,
    "sysrq": 99,
    "up": 103,
    "left": 105,
    "right": 106,
    "down": 108,
    "mute": 113,
    "volume_down": 114,
    "volume_up": 115,
    "help": 138,
    "media_next": 163,
    "media_play_pause": 164,
    "media_previous": 165,
    "brightness_down": 224,
    "brightness_up": 225,
}


def emit(descriptor: int, event_type: int, code: int, value: int) -> None:
    os.write(descriptor, struct.pack("llHHi", 0, 0, event_type, code, value))


def press(descriptor: int, key: int, hold: float, gap: float) -> None:
    emit(descriptor, EV_KEY, key, 1)
    emit(descriptor, EV_SYN, 0, 0)
    time.sleep(hold)
    emit(descriptor, EV_KEY, key, 0)
    emit(descriptor, EV_SYN, 0, 0)
    time.sleep(gap)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("keys", nargs="+", choices=sorted(KEYS))
    parser.add_argument("--settle", type=float, default=1.5)
    parser.add_argument("--hold", type=float, default=0.12)
    parser.add_argument("--gap", type=float, default=0.30)
    arguments = parser.parse_args()
    if os.geteuid() != 0:
        parser.error("device QA keyboard must run as root")
    if min(arguments.settle, arguments.hold, arguments.gap) < 0:
        parser.error("timings must be non-negative")

    descriptor = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    try:
        for event_type in (EV_KEY, EV_MSC, EV_REP):
            fcntl.ioctl(descriptor, UI_SET_EVBIT, event_type)
        for key in sorted(set(KEYS.values()).union(KEYBOARD_CLASSIFIER_KEYS)):
            fcntl.ioctl(descriptor, UI_SET_KEYBIT, key)
        fcntl.ioctl(descriptor, UI_SET_MSCBIT, MSC_SCAN)
        setup = struct.pack(
            "HHHH80sI",
            BUS_USB,
            0x4350,
            0x3051,
            1,
            b"cp0-qa-seat-keyboard",
            0,
        )
        fcntl.ioctl(descriptor, UI_DEV_SETUP, setup)
        fcntl.ioctl(descriptor, UI_DEV_CREATE)
        time.sleep(arguments.settle)
        for name in arguments.keys:
            press(descriptor, KEYS[name], arguments.hold, arguments.gap)
        time.sleep(arguments.settle)
        fcntl.ioctl(descriptor, UI_DEV_DESTROY)
    finally:
        os.close(descriptor)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
