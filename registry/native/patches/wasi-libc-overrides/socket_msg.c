/*
 * POSIX sendmsg(3)/recvmsg(3) over AgentOS's existing sendto/recvfrom libc
 * surface. The Linux msghdr/iovec layout comes from sys/socket.h and sys/uio.h.
 * Scatter/gather work is bounded to UIO_MAXIOV and 16 MiB per call.
 */

#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>

#define AGENTOS_MSG_MAX_BYTES (16u * 1024u * 1024u)

static int message_size(const struct msghdr *msg, size_t *total) {
    if (msg == NULL || total == NULL || msg->msg_iovlen < 0 ||
        msg->msg_iovlen > UIO_MAXIOV ||
        (msg->msg_iovlen != 0 && msg->msg_iov == NULL)) {
        errno = EINVAL;
        return -1;
    }

    size_t size = 0;
    for (int i = 0; i < msg->msg_iovlen; ++i) {
        size_t len = msg->msg_iov[i].iov_len;
        if (len > AGENTOS_MSG_MAX_BYTES - size) {
            errno = EMSGSIZE;
            return -1;
        }
        size += len;
    }
    *total = size;
    return 0;
}

ssize_t sendmsg(int fd, const struct msghdr *msg, int flags) {
    size_t total;
    if (message_size(msg, &total) != 0)
        return -1;
    if (msg->msg_controllen != 0) {
        errno = EOPNOTSUPP;
        return -1;
    }

    if (msg->msg_iovlen == 1)
        return sendto(fd, msg->msg_iov[0].iov_base, total, flags,
                      msg->msg_name, msg->msg_namelen);

    unsigned char empty = 0;
    unsigned char *buffer = total == 0 ? &empty : malloc(total);
    if (buffer == NULL) {
        errno = ENOMEM;
        return -1;
    }

    size_t offset = 0;
    for (int i = 0; i < msg->msg_iovlen; ++i) {
        memcpy(buffer + offset, msg->msg_iov[i].iov_base,
               msg->msg_iov[i].iov_len);
        offset += msg->msg_iov[i].iov_len;
    }
    ssize_t result = sendto(fd, buffer, total, flags,
                            msg->msg_name, msg->msg_namelen);
    if (total != 0)
        free(buffer);
    return result;
}

ssize_t recvmsg(int fd, struct msghdr *msg, int flags) {
    size_t total;
    if (message_size(msg, &total) != 0)
        return -1;

    unsigned char empty = 0;
    unsigned char *buffer = total == 0 ? &empty : malloc(total);
    if (buffer == NULL) {
        errno = ENOMEM;
        return -1;
    }

    socklen_t addrlen = msg->msg_namelen;
    ssize_t result = recvfrom(fd, buffer, total, flags, msg->msg_name,
                              msg->msg_name == NULL ? NULL : &addrlen);
    if (result >= 0) {
        size_t remaining = (size_t) result;
        size_t offset = 0;
        for (int i = 0; i < msg->msg_iovlen && remaining != 0; ++i) {
            size_t count = msg->msg_iov[i].iov_len;
            if (count > remaining)
                count = remaining;
            memcpy(msg->msg_iov[i].iov_base, buffer + offset, count);
            offset += count;
            remaining -= count;
        }
        msg->msg_namelen = addrlen;
        msg->msg_controllen = 0;
        msg->msg_flags = 0;
    }
    if (total != 0)
        free(buffer);
    return result;
}
