/*
 * AF_UNIX socketpair and SCM_RIGHTS support for the AgentOS sysroot.
 *
 * These calls use kernel-backed process descriptors through host_process,
 * rather than runner-local host_net handles, because socketpair endpoints and
 * passed file descriptions must survive posix_spawn inheritance. The kernel
 * owns buffering, poll/HUP/shutdown state, ancillary queues, resource limits,
 * and atomic descriptor installation.
 */

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <unistd.h>

#ifndef IOV_MAX
#define IOV_MAX 1024
#endif

#define AGENTOS_MAX_SOCKET_MESSAGE_BYTES (64u * 1024u * 1024u)
#define SCM_MAX_FD 253u

#define WASM_IMPORT(mod, fn) \
    __attribute__((__import_module__(mod), __import_name__(fn)))

WASM_IMPORT("host_process", "fd_socketpair")
uint32_t __host_fd_socketpair(uint32_t socket_kind, uint32_t nonblocking,
    uint32_t close_on_exec, uint32_t *ret_first, uint32_t *ret_second);

WASM_IMPORT("host_process", "fd_sendmsg_rights")
uint32_t __host_fd_sendmsg_rights(uint32_t socket_fd,
    const uint8_t *data, uint32_t data_len,
    const uint32_t *rights, uint32_t rights_len,
    uint32_t flags, uint32_t *ret_sent);

WASM_IMPORT("host_process", "fd_recvmsg_rights")
uint32_t __host_fd_recvmsg_rights(uint32_t socket_fd,
    uint8_t *data, uint32_t data_len,
    uint32_t *rights, uint32_t rights_capacity,
    uint32_t flags, uint32_t *ret_received,
    uint32_t *ret_rights_len, uint32_t *ret_msg_flags);

WASM_IMPORT("host_net", "net_recvmsg")
uint32_t __host_net_recvmsg(uint32_t fd, struct msghdr *message,
    uint32_t flags, uint32_t *ret_received);

int __agentos_set_cloexec_fd(int fd, int enabled);

static int checked_iov_bytes(const struct iovec *iov, size_t iovlen,
    uint32_t *total_out) {
    size_t total = 0;

    if (iovlen > IOV_MAX) {
        errno = EINVAL;
        return -1;
    }
    if (iovlen != 0 && iov == NULL) {
        errno = EFAULT;
        return -1;
    }
    for (size_t i = 0; i < iovlen; i++) {
        if (iov[i].iov_len != 0 && iov[i].iov_base == NULL) {
            errno = EFAULT;
            return -1;
        }
        if (iov[i].iov_len > UINT32_MAX - total) {
            errno = EMSGSIZE;
            return -1;
        }
        total += iov[i].iov_len;
        if (total > AGENTOS_MAX_SOCKET_MESSAGE_BYTES) {
            errno = EMSGSIZE;
            return -1;
        }
    }
    *total_out = (uint32_t)total;
    return 0;
}

static uint8_t *gather_iov(const struct iovec *iov, size_t iovlen,
    uint32_t total) {
    uint8_t *buffer = malloc(total == 0 ? 1 : total);
    size_t offset = 0;

    if (buffer == NULL) {
        errno = ENOMEM;
        return NULL;
    }
    for (size_t i = 0; i < iovlen; i++) {
        memcpy(buffer + offset, iov[i].iov_base, iov[i].iov_len);
        offset += iov[i].iov_len;
    }
    return buffer;
}

static void scatter_iov(const uint8_t *buffer, uint32_t length,
    const struct iovec *iov, size_t iovlen) {
    size_t offset = 0;

    for (size_t i = 0; i < iovlen && offset < length; i++) {
        size_t chunk = iov[i].iov_len;
        if (chunk > (size_t)length - offset)
            chunk = (size_t)length - offset;
        memcpy(iov[i].iov_base, buffer + offset, chunk);
        offset += chunk;
    }
}

int socketpair(int domain, int type, int protocol, int sv[2]) {
    uint32_t first, second;
    uint32_t error;

    if (sv == NULL) {
        errno = EFAULT;
        return -1;
    }
    if (domain != AF_UNIX) {
        errno = EOPNOTSUPP;
        return -1;
    }
    if (type & ~(0x0f | SOCK_NONBLOCK | SOCK_CLOEXEC)) {
        errno = EINVAL;
        return -1;
    }
    int socket_type = type & 0x0f;
    if (socket_type != SOCK_STREAM && socket_type != SOCK_DGRAM &&
        socket_type != SOCK_SEQPACKET) {
        errno = ESOCKTNOSUPPORT;
        return -1;
    }
    /* Linux AF_UNIX accepts protocol 0 and PF_UNIX (1). */
    if (protocol != 0 && protocol != 1) {
        errno = EPROTONOSUPPORT;
        return -1;
    }

    uint32_t socket_kind = socket_type == SOCK_STREAM ? 1u :
        socket_type == SOCK_DGRAM ? 2u : 3u;
    error = __host_fd_socketpair(socket_kind,
        (type & SOCK_NONBLOCK) != 0, (type & SOCK_CLOEXEC) != 0,
        &first, &second);
    if (error != 0) {
        errno = (int)error;
        return -1;
    }

    sv[0] = (int)first;
    sv[1] = (int)second;
    if (__agentos_set_cloexec_fd(sv[0],
        (type & SOCK_CLOEXEC) != 0) < 0 ||
        __agentos_set_cloexec_fd(sv[1],
        (type & SOCK_CLOEXEC) != 0) < 0) {
        int saved_errno = errno;
        close(sv[0]);
        close(sv[1]);
        errno = saved_errno;
        return -1;
    }
    return 0;
}

