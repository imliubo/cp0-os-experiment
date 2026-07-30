#include "seccomp.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/ptrace.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <unistd.h>

static int expect_denied(const char *operation, long result) {
    if (result == -1 && errno == EPERM)
        return 0;
    fprintf(stderr, "seccomp-probe: %s was not denied with EPERM\n", operation);
    return -1;
}

int main(void) {
    int failed = 0;
    int unix_socket;

    if (cp0_install_runtime_seccomp() != 0) {
        fprintf(stderr, "seccomp-probe: cannot install filter\n");
        return 1;
    }
    errno = 0;
    failed |= expect_denied(
        "openat", syscall(SYS_openat, AT_FDCWD, "/etc/passwd", O_RDONLY, 0));
    errno = 0;
    failed |= expect_denied("socket", syscall(SYS_socket, AF_INET, SOCK_STREAM, 0));
    unix_socket = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (unix_socket < 0) {
        fprintf(stderr, "seccomp-probe: AF_UNIX socket was denied\n");
        failed = 1;
    } else {
        close(unix_socket);
    }
    errno = 0;
    failed |= expect_denied(
        "mount", syscall(SYS_mount, "none", "/tmp", "tmpfs", 0, NULL));
    errno = 0;
    failed |= expect_denied("ptrace", syscall(SYS_ptrace, PTRACE_TRACEME, 0, 0));

    if (failed != 0)
        return 1;
    puts("seccomp-probe: forbidden syscall checks passed");
    return 0;
}
