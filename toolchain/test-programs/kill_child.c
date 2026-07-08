/* kill_child.c -- spawn long-running child, kill it, verify it terminated */
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <time.h>

#include "posix_spawn_compat.h"

extern char **environ;

int main(void) {
    /* Spawn 'sleep 999' -- long-running child we'll kill */
    char *argv[] = {"sleep", "999", NULL};
    pid_t child;
    int err = posix_spawnp(&child, "sleep", NULL, NULL, argv, environ);
    if (err != 0) {
        fprintf(stderr, "posix_spawn failed: %d\n", err);
        return 1;
    }
    printf("spawned: yes\n");

    /* Brief pause to let child start */
    struct timespec ts = {0, 50000000}; /* 50ms */
    nanosleep(&ts, NULL);

    /* Kill the child */
    if (kill(child, SIGTERM) != 0) {
        perror("kill");
        return 1;
    }
    printf("kill: ok\n");

    /* Wait for child to terminate */
    int status;
    pid_t w = waitpid(child, &status, 0);
    if (w < 0) {
        perror("waitpid");
        return 1;
    }
    printf("terminated: yes\n");
    printf("signaled=%s\n", WIFSIGNALED(status) ? "yes" : "no");
    if (WIFSIGNALED(status)) {
        printf("termsig=%d\n", WTERMSIG(status));
    }

    return 0;
}
