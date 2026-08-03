#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! -f $1 ]]; then
    echo "usage: analyze-keyboard-diagnostics.sh keyboard-test.log" >&2
    exit 2
fi

awk -F, '
NR == 1 {
    if ($1 != "CP0K" || $2 != "1" || $3 !~ /^[0-9]+$/) {
        print "error: unsupported keyboard diagnostics log" > "/dev/stderr"
        exit 2
    }
    total = $3 + 0
    print "Keyboard diagnostics schema 1, tests " total
    next
}
$1 == "S" {
    prompt[$2] = $3
    expected_code[$2] = $4 + 0
    expected_shift[$2] = $5 + 0
    expected_ascii[$2] = $6
    next
}
$1 == "C" {
    step = $2 + 0
    captures++
    actual_code = $3 + 0
    actual_modifiers = $4 + 0
    actual_ascii = $5
    matched = $6 + 0
    if (!matched) {
        mismatches++
        actual_shift = actual_modifiers % 2
        printf "MISMATCH step %d: %s; expected code=%d shift=%d ascii=%s; received code=%d modifiers=%d ascii=%s\n", \
            step, prompt[step], expected_code[step], expected_shift[step], expected_ascii[step], \
            actual_code, actual_modifiers, actual_ascii
        if (actual_code != expected_code[step]) {
            code_errors++
        } else if (actual_shift != expected_shift[step] || actual_modifiers > 1) {
            shift_errors++
        } else if (actual_ascii != expected_ascii[step]) {
            ascii_errors++
        } else {
            other_errors++
        }
    }
    next
}
$1 == "K" { confirmed++ ; next }
$1 == "R" { retries++ ; next }
$1 == "D" {
    complete = 1
    done_confirmed = $2 + 0
    done_passed = $3 + 0
    done_attempts = $4 + 0
    truncated = $5 + 0
    next
}
$1 == "X" { runtime_errors++ }
END {
    if (NR == 0) {
        print "error: empty keyboard diagnostics log" > "/dev/stderr"
        exit 2
    }
    printf "Summary: captures=%d confirmed=%d retries=%d mismatches=%d\n", \
        captures, confirmed, retries, mismatches
    if (!complete) {
        printf "Diagnosis: test incomplete at confirmation %d of %d\n", confirmed, total
    } else {
        printf "Completion: confirmed=%d passed=%d attempts=%d truncated=%d\n", \
            done_confirmed, done_passed, done_attempts, truncated
    }
    if (code_errors) {
        printf "Diagnosis: %d key-code translation mismatch(es); inspect the keypad driver Sym table.\n", code_errors
    }
    if (shift_errors) {
        printf "Diagnosis: %d modifier-state mismatch(es); inspect physical/synthetic Shift press and release ordering.\n", shift_errors
    }
    if (ascii_errors) {
        printf "Diagnosis: %d userspace ASCII mapping mismatch(es); inspect the Runtime or text-input keymap.\n", ascii_errors
    }
    if (other_errors) {
        printf "Diagnosis: %d unclassified event mismatch(es).\n", other_errors
    }
    if (runtime_errors) {
        printf "Diagnosis: %d Runtime input polling error(s).\n", runtime_errors
    }
    if (complete && mismatches == 0 && !runtime_errors && !truncated) {
        print "Diagnosis: Runtime key events match all expected V0.6 inputs. Investigate the text renderer or consuming input widget."
    }
    if (truncated) {
        print "Diagnosis: log was truncated; repeat the test without unnecessary key presses."
    }
}
' "$1"
