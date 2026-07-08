/* unix_socket.c — AF_UNIX server: bind, listen, accept one connection, recv, send "pong", close */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>

#ifdef __has_include
#  if __has_include(<sys/un.h>)
#    include <sys/un.h>
#  endif
#endif

#ifndef AF_UNIX
#  define AF_UNIX 1
#endif

#ifndef AF_LOCAL
#  define AF_LOCAL AF_UNIX
#endif

#ifndef offsetof
#  define offsetof(type, member) __builtin_offsetof(type, member)
#endif

/* Fallback if sys/un.h was not available */
#ifndef SUN_LEN
struct sockaddr_un {
    sa_family_t sun_family;
    char sun_path[108];
};
#define SUN_LEN(su) (offsetof(struct sockaddr_un, sun_path) + strlen((su)->sun_path))
#endif

int main(int argc, char *argv[]) {
    const char *path = "/tmp/test.sock";
    if (argc >= 2) {
        path = argv[1];
    }

    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        perror("socket");
        return 1;
    }

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, path, sizeof(addr.sun_path) - 1);

    if (bind(fd, (struct sockaddr *)&addr, SUN_LEN(&addr)) < 0) {
        perror("bind");
        close(fd);
        return 1;
    }

    if (listen(fd, 1) < 0) {
        perror("listen");
        close(fd);
        return 1;
    }

    printf("listening on %s\n", path);
    fflush(stdout);

    struct sockaddr_un client_addr;
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
