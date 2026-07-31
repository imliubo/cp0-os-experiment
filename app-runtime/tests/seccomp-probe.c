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

static int expect_open_denied(const char *path) {
    errno = 0;
    return expect_denied(path,
                         syscall(SYS_openat, AT_FDCWD, path, O_RDONLY, 0));
}

int main(void) {
    int failed = 0;
    int unix_socket;
    int pair[2];
    static const char *forbidden_paths[] = {
        "/etc/passwd",
        "/proc/self/root/etc/passwd",
        "/dev/dri/card0",
        "/dev/input/event0",
        "/dev/gpiochip0",
        "/dev/snd/controlC0",
    };

    if (cp0_install_runtime_seccomp() != 0) {
        fprintf(stderr, "seccomp-probe: cannot install filter\n");
        return 1;
    }
    for (size_t index = 0;
         index < sizeof(forbidden_paths) / sizeof(forbidden_paths[0]); index++)
        failed |= expect_open_denied(forbidden_paths[index]);
    errno = 0;
    failed |= expect_denied("socket", syscall(SYS_socket, AF_INET, SOCK_STREAM, 0));
    errno = 0;
    failed |= expect_denied("socket-ipv6",
                            syscall(SYS_socket, AF_INET6, SOCK_STREAM, 0));
    errno = 0;
    failed |= expect_denied("socket-netlink",
                            syscall(SYS_socket, AF_NETLINK, SOCK_RAW, 0));
    unix_socket = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (unix_socket < 0) {
        fprintf(stderr, "seccomp-probe: AF_UNIX socket was denied\n");
        failed = 1;
    } else {
        close(unix_socket);
    }
    errno = 0;
    failed |= expect_denied(
        "socketpair", syscall(SYS_socketpair, AF_UNIX, SOCK_STREAM, 0, pair));
    errno = 0;
    failed |= expect_denied(
        "mount", syscall(SYS_mount, "none", "/tmp", "tmpfs", 0, NULL));
    errno = 0;
    failed |= expect_denied("ptrace", syscall(SYS_ptrace, PTRACE_TRACEME, 0, 0));
    errno = 0;
    failed |= expect_denied("clone", syscall(SYS_clone, 0, 0, 0, 0, 0));
    errno = 0;
    failed |= expect_denied("execve",
                            syscall(SYS_execve, "/bin/true", NULL, NULL));
    errno = 0;
    failed |= expect_denied("kill", syscall(SYS_kill, getpid(), 0));

    if (failed != 0)
        return 1;
    puts("seccomp-probe: forbidden syscall checks passed");
    return 0;
}
