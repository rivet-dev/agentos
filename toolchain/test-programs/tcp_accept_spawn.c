#include <arpa/inet.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#include "posix_spawn_compat.h"

extern char **environ;

int main(void) {
    int listener_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (listener_fd < 0) {
        perror("socket");
        return 1;
    }

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    int port = 31000 + (getpid() % 10000);
    addr.sin_port = htons((uint16_t)port);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);

    if (bind(listener_fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        perror("bind");
        close(listener_fd);
        return 1;
    }
    if (listen(listener_fd, 1) != 0) {
        perror("listen");
        close(listener_fd);
        return 1;
    }

    char delay_arg[16];
    char port_arg[16];
    snprintf(delay_arg, sizeof(delay_arg), "%d", 100);
    snprintf(port_arg, sizeof(port_arg), "%d", port);

    char *argv[] = {"delayed_tcp_echo", delay_arg, port_arg, NULL};
    pid_t child;
    int spawn_err = posix_spawnp(&child, "delayed_tcp_echo", NULL, NULL, argv, environ);
    if (spawn_err != 0) {
        fprintf(stderr, "posix_spawn delayed_tcp_echo failed: %d\n", spawn_err);
        close(listener_fd);
        return 1;
    }

    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);
    int client_fd = accept(listener_fd, (struct sockaddr *)&client_addr, &client_len);
    if (client_fd < 0) {
        perror("accept");
        close(listener_fd);
        return 1;
    }

    char buf[16] = {0};
    ssize_t n = recv(client_fd, buf, sizeof(buf) - 1, 0);
    if (n < 0) {
        perror("recv");
        close(client_fd);
        close(listener_fd);
        return 1;
    }
    buf[n] = '\0';
    if (send(client_fd, "pong", 4, 0) != 4) {
        perror("send");
        close(client_fd);
        close(listener_fd);
        return 1;
    }

    int status = 0;
    if (waitpid(child, &status, 0) < 0) {
        perror("waitpid");
        close(client_fd);
        close(listener_fd);
        return 1;
    }

    close(client_fd);
    close(listener_fd);

    printf("accept_child_message=%s\n", strcmp(buf, "hello") == 0 ? "yes" : "no");
    printf("accept_child_exit=%d\n", WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status));

    return strcmp(buf, "hello") == 0 && WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 1;
}
