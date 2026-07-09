#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

int main(void) {
    int high_fd = fcntl(STDOUT_FILENO, F_DUPFD, 512);
    if (high_fd < 0) {
        perror("F_DUPFD");
        return 1;
    }

    closefrom(high_fd);
    errno = 0;
    int result = fcntl(high_fd, F_GETFD);
    int closed = result == -1 && errno == EBADF;
    printf("closefrom_closed=%s\n", closed ? "yes" : "no");
    return closed ? 0 : 1;
}
