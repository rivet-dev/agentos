#include <arpa/inet.h>
#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int write_all(int fd, const void *data, size_t length) {
    const unsigned char *cursor = data;

    while (length > 0) {
        ssize_t written = write(fd, cursor, length);
        if (written < 0 && errno == EINTR)
            continue;
        if (written <= 0)
            return -1;
        cursor += written;
        length -= (size_t)written;
    }
    return 0;
}

static int connect_ipv4(const char *host, const char *port_string) {
    char *end = NULL;
    long port = strtol(port_string, &end, 10);
    struct sockaddr_in address;
    int fd;

    if (end == port_string || *end != '\0' || port <= 0 || port > 65535) {
        errno = EINVAL;
        return -1;
    }
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons((unsigned short)port);
    if (inet_pton(AF_INET, host, &address.sin_addr) != 1) {
        errno = EINVAL;
        return -1;
    }
    fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0)
        return -1;
    if (connect(fd, (struct sockaddr *)&address, sizeof(address)) < 0) {
        int saved_errno = errno;
        close(fd);
        errno = saved_errno;
        return -1;
    }
    return fd;
}

static int proxy_stdio(int socket_fd) {
    int input_open = 1;
    unsigned char buffer[16384];

    for (;;) {
        struct pollfd poll_fds[2] = {
            { .fd = STDIN_FILENO, .events = input_open ? POLLIN : 0 },
            { .fd = socket_fd, .events = POLLIN },
        };
        int ready;
        do {
            ready = poll(poll_fds, 2, -1);
        } while (ready < 0 && errno == EINTR);
        if (ready < 0)
            return -1;

        if (input_open && (poll_fds[0].revents & (POLLIN | POLLHUP))) {
            ssize_t length = read(STDIN_FILENO, buffer, sizeof(buffer));
            if (length < 0 && errno == EINTR)
                continue;
            if (length < 0)
                return -1;
            if (length == 0) {
                input_open = 0;
                if (shutdown(socket_fd, SHUT_WR) < 0 && errno != ENOTCONN)
                    return -1;
            } else if (write_all(socket_fd, buffer, (size_t)length) < 0) {
                return -1;
            }
        }

        if (poll_fds[1].revents & (POLLIN | POLLHUP)) {
            ssize_t length = read(socket_fd, buffer, sizeof(buffer));
            if (length < 0 && errno == EINTR)
                continue;
            if (length < 0)
                return -1;
            if (length == 0)
                return 0;
            if (write_all(STDOUT_FILENO, buffer, (size_t)length) < 0)
                return -1;
        }
        if (poll_fds[0].revents & (POLLERR | POLLNVAL)) {
            errno = EIO;
            return -1;
        }
        if (poll_fds[1].revents & (POLLERR | POLLNVAL)) {
            errno = EIO;
            return -1;
        }
    }
}

static int proxy_fdpass(int socket_fd) {
    char marker = 'F';
    struct iovec iov = { .iov_base = &marker, .iov_len = 1 };
    union {
        struct cmsghdr align;
        unsigned char bytes[CMSG_SPACE(sizeof(int))];
    } control;
    struct msghdr message;
    struct cmsghdr *header;

    memset(&message, 0, sizeof(message));
    memset(&control, 0, sizeof(control));
    message.msg_iov = &iov;
    message.msg_iovlen = 1;
    message.msg_control = control.bytes;
    message.msg_controllen = sizeof(control.bytes);
    header = CMSG_FIRSTHDR(&message);
    header->cmsg_level = SOL_SOCKET;
    header->cmsg_type = SCM_RIGHTS;
    header->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(header), &socket_fd, sizeof(socket_fd));
    return sendmsg(STDOUT_FILENO, &message, 0) == 1 ? 0 : -1;
}

int main(int argc, char **argv) {
    int socket_fd;
    int result;

    if (argc != 4 ||
        (strcmp(argv[1], "stdio") != 0 && strcmp(argv[1], "fdpass") != 0)) {
        fprintf(stderr, "usage: ssh_proxy_helper {stdio|fdpass} HOST PORT\n");
        return 2;
    }
    socket_fd = connect_ipv4(argv[2], argv[3]);
    if (socket_fd < 0) {
        perror("ssh_proxy_helper connect");
        return 1;
    }
    result = strcmp(argv[1], "fdpass") == 0 ?
        proxy_fdpass(socket_fd) : proxy_stdio(socket_fd);
    if (result < 0)
        perror("ssh_proxy_helper");
    close(socket_fd);
    return result == 0 ? 0 : 1;
}