static int collect_rights(const struct msghdr *message,
    uint32_t **rights_out, uint32_t *rights_len_out) {
    struct cmsghdr *control;
    uint32_t *rights = NULL;
    size_t rights_len = 0;

    *rights_out = NULL;
    *rights_len_out = 0;
    if (message->msg_controllen == 0)
        return 0;
    if (message->msg_control == NULL) {
        errno = EFAULT;
        return -1;
    }

    for (control = CMSG_FIRSTHDR(message); control != NULL;
        control = CMSG_NXTHDR((struct msghdr *)message, control)) {
        size_t payload_len;
        size_t count;
        uint32_t *next;

        if (control->cmsg_len < CMSG_LEN(0) ||
            (uint8_t *)control + control->cmsg_len >
                (uint8_t *)message->msg_control + message->msg_controllen) {
            free(rights);
            errno = EINVAL;
            return -1;
        }
        if (control->cmsg_level != SOL_SOCKET ||
            control->cmsg_type != SCM_RIGHTS) {
            free(rights);
            errno = EINVAL;
            return -1;
        }
        payload_len = control->cmsg_len - CMSG_LEN(0);
        if (payload_len == 0 || payload_len % sizeof(int) != 0) {
            free(rights);
            errno = EINVAL;
            return -1;
        }
        count = payload_len / sizeof(int);
        if (count > UINT32_MAX - rights_len ||
            rights_len + count > SCM_MAX_FD) {
            free(rights);
            errno = EINVAL;
            return -1;
        }
        next = realloc(rights, (rights_len + count) * sizeof(*rights));
        if (next == NULL) {
            free(rights);
            errno = ENOMEM;
            return -1;
        }
        rights = next;
        for (size_t i = 0; i < count; i++) {
            int fd;
            memcpy(&fd, CMSG_DATA(control) + i * sizeof(fd), sizeof(fd));
            if (fd < 0) {
                free(rights);
                errno = EBADF;
                return -1;
            }
            rights[rights_len + i] = (uint32_t)fd;
        }
        rights_len += count;
    }

    *rights_out = rights;
    *rights_len_out = (uint32_t)rights_len;
    return 0;
}

ssize_t sendmsg(int fd, const struct msghdr *message, int flags) {
    uint32_t total;
    uint8_t *data;
    uint32_t *rights;
    uint32_t rights_len;
    uint32_t sent = 0;
    uint32_t error;

    if (message == NULL) {
        errno = EFAULT;
        return -1;
    }
    if (flags & ~(MSG_DONTWAIT | MSG_NOSIGNAL)) {
        errno = EOPNOTSUPP;
        return -1;
    }
    if (checked_iov_bytes(message->msg_iov, message->msg_iovlen, &total) < 0)
        return -1;
    data = gather_iov(message->msg_iov, message->msg_iovlen, total);
    if (data == NULL)
        return -1;
    if (collect_rights(message, &rights, &rights_len) < 0) {
        free(data);
        return -1;
    }

    if (rights_len != 0 && message->msg_name != NULL) {
        free(rights);
        free(data);
        errno = EISCONN;
        return -1;
    }

    error = __host_fd_sendmsg_rights((uint32_t)fd, data, total,
        rights, rights_len, (uint32_t)flags, &sent);
    if (error == (uint32_t)ENOTSOCK && rights_len == 0) {
        ssize_t result;
        if (message->msg_name != NULL)
            result = sendto(fd, data, total, flags, message->msg_name,
                message->msg_namelen);
        else
            result = send(fd, data, total, flags);
        free(rights);
        free(data);
        return result;
    }
    free(rights);
    free(data);
    if (error != 0) {
        errno = (int)error;
        return -1;
    }
    return (ssize_t)sent;
}

