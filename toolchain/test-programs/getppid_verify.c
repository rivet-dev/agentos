/* getppid_verify.c -- verify child's getppid() matches parent's getpid()
 *
 * Parent calls getpid(), spawns getppid_test with piped stdout, captures
 * child output, verifies child's ppid matches parent's pid.
 *
 * Note: first piped spawn in a WASM process has a pipe-capture quirk,
 * so we do a warmup piped spawn of 'true' first. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "posix_spawn_compat.h"

extern char **environ;

/* Spawn a command with piped stdout, capture output into buf. */
static ssize_t spawn_capture(const char *cmd, char *const argv[],
                              char *buf, size_t bufsize, int *exit_code) {
    int pipefd[2];
    if (pipe(pipefd) != 0)
        return -1;

    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, pipefd[1], STDOUT_FILENO);
    posix_spawn_file_actions_addclose(&fa, pipefd[0]);
    posix_spawn_file_actions_addclose(&fa, pipefd[1]);

    pid_t child;
    int err = posix_spawnp(&child, cmd, &fa, NULL, argv, environ);
    posix_spawn_file_actions_destroy(&fa);

    if (err != 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return -1;
    }

    close(pipefd[1]);

    ssize_t total = 0;
    ssize_t n;
    while ((n = read(pipefd[0], buf + total, bufsize - 1 - total)) > 0)
        total += n;
    close(pipefd[0]);
    buf[total] = '\0';

    int status;
    waitpid(child, &status, 0);
    *exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;

    return total;
}

int main(void) {
    pid_t my_pid = getpid();
    printf("parent_pid=%d\n", my_pid);

    /* Warmup piped spawn — works around first-pipe-capture WASM quirk */
    {
        char buf[64];
        int ec;
        char *argv[] = {"true", NULL};
        spawn_capture("true", argv, buf, sizeof(buf), &ec);
    }

    /* Spawn getppid_test with piped stdout to capture child's ppid output */
    char buf[512];
    int child_exit;
    char *child_argv[] = {"getppid_test", NULL};
    ssize_t captured = spawn_capture("getppid_test", child_argv,
                                      buf, sizeof(buf), &child_exit);

    printf("child_exit=%d\n", child_exit);

    if (captured <= 0) {
        printf("child_output=none\n");
        printf("match=no\n");
        return 1;
    }

    /* Parse child's ppid=N from getppid_test output */
    int child_ppid = -1;
    char *line = strstr(buf, "ppid=");
    if (line)
        child_ppid = atoi(line + 5);

    printf("child_reported_ppid=%d\n", child_ppid);
    printf("match=%s\n", (child_ppid == (int)my_pid) ? "yes" : "no");

    return 0;
}
