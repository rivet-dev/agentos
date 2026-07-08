/* waitpid_edge.c -- edge case tests for waitpid with concurrent children and invalid PIDs */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>

#include "posix_spawn_compat.h"

extern char **environ;

int main(void) {
    /* Warmup piped spawn (first piped spawn has a capture quirk) */
    {
        pid_t wp;
        char *wargv[] = {"true", NULL};
        posix_spawnp(&wp, "true", NULL, NULL, wargv, environ);
        waitpid(wp, NULL, 0);
    }

    /* Test 1: spawn 3 children with different exit codes, waitpid each by specific PID */
    {
        pid_t c1, c2, c3;
        char *a1[] = {"sh", "-c", "exit 1", NULL};
        char *a2[] = {"sh", "-c", "exit 2", NULL};
        char *a3[] = {"sh", "-c", "exit 3", NULL};
        int err;

        err = posix_spawnp(&c1, "sh", NULL, NULL, a1, environ);
        if (err != 0) { printf("test1: FAIL (spawn c1 err=%d)\n", err); return 1; }

        err = posix_spawnp(&c2, "sh", NULL, NULL, a2, environ);
        if (err != 0) { printf("test1: FAIL (spawn c2 err=%d)\n", err); return 1; }

        err = posix_spawnp(&c3, "sh", NULL, NULL, a3, environ);
        if (err != 0) { printf("test1: FAIL (spawn c3 err=%d)\n", err); return 1; }

        int s1, s2, s3;
        pid_t r1 = waitpid(c1, &s1, 0);
        pid_t r2 = waitpid(c2, &s2, 0);
        pid_t r3 = waitpid(c3, &s3, 0);

        int e1 = WIFEXITED(s1) ? WEXITSTATUS(s1) : -1;
        int e2 = WIFEXITED(s2) ? WEXITSTATUS(s2) : -1;
        int e3 = WIFEXITED(s3) ? WEXITSTATUS(s3) : -1;

        int ok = (r1 == c1 && e1 == 1 &&
                  r2 == c2 && e2 == 2 &&
                  r3 == c3 && e3 == 3);
        printf("test1_c1_exit: %d\n", e1);
        printf("test1_c2_exit: %d\n", e2);
        printf("test1_c3_exit: %d\n", e3);
        printf("test1: %s\n", ok ? "ok" : "FAIL");
    }

    /* Test 2: spawn 2 children, use wait() (waitpid -1) twice, verify both reaped */
    {
        pid_t c1, c2;
        char *a1[] = {"true", NULL};
        char *a2[] = {"true", NULL};
        int err;

        err = posix_spawnp(&c1, "true", NULL, NULL, a1, environ);
        if (err != 0) { printf("test2: FAIL (spawn c1 err=%d)\n", err); return 1; }

        err = posix_spawnp(&c2, "true", NULL, NULL, a2, environ);
        if (err != 0) { printf("test2: FAIL (spawn c2 err=%d)\n", err); return 1; }

        int s1, s2;
        pid_t r1 = wait(&s1);
        pid_t r2 = wait(&s2);

        /* Both returned PIDs must be valid (> 0) and distinct */
        int valid = (r1 > 0 && r2 > 0 && r1 != r2);
        /* Both must be one of c1 or c2 */
        int known = ((r1 == c1 || r1 == c2) && (r2 == c1 || r2 == c2));
        /* Both must have exited successfully */
        int exited = (WIFEXITED(s1) && WEXITSTATUS(s1) == 0 &&
                      WIFEXITED(s2) && WEXITSTATUS(s2) == 0);

        printf("test2_r1_valid: %s\n", (r1 > 0) ? "yes" : "no");
        printf("test2_r2_valid: %s\n", (r2 > 0) ? "yes" : "no");
        printf("test2_distinct: %s\n", (r1 != r2) ? "yes" : "no");
        printf("test2: %s\n", (valid && known && exited) ? "ok" : "FAIL");
    }

    /* Test 3: waitpid with PID that was never spawned, verify returns -1 */
    {
        errno = 0;
        int status;
        pid_t ret = waitpid(99999, &status, 0);
        int err = errno;
        /* POSIX: returns -1, errno = ECHILD (or ESRCH in some implementations) */
        int failed = (ret == -1 && err != 0);
        printf("test3_ret: %d\n", (int)ret);
        printf("test3_failed: %s\n", failed ? "yes" : "no");
        printf("test3: %s\n", failed ? "ok" : "FAIL");
    }

    return 0;
}
