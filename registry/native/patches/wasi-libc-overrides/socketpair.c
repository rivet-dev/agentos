#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>

/*
 * socketpair(2) guarantees a connected, indistinguishable pair. The current
 * standard syscall provider has the primitives needed to construct a stream
 * pair but no atomic socketpair operation, so libc uses a private loopback
 * listener. This is the same portable construction used by OpenSSL on systems
 * without socketpair and adds no host capability.
 * POSIX: https://pubs.opengroup.org/onlinepubs/9799919799/functions/socketpair.html
 * Linux: https://man7.org/linux/man-pages/man2/socketpair.2.html
 */
int socketpair(int domain, int type, int protocol, int pair[2]) {
    int base_type = type & ~(SOCK_CLOEXEC | SOCK_NONBLOCK);
    int listener = -1;
    int writer = -1;
    int reader = -1;
    int saved_errno;
    struct sockaddr_in address = {0};
    socklen_t address_len = sizeof(address);

    if (pair == NULL) {
        errno = EFAULT;
        return -1;
    }
    if ((domain != AF_UNIX && domain != AF_INET) || base_type != SOCK_STREAM) {
        errno = EPROTONOSUPPORT;
        return -1;
    }

    listener = socket(AF_INET, SOCK_STREAM, protocol);
    if (listener < 0)
        goto fail;

    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = 0;
    if (bind(listener, (const struct sockaddr *)&address, sizeof(address)) < 0 ||
        getsockname(listener, (struct sockaddr *)&address, &address_len) < 0 ||
        listen(listener, 1) < 0)
        goto fail;

    writer = socket(AF_INET, SOCK_STREAM, protocol);
    if (writer < 0 ||
        connect(writer, (const struct sockaddr *)&address, address_len) < 0)
        goto fail;

    reader = accept(listener, NULL, NULL);
    if (reader < 0)
        goto fail;
    close(listener);
    listener = -1;

    if ((type & SOCK_CLOEXEC) != 0 &&
        (fcntl(reader, F_SETFD, FD_CLOEXEC) < 0 ||
         fcntl(writer, F_SETFD, FD_CLOEXEC) < 0))
        goto fail;
    if ((type & SOCK_NONBLOCK) != 0 &&
        (fcntl(reader, F_SETFL, O_NONBLOCK) < 0 ||
         fcntl(writer, F_SETFL, O_NONBLOCK) < 0))
        goto fail;

    pair[0] = reader;
    pair[1] = writer;
    return 0;

fail:
    saved_errno = errno;
    if (reader >= 0)
        close(reader);
    if (writer >= 0)
        close(writer);
    if (listener >= 0)
        close(listener);
    errno = saved_errno;
    return -1;
}