ssize_t recvmsg(int fd, struct msghdr *message, int flags) {
    uint32_t total;
    uint8_t *data;
    uint32_t rights_capacity = 0;
    uint32_t *rights = NULL;
    uint32_t received = 0;
    uint32_t rights_len = 0;
    uint32_t host_message_flags = 0;
    uint32_t error;

    if (message == NULL) {
        errno = EFAULT;
        return -1;
    }
    if (flags & ~(MSG_DONTWAIT | MSG_CMSG_CLOEXEC | MSG_PEEK | MSG_TRUNC |
        MSG_WAITALL)) {
        errno = EOPNOTSUPP;
        return -1;
    }
    error = __host_net_recvmsg((uint32_t)fd, message, (uint32_t)flags, &received);
    if (error == 0) return (ssize_t)received;
    if (error != (uint32_t)ENOTSOCK) { errno = (int)error; return -1; }
    if (checked_iov_bytes(message->msg_iov, message->msg_iovlen, &total) < 0)
        return -1;
    data = malloc(total == 0 ? 1 : total);
    if (data == NULL) {
        errno = ENOMEM;
        return -1;
    }

    if (message->msg_control != NULL &&
        message->msg_controllen >= CMSG_SPACE(sizeof(int))) {
        rights_capacity = (uint32_t)((message->msg_controllen -
            CMSG_SPACE(0)) / sizeof(int));
        rights = malloc(rights_capacity * sizeof(*rights));
        if (rights == NULL) {
            free(data);
            errno = ENOMEM;
            return -1;
        }
    }

    error = __host_fd_recvmsg_rights((uint32_t)fd, data, total,
        rights, rights_capacity, (uint32_t)flags, &received,
        &rights_len, &host_message_flags);
    if (error == (uint32_t)ENOTSOCK) {
        ssize_t result;
        int socket_type = 0;
        socklen_t socket_type_len = sizeof(socket_type);
        if (getsockopt(fd, SOL_SOCKET, SO_TYPE, &socket_type, &socket_type_len) < 0) {
            free(rights);
            free(data);
            return -1;
        }
        const int datagram = socket_type == SOCK_DGRAM;
        /* Request full datagram length to report output MSG_TRUNC even when
         * the caller did not request a full-length return value. Streams keep
         * their existing receive flags and partial-read semantics. */
        const int receive_flags = (flags & ~MSG_CMSG_CLOEXEC) |
            (datagram ? MSG_TRUNC : 0);
        if (message->msg_name != NULL) {
            socklen_t name_length = message->msg_namelen;
            result = recvfrom(fd, data, total, receive_flags,
                message->msg_name,
                &name_length);
            message->msg_namelen = name_length;
        } else {
            result = recv(fd, data, total, receive_flags);
            message->msg_namelen = 0;
        }
        if (result >= 0) {
            const uint32_t copied = (uint64_t)result > total ? total : (uint32_t)result;
            scatter_iov(data, copied, message->msg_iov, message->msg_iovlen);
            message->msg_flags = datagram && (uint64_t)result > total ? MSG_TRUNC : 0;
            if (datagram && (flags & MSG_TRUNC) == 0) result = copied;
            message->msg_controllen = 0;
        }
        free(rights);
        free(data);
        return result;
    }
    if (error != 0) {
        free(rights);
        free(data);
        errno = (int)error;
        return -1;
    }

    if (rights_len > rights_capacity) {
        /*
         * Linux returns the payload and reports ancillary truncation. The
         * host installs at most rights_capacity descriptions and closes any
         * excess atomically, so only the bounded prefix is visible here.
         */
        rights_len = rights_capacity;
        host_message_flags |= 2u;
    }
    scatter_iov(data, received, message->msg_iov, message->msg_iovlen);
    free(data);
    uint32_t full_length = host_message_flags >> 2;
    message->msg_flags =
        ((host_message_flags & 1u) != 0 ? MSG_TRUNC : 0) |
        ((host_message_flags & 2u) != 0 ? MSG_CTRUNC : 0);
    message->msg_namelen = 0;

    if (rights_len > 0 && rights_capacity > 0) {
        struct cmsghdr *control;
        size_t used = CMSG_SPACE((size_t)rights_len * sizeof(int));

        memset(message->msg_control, 0, used);
        control = (struct cmsghdr *)message->msg_control;
        control->cmsg_len = CMSG_LEN((size_t)rights_len * sizeof(int));
        control->cmsg_level = SOL_SOCKET;
        control->cmsg_type = SCM_RIGHTS;
        for (uint32_t i = 0; i < rights_len; i++) {
            int installed = (int)rights[i];
            memcpy(CMSG_DATA(control) + i * sizeof(installed),
                &installed, sizeof(installed));
            if (__agentos_set_cloexec_fd(installed,
                (flags & MSG_CMSG_CLOEXEC) != 0) < 0) {
                int saved_errno = errno;
                for (uint32_t j = 0; j < rights_len; j++)
                    close((int)rights[j]);
                free(rights);
                errno = saved_errno;
                return -1;
            }
        }
        message->msg_controllen = used;
    } else {
        message->msg_controllen = 0;
    }
    free(rights);
    return (ssize_t)(((flags & MSG_TRUNC) != 0 && full_length > received) ?
        full_length : received);
}
