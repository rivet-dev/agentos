#define _WASI_EMULATED_PROCESS_CLOCKS

#include <errno.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>

/*
 * POSIX exec and Linux execve semantics:
 * https://pubs.opengroup.org/onlinepubs/9799919799/functions/exec.html
 * https://man7.org/linux/man-pages/man2/execve.2.html
 *
 * This libc compatibility path uses the existing standard spawn/wait/exit
 * surface. It preserves execve's no-return behavior on success, but it does
 * not yet preserve the virtual PID. The R0 conformance gate must replace this
 * with the generated kernel exec syscall before declaring execve complete.
 */
int execve(const char *path, char *const argv[], char *const envp[]) {
    pid_t pid;
    int status;
    int error;

    if (path == NULL || argv == NULL) {
        errno = EFAULT;
        return -1;
    }

    error = posix_spawn(&pid, path, NULL, NULL, argv, envp);
    if (error != 0) {
        errno = error;
        return -1;
    }

    if (waitpid(pid, &status, 0) < 0)
        _exit(127);
    if (WIFEXITED(status))
        _exit(WEXITSTATUS(status));
    if (WIFSIGNALED(status))
        _exit(128 + WTERMSIG(status));
    _exit(127);
}
