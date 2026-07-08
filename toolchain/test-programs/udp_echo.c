/* udp_echo.c — bind UDP socket, recv datagram, echo it back, then exit */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

int main(int argc, char *argv[]) {
    if (argc < 2) {
        fprintf(stderr, "usage: udp_echo <port>\n");
        return 1;
    }

    int port = atoi(argv[1]);

    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) {
        perror("socket");
        return 1;
    }

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    addr.sin_addr.s_addr = htonl(INADDR_ANY);

    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        perror("bind");
        close(fd);
        return 1;
    }

    printf("listening on port %d\n", port);
    fflush(stdout);

    /* Echo one datagram and exit */
    char buf[1024];
    struct sockaddr_in src_addr;
    socklen_t src_len = sizeof(src_addr);

    ssize_t n = recvfrom(fd, buf, sizeof(buf) - 1, 0,
                         (struct sockaddr *)&src_addr, &src_len);
    if (n < 0) {
        perror("recvfrom");
        close(fd);
        return 1;
    }
    buf[n] = '\0';

    printf("received: %s\n", buf);

    ssize_t sent = sendto(fd, buf, (size_t)n, 0,
                          (struct sockaddr *)&src_addr, src_len);
    if (sent < 0) {
        perror("sendto");
        close(fd);
        return 1;
    }

    printf("echoed: %zd\n", sent);

    close(fd);
    return 0;
}
