/* delayed_tcp_echo.c -- sleep briefly, connect to loopback, send hello, read pong */
#include <arpa/inet.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char *argv[]) {
    if (argc < 3) {
        fprintf(stderr, "usage: delayed_tcp_echo <delay_ms> <port>\n");
        return 1;
    }

    int delay_ms = atoi(argv[1]);
    int port = atoi(argv[2]);
    if (delay_ms > 0) {
        usleep((useconds_t)delay_ms * 1000u);
    }

    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) {
        perror("socket");
        return 1;
    }

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);

    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        perror("connect");
        close(fd);
        return 1;
    }

    if (send(fd, "hello", 5, 0) != 5) {
        perror("send");
        close(fd);
        return 1;
    }

    char buf[8] = {0};
    ssize_t n = recv(fd, buf, sizeof(buf) - 1, 0);
    if (n < 0) {
        perror("recv");
        close(fd);
        return 1;
    }

    close(fd);
    return strcmp(buf, "pong") == 0 ? 0 : 1;
}
