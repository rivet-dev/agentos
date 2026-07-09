#include <errno.h>
#include <dirent.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: openat_test DIRECTORY\n");
        return 2;
    }

    int directory_fd = open(argv[1], 0);
    if (directory_fd < 0) {
        perror("open directory");
        return 1;
    }
    DIR *directory = fdopendir(directory_fd);
    if (!directory) {
        perror("fdopendir");
        close(directory_fd);
        return 1;
    }
    while (readdir(directory)) {
    }
    directory_fd = dirfd(directory);

    int fd = openat(directory_fd, "payload", 0);
    if (fd < 0) {
        fprintf(stderr, "openat: %s\n", strerror(errno));
        closedir(directory);
        return 1;
    }

    char buffer[8] = {0};
    ssize_t length = read(fd, buffer, sizeof(buffer));
    int failed = length != 7 || memcmp(buffer, "openat\n", 7) != 0;
    if (close(fd) != 0 || closedir(directory) != 0) {
        perror("close");
        return 1;
    }
    if (failed) {
        fprintf(stderr, "openat payload mismatch\n");
        return 1;
    }
    puts("openat: ok");
    return 0;
}
