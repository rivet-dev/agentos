#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/uio.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: pwritev_test FILE\n");
        return 2;
    }
    int fd = open(argv[1], O_CREAT | O_TRUNC | O_RDWR, 0600);
    if (fd < 0 || write(fd, "00xxxxxxxx", 10) != 10) {
        perror("prepare");
        return 1;
    }

    struct iovec vectors[] = {
        {.iov_base = "abc", .iov_len = 3},
        {.iov_base = "def", .iov_len = 3},
    };
    if (pwritev(fd, vectors, 2, 2) != 6) {
        perror("pwritev");
        close(fd);
        return 1;
    }

    char actual[11] = {0};
    if (pread(fd, actual, 10, 0) != 10 || memcmp(actual, "00abcdefxx", 10) != 0) {
        fprintf(stderr, "pwritev payload mismatch\n");
        close(fd);
        return 1;
    }
    if (close(fd) != 0) {
        perror("close");
        return 1;
    }
    puts("pwritev: ok");
    return 0;
}
