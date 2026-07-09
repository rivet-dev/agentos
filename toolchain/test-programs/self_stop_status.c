/* Focused Linux self-stop and parent-continue regression. */
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include "posix_spawn_compat.h"

extern char **environ;

static int fail(const char *step, pid_t result, int status) {
    fprintf(stderr, "%s: result=%d status=0x%x errno=%d\n",
            step, (int)result, status, errno);
    return 1;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "child") == 0) {
        puts("child-before-stop");
        fflush(stdout);
        if (kill(getpid(), SIGSTOP) != 0)
            return fail("self SIGSTOP", -1, 0);
        puts("child-after-continue");
        return 0;
    }

    pid_t child;
    char *child_argv[] = {"self_stop_status", "child", NULL};
    int error = posix_spawnp(&child, child_argv[0], NULL, NULL, child_argv, environ);
    if (error != 0) {
        errno = error;
        return fail("spawn", -1, 0);
    }

    int status = 0;
    pid_t result = waitpid(child, &status, WUNTRACED);
    if (result != child || !WIFSTOPPED(status) || WSTOPSIG(status) != SIGSTOP)
        return fail("wait stopped", result, status);

    result = waitpid(child, &status, WNOHANG | WUNTRACED);
    if (result != 0)
        return fail("stopped child advanced", result, status);

    if (kill(child, SIGCONT) != 0)
        return fail("parent SIGCONT", -1, status);
    result = waitpid(child, &status, WCONTINUED);
    if (result != child || !WIFCONTINUED(status))
        return fail("wait continued", result, status);

    result = waitpid(child, &status, 0);
    if (result != child || !WIFEXITED(status) || WEXITSTATUS(status) != 0)
        return fail("wait exit", result, status);

    puts("self-stop-ok");
    return 0;
}
