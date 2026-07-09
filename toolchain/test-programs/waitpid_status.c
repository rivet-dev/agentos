/* Focused Linux waitpid option and status regression. */
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <unistd.h>

#include "posix_spawn_compat.h"

extern char **environ;

static int fail(const char *step, pid_t result, int status) {
    fprintf(stderr, "%s: result=%d status=0x%x errno=%d\n",
            step, (int)result, status, errno);
    return 1;
}

int main(void) {
    pid_t child;
    char *argv[] = {"sh", "-c", "sleep 30", NULL};
    int error = posix_spawnp(&child, "sh", NULL, NULL, argv, environ);
    if (error != 0) {
        errno = error;
        return fail("spawn", -1, 0);
    }

    int status = 0;
    pid_t result = waitpid(child, &status, WNOHANG);
    if (result != 0)
        return fail("WNOHANG running child", result, status);

    errno = 0;
    result = waitpid(child, &status, 0x40000000);
    if (result != -1 || errno != EINVAL)
        return fail("unknown option", result, status);

    if (kill(child, SIGSTOP) != 0)
        return fail("SIGSTOP", -1, status);
    result = waitpid(child, &status, WUNTRACED);
    if (result != child || !WIFSTOPPED(status) || WSTOPSIG(status) != SIGSTOP)
        return fail("WUNTRACED", result, status);

    result = waitpid(child, &status, WNOHANG | WUNTRACED);
    if (result != 0)
        return fail("consumed stop event", result, status);

    if (kill(child, SIGCONT) != 0)
        return fail("SIGCONT", -1, status);
    result = waitpid(child, &status, WCONTINUED);
    if (result != child || !WIFCONTINUED(status))
        return fail("WCONTINUED", result, status);

    if (kill(child, SIGTERM) != 0)
        return fail("SIGTERM", -1, status);
    result = waitpid(child, &status, 0);
    if (result != child || !WIFSIGNALED(status) || WTERMSIG(status) != SIGTERM)
        return fail("signal exit", result, status);

    puts("waitpid-status-ok");
    return 0;
}
