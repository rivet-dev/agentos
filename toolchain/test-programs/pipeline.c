/* pipeline.c -- pipe + spawn: 'echo hello | cat' via posix_spawn + fd_pipe */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "posix_spawn_compat.h"

extern char **environ;

int main(void) {
    /* p: echo stdout -> cat stdin */
    int p[2];
    if (pipe(p) != 0) { perror("pipe1"); return 1; }

    /* rp: cat stdout -> parent */
    int rp[2];
    if (pipe(rp) != 0) { perror("pipe2"); return 1; }

    /* Spawn echo with stdout -> p[1] */
    posix_spawn_file_actions_t fa1;
    posix_spawn_file_actions_init(&fa1);
    posix_spawn_file_actions_adddup2(&fa1, p[1], STDOUT_FILENO);
    posix_spawn_file_actions_addclose(&fa1, p[0]);
    posix_spawn_file_actions_addclose(&fa1, p[1]);
    posix_spawn_file_actions_addclose(&fa1, rp[0]);
    posix_spawn_file_actions_addclose(&fa1, rp[1]);

    char *echo_argv[] = {"echo", "hello", NULL};
    pid_t echo_pid;
    int err = posix_spawnp(&echo_pid, "echo", &fa1, NULL, echo_argv, environ);
    posix_spawn_file_actions_destroy(&fa1);
    if (err != 0) {
        fprintf(stderr, "spawn echo failed: %d\n", err);
        return 1;
    }

    /* Spawn cat with stdin <- p[0], stdout -> rp[1] */
    posix_spawn_file_actions_t fa2;
    posix_spawn_file_actions_init(&fa2);
    posix_spawn_file_actions_adddup2(&fa2, p[0], STDIN_FILENO);
    posix_spawn_file_actions_adddup2(&fa2, rp[1], STDOUT_FILENO);
    posix_spawn_file_actions_addclose(&fa2, p[0]);
    posix_spawn_file_actions_addclose(&fa2, p[1]);
    posix_spawn_file_actions_addclose(&fa2, rp[0]);
    posix_spawn_file_actions_addclose(&fa2, rp[1]);

    char *cat_argv[] = {"cat", NULL};
    pid_t cat_pid;
    err = posix_spawnp(&cat_pid, "cat", &fa2, NULL, cat_argv, environ);
    posix_spawn_file_actions_destroy(&fa2);
    if (err != 0) {
        fprintf(stderr, "spawn cat failed: %d\n", err);
        return 1;
    }

    /* Close pipe ends in parent */
    close(p[0]);
    close(p[1]);
    close(rp[1]);

    /* Read cat's output */
    char buf[256];
    ssize_t n = read(rp[0], buf, sizeof(buf) - 1);
    close(rp[0]);

    /* Wait for both children */
    int st1, st2;
    waitpid(echo_pid, &st1, 0);
    waitpid(cat_pid, &st2, 0);

    if (n > 0) {
        buf[n] = '\0';
        printf("pipeline_output: %s", buf);
    } else {
        printf("pipeline_output: (empty)\n");
    }
    printf("echo_exit: %d\n", WIFEXITED(st1) ? WEXITSTATUS(st1) : -1);
    printf("cat_exit: %d\n", WIFEXITED(st2) ? WEXITSTATUS(st2) : -1);

    return 0;
}
