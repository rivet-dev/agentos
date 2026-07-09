#include <errno.h>
#include <fcntl.h>
#include <posix_spawn_compat.h>
#include <stdio.h>
#include <string.h>
#include <sys/file.h>
#include <unistd.h>

extern char **environ;

static int open_lock_file(const char *path) {
    int fd = open(path, O_CREAT | O_RDWR, 0600);
    if (fd < 0)
        perror("open lock file");
    return fd;
}

static int try_lock(const char *path, int expect_blocked) {
    int fd = open_lock_file(path);
    if (fd < 0)
        return 1;
    if (flock(fd, LOCK_EX | LOCK_NB) != 0) {
        if (expect_blocked && (errno == EAGAIN || errno == EWOULDBLOCK)) {
            close(fd);
            return 0;
        }
        perror("flock try");
        close(fd);
        return 1;
    }
    if (expect_blocked) {
        fprintf(stderr, "contending flock unexpectedly succeeded\n");
        close(fd);
        return 1;
    }
    if (flock(fd, LOCK_UN) != 0) {
        perror("flock unlock");
        close(fd);
        return 1;
    }
    return close(fd) != 0;
}

static int hold_lock(const char *path, const char *ready_path) {
    int fd = open_lock_file(path);
    if (fd < 0)
        return 1;
    if (flock(fd, LOCK_EX) != 0) {
        perror("flock hold");
        return 1;
    }
    int ready = open(ready_path, O_CREAT | O_TRUNC | O_WRONLY, 0600);
    if (ready < 0 || write(ready, "ready", 5) != 5 || close(ready) != 0) {
        perror("write ready marker");
        return 1;
    }
    sleep(2);
    if (flock(fd, LOCK_UN) != 0) {
        perror("flock unlock");
        return 1;
    }
    return close(fd) != 0;
}

static int selftest(const char *argv0, const char *path) {
    int fd = open_lock_file(path);
    if (fd < 0)
        return 1;
    if (flock(fd, LOCK_EX) != 0) {
        perror("flock parent hold");
        return 1;
    }
    char *child_argv[] = {
        (char *)argv0,
        (char *)"try-blocked",
        (char *)path,
        NULL,
    };
    pid_t child;
    int error = posix_spawnp(&child, argv0, NULL, NULL, child_argv, environ);
    if (error != 0) {
        errno = error;
        perror("posix_spawnp");
        return 1;
    }
    int status;
    if (waitpid(child, &status, 0) < 0) {
        perror("waitpid");
        return 1;
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        fprintf(stderr, "lock contender failed: status=%d\n", status);
        return 1;
    }
    if (flock(fd, LOCK_UN) != 0) {
        perror("flock parent unlock");
        return 1;
    }
    if (close(fd) != 0)
        return 1;
    if (try_lock(path, 0) != 0)
        return 1;
    puts("flock=ok");
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 4 && strcmp(argv[1], "hold") == 0)
        return hold_lock(argv[2], argv[3]);
    if (argc == 3 && strcmp(argv[1], "try-blocked") == 0)
        return try_lock(argv[2], 1);
    if (argc == 3 && strcmp(argv[1], "selftest") == 0)
        return selftest(argv[0], argv[2]);
    fprintf(stderr, "usage: flock_test hold path ready-path | selftest path\n");
    return 2;
}
