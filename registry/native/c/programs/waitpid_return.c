/* waitpid_return.c -- verify waitpid returns the correct child PID */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "posix_spawn_compat.h"

extern char **environ;

int main(void) {
    /* Test 1: waitpid with specific PID returns that PID */
    pid_t child1;
    char *argv1[] = {"true", NULL};
    int err = posix_spawnp(&child1, "true", NULL, NULL, argv1, environ);
    if (err != 0) {
        fprintf(stderr, "spawn child1 failed: %d\n", err);
        return 1;
    }

    int status1;
    pid_t ret1 = waitpid(child1, &status1, 0);
    printf("test1_spawn_pid: %d\n", (int)child1);
    printf("test1_waitpid_ret: %d\n", (int)ret1);
    printf("test1_match: %s\n", (ret1 == child1) ? "yes" : "no");
    printf("test1_exit: %d\n", WIFEXITED(status1) ? WEXITSTATUS(status1) : -1);

    /* Test 2: wait() (waitpid(-1)) returns the actual child PID */
    pid_t child2;
    char *argv2[] = {"true", NULL};
    err = posix_spawnp(&child2, "true", NULL, NULL, argv2, environ);
    if (err != 0) {
        fprintf(stderr, "spawn child2 failed: %d\n", err);
        return 1;
    }

    int status2;
    pid_t ret2 = wait(&status2);
    printf("test2_spawn_pid: %d\n", (int)child2);
    printf("test2_wait_ret: %d\n", (int)ret2);
    printf("test2_match: %s\n", (ret2 == child2) ? "yes" : "no");
    printf("test2_exit: %d\n", WIFEXITED(status2) ? WEXITSTATUS(status2) : -1);

    /* Test 3: waitpid returns > 0 (not -1 or 0) */
    printf("test3_ret1_positive: %s\n", (ret1 > 0) ? "yes" : "no");
    printf("test3_ret2_positive: %s\n", (ret2 > 0) ? "yes" : "no");

    return 0;
}
