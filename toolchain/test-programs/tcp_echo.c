/* tcp_echo.c — connect to a TCP echo server, send "hello", receive response */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

int main(int argc, char *argv[]) {
    if (argc < 2) {
        fprintf(stderr, "usage: tcp_echo <port>\n");
        return 1;
    }

    int port = atoi(argv[1]);

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

    const char *msg = "hello";
    ssize_t sent = send(fd, msg, strlen(msg), 0);
    if (sent < 0) {
        perror("send");
        close(fd);
        return 1;
    }

    char buf[256];
    ssize_t n = recv(fd, buf, sizeof(buf) - 1, 0);
    if (n < 0) {
        perror("recv");
        close(fd);
        return 1;
    }
    buf[n] = '\0';

    printf("sent: %zd\n", sent);
    printf("received: %s\n", buf);

    close(fd);
    return 0;
}
