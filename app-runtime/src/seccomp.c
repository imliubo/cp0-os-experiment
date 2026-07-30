#include "seccomp.h"

#include <errno.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <unistd.h>

#include <linux/audit.h>
#include <linux/filter.h>
#include <linux/seccomp.h>

#if !defined(__aarch64__)
#error "CardputerZero runtime seccomp is defined only for aarch64"
#endif

#define CP0_ALLOW_SYSCALL(name)                                                \
    BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_##name, 0, 1),                   \
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW)

int cp0_install_runtime_seccomp(void) {
    struct sock_filter filter[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS,
                 (uint32_t)offsetof(struct seccomp_data, arch)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH_AARCH64, 1, 0),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS,
                 (uint32_t)offsetof(struct seccomp_data, nr)),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, __NR_socket, 0, 4),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS,
                 (uint32_t)offsetof(struct seccomp_data, args[0])),
        BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, AF_UNIX, 0, 1),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
        BPF_STMT(BPF_RET | BPF_K,
                 SECCOMP_RET_ERRNO | (EPERM & SECCOMP_RET_DATA)),
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS,
                 (uint32_t)offsetof(struct seccomp_data, nr)),
        CP0_ALLOW_SYSCALL(read),
        CP0_ALLOW_SYSCALL(pread64),
        CP0_ALLOW_SYSCALL(write),
        CP0_ALLOW_SYSCALL(close),
        CP0_ALLOW_SYSCALL(connect),
        CP0_ALLOW_SYSCALL(setsockopt),
        CP0_ALLOW_SYSCALL(fstat),
        CP0_ALLOW_SYSCALL(newfstatat),
        CP0_ALLOW_SYSCALL(lseek),
        CP0_ALLOW_SYSCALL(brk),
        CP0_ALLOW_SYSCALL(mmap),
        CP0_ALLOW_SYSCALL(mprotect),
        CP0_ALLOW_SYSCALL(munmap),
        CP0_ALLOW_SYSCALL(madvise),
        CP0_ALLOW_SYSCALL(rt_sigaction),
        CP0_ALLOW_SYSCALL(rt_sigprocmask),
        CP0_ALLOW_SYSCALL(rt_sigreturn),
        CP0_ALLOW_SYSCALL(clock_gettime),
        CP0_ALLOW_SYSCALL(clock_nanosleep),
        CP0_ALLOW_SYSCALL(nanosleep),
        CP0_ALLOW_SYSCALL(futex),
        CP0_ALLOW_SYSCALL(sched_yield),
        CP0_ALLOW_SYSCALL(getpid),
        CP0_ALLOW_SYSCALL(gettid),
        CP0_ALLOW_SYSCALL(tgkill),
        CP0_ALLOW_SYSCALL(set_tid_address),
        CP0_ALLOW_SYSCALL(set_robust_list),
        CP0_ALLOW_SYSCALL(prlimit64),
        CP0_ALLOW_SYSCALL(getrandom),
        CP0_ALLOW_SYSCALL(rseq),
        CP0_ALLOW_SYSCALL(ppoll),
        CP0_ALLOW_SYSCALL(epoll_create1),
        CP0_ALLOW_SYSCALL(epoll_ctl),
        CP0_ALLOW_SYSCALL(epoll_pwait),
        CP0_ALLOW_SYSCALL(eventfd2),
        CP0_ALLOW_SYSCALL(recvmsg),
        CP0_ALLOW_SYSCALL(sendmsg),
        CP0_ALLOW_SYSCALL(recvfrom),
        CP0_ALLOW_SYSCALL(sendto),
        CP0_ALLOW_SYSCALL(ioctl),
        CP0_ALLOW_SYSCALL(memfd_create),
        CP0_ALLOW_SYSCALL(ftruncate),
        CP0_ALLOW_SYSCALL(fcntl),
        CP0_ALLOW_SYSCALL(exit),
        CP0_ALLOW_SYSCALL(exit_group),
        BPF_STMT(BPF_RET | BPF_K,
                 SECCOMP_RET_ERRNO | (EPERM & SECCOMP_RET_DATA)),
    };
    struct sock_fprog program = {
        .len = (unsigned short)(sizeof(filter) / sizeof(filter[0])),
        .filter = filter,
    };

    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0)
        return -1;
    return (int)syscall(SYS_seccomp, SECCOMP_SET_MODE_FILTER, 0, &program);
}
