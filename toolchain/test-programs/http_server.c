/* http_server.c - bind, listen, accept one HTTP request, reply, close */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

static const char *status_text(int status) {
    return status == 200 ? "OK" : "Internal Server Error";
}

int main(int argc, char *argv[]) {
    if (argc < 2) {
        fprintf(stderr, "usage: http_server <port>\n");
        return 1;
    }

    int port = atoi(argv[1]);

    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) {
        perror("socket");
        return 1;
    }

    int reuse = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));

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

    printf("http listening on port %d\n", port);
    fflush(stdout);

    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);
    int client_fd = accept(fd, (struct sockaddr *)&client_addr, &client_len);
    if (client_fd < 0) {
        perror("accept");
        close(fd);
        return 1;
    }

    char request[2048];
    ssize_t total = 0;
    while (total < (ssize_t)sizeof(request) - 1) {
        ssize_t n = recv(client_fd, request + total, sizeof(request) - 1 - (size_t)total, 0);
        if (n < 0) {
            perror("recv");
            close(client_fd);
            close(fd);
            return 1;
        }
        if (n == 0) {
            break;
        }
        total += n;
        request[total] = '\0';
        if (strstr(request, "\r\n\r\n") != NULL) {
            break;
        }
    }
    request[total] = '\0';

    char method[16] = "";
    char path[512] = "";
    sscanf(request, "%15s %511s", method, path);
    printf("received request: %s %s\n", method, path);
    fflush(stdout);

    int status = 200;
    char body[768];
    int body_len = snprintf(body, sizeof(body), "wasm:%s:%s", method, path);
    if (body_len < 0 || (size_t)body_len >= sizeof(body)) {
        status = 500;
        body_len = snprintf(body, sizeof(body), "wasm:error");
    }

    char response[1400];
    int response_len = snprintf(
        response,
        sizeof(response),
        "HTTP/1.1 %d %s\r\n"
        "Content-Type: text/plain\r\n"
        "Content-Length: %d\r\n"
        "Connection: close\r\n"
        "\r\n"
        "%s",
        status,
        status_text(status),
        body_len,
        body
    );

    if (response_len < 0 || (size_t)response_len >= sizeof(response)) {
        fprintf(stderr, "response too large\n");
        close(client_fd);
        close(fd);
        return 1;
    }

    ssize_t sent = send(client_fd, response, (size_t)response_len, 0);
    if (sent < 0) {
        perror("send");
        close(client_fd);
        close(fd);
        return 1;
    }

    printf("sent response: %zd\n", sent);

    close(client_fd);
    close(fd);
    return 0;
}
