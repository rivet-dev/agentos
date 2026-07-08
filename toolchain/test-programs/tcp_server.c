/* tcp_server.c — bind, listen, accept one connection, recv, send "pong", close */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

int main(int argc, char *argv[]) {
    if (argc < 2) {
        fprintf(stderr, "usage: tcp_server <port>\n");
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
    addr.sin_addr.s_addr = htonl(INADDR_ANY);

    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        perror("bind");
        close(fd);
        return 1;
    }

    if (listen(fd, 1) < 0) {
        perror("listen");
        close(fd);
        return 1;
    }

    printf("listening on port %d\n", port);
    fflush(stdout);

    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);
    int client_fd = accept(fd, (struct sockaddr *)&client_addr, &client_len);
    if (client_fd < 0) {
        perror("accept");
        close(fd);
        return 1;
    }

    char buf[256];
    ssize_t n = recv(client_fd, buf, sizeof(buf) - 1, 0);
    if (n < 0) {
        perror("recv");
        close(client_fd);
        close(fd);
        return 1;
    }
    buf[n] = '\0';

    printf("received: %s\n", buf);

    const char *reply = "pong";
    ssize_t sent = send(client_fd, reply, strlen(reply), 0);
    if (sent < 0) {
        perror("send");
        close(client_fd);
        close(fd);
        return 1;
    }

    printf("sent: %zd\n", sent);

    close(client_fd);
    close(fd);
    return 0;
}
