/* pipe_test.c — create pipe, write then read through it */
#include <stdio.h>
#include <unistd.h>
#include <string.h>

int main(void) {
    int fds[2];
    if (pipe(fds) != 0) {
        perror("pipe");
        return 1;
    }

    const char *msg = "hello through pipe";
    ssize_t written = write(fds[1], msg, strlen(msg));
    close(fds[1]);

    char buf[256];
    ssize_t n = read(fds[0], buf, sizeof(buf));
    close(fds[0]);

    if (n > 0) {
        buf[n] = '\0';
        printf("read: %s\n", buf);
        printf("bytes: %zd\n", n);
    } else {
        printf("read failed\n");
        return 1;
    }

    printf("written: %zd\n", written);
    return 0;
}
