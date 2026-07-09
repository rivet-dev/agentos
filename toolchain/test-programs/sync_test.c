#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: sync_test FILE\n");
        return 2;
    }

    int fd = open(argv[1], O_CREAT | O_TRUNC | O_WRONLY, 0600);
    if (fd < 0) {
        perror("open write");
        return 1;
    }
    if (write(fd, "durable", 7) != 7) {
        perror("write");
        close(fd);
        return 1;
    }
    if (syncfs(fd) != 0) {
        perror("syncfs");
        close(fd);
        return 1;
    }
    sync();
    if (close(fd) != 0) {
        perror("close write");
        return 1;
    }

    fd = open(argv[1], O_RDONLY);
    if (fd < 0) {
        perror("open read");
        return 1;
    }
    char actual[8] = {0};
    if (read(fd, actual, 7) != 7 || memcmp(actual, "durable", 7) != 0) {
        fprintf(stderr, "sync payload mismatch\n");
        close(fd);
        return 1;
    }
    if (close(fd) != 0) {
        perror("close read");
        return 1;
    }
    errno = 0;
    if (syncfs(-1) != -1 || errno != EBADF) {
        fprintf(stderr, "syncfs invalid-fd errno mismatch: %d\n", errno);
        return 1;
    }
    puts("sync: ok");
    return 0;
}
