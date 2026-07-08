/* spawn_child.c -- posix_spawn 'echo hello', waitpid, print child stdout */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "posix_spawn_compat.h"

extern char **environ;

int main(void) {
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        perror("pipe");
        return 1;
    }

    /* Redirect child stdout to pipe write end */
    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, pipefd[1], STDOUT_FILENO);
    posix_spawn_file_actions_addclose(&fa, pipefd[0]);
    posix_spawn_file_actions_addclose(&fa, pipefd[1]);

    char *argv[] = {"echo", "hello", NULL};
    pid_t child;
    int err = posix_spawnp(&child, "echo", &fa, NULL, argv, environ);
    posix_spawn_file_actions_destroy(&fa);

    if (err != 0) {
        fprintf(stderr, "posix_spawn failed: %d\n", err);
        return 1;
    }

    close(pipefd[1]);

    char buf[256];
    ssize_t n = read(pipefd[0], buf, sizeof(buf) - 1);
    close(pipefd[0]);

    int status;
    waitpid(child, &status, 0);

    if (n > 0) {
        buf[n] = '\0';
        printf("child_stdout: %s", buf);
    } else {
        printf("child_stdout: (empty)\n");
    }
    printf("child_exit: %d\n", WIFEXITED(status) ? WEXITSTATUS(status) : -1);

    return 0;
}
